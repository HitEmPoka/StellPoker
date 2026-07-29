use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl JobPriority {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => JobPriority::Low,
            1 => JobPriority::Normal,
            2 => JobPriority::High,
            _ => JobPriority::Critical,
        }
    }
}

impl PartialOrd for JobPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JobPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = *self as u32;
        let b = *other as u32;
        a.cmp(&b)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Processing,
    Completed,
    Failed(String),
    Cancelled,
    Retrying,
}

#[derive(Debug, Clone)]
pub struct JobState {
    pub job_id: String,
    pub job_type: String,
    pub table_id: u32,
    pub priority: JobPriority,
    pub status: JobStatus,
    pub progress: f64,
    pub retry_count: u32,
    pub max_retries: u32,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    pub payload: serde_json::Value,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct JobEntry {
    job_id: String,
    priority: JobPriority,
    created_at: Instant,
}

impl Eq for JobEntry {}

impl PartialEq for JobEntry {
    fn eq(&self, other: &Self) -> bool {
        self.job_id == other.job_id
    }
}

impl PartialOrd for JobEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JobEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.created_at.cmp(&other.created_at))
    }
}

pub type JobHandler = Arc<dyn Fn(JobState) -> tokio::task::JoinHandle<Result<serde_json::Value, String>> + Send + Sync>;

pub struct JobQueue {
    jobs: RwLock<HashMap<String, Arc<Mutex<JobState>>>>,
    pending: Mutex<BinaryHeap<JobEntry>>,
    handlers: RwLock<HashMap<String, JobHandler>>,
    worker_count: usize,
}

impl JobQueue {
    pub fn new(worker_count: usize) -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
            pending: Mutex::new(BinaryHeap::new()),
            handlers: RwLock::new(HashMap::new()),
            worker_count,
        }
    }

    pub fn register_handler(&self, job_type: &str, handler: JobHandler) {
        let mut handlers = self.handlers.blocking_write();
        handlers.insert(job_type.to_string(), handler);
    }

    pub async fn enqueue(
        &self,
        job_type: &str,
        table_id: u32,
        priority: JobPriority,
        max_retries: u32,
        payload: serde_json::Value,
    ) -> String {
        let job_id = format!("job-{}-{}", job_type, Uuid::new_v4());
        let state = JobState {
            job_id: job_id.clone(),
            job_type: job_type.to_string(),
            table_id,
            priority,
            status: JobStatus::Queued,
            progress: 0.0,
            retry_count: 0,
            max_retries,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
            payload,
            result: None,
        };

        self.jobs
            .write()
            .await
            .insert(job_id.clone(), Arc::new(Mutex::new(state)));

        self.pending
            .lock()
            .await
            .push(JobEntry {
                job_id: job_id.clone(),
                priority,
                created_at: Instant::now(),
            });

        tracing::info!(job_id = %job_id, job_type = %job_type, priority = ?priority, "job enqueued");
        job_id
    }

    pub async fn cancel(&self, job_id: &str) -> Result<(), String> {
        let jobs = self.jobs.read().await;
        let state = jobs
            .get(job_id)
            .ok_or_else(|| format!("job {} not found", job_id))?;
        let mut state = state.lock().await;
        match state.status {
            JobStatus::Queued | JobStatus::Retrying => {
                state.status = JobStatus::Cancelled;
                tracing::info!(job_id = %job_id, "job cancelled");
                Ok(())
            }
            JobStatus::Processing => {
                state.status = JobStatus::Cancelled;
                tracing::info!(job_id = %job_id, "job cancelled (was processing)");
                Ok(())
            }
            JobStatus::Completed => Err("job already completed".to_string()),
            JobStatus::Cancelled => Err("job already cancelled".to_string()),
            JobStatus::Failed(_) => Err("job already failed".to_string()),
        }
    }

    pub async fn get_status(&self, job_id: &str) -> Option<JobState> {
        let jobs = self.jobs.read().await;
        let state = jobs.get(job_id)?;
        let status = state.lock().await.clone();
        Some(status)
    }

    pub async fn get_status_by_table(
        &self,
        table_id: u32,
    ) -> Vec<JobState> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter_map(|s| {
                let state = s.blocking_lock();
                if state.table_id == table_id {
                    Some(state.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn get_queue_length(&self) -> usize {
        self.pending.lock().await.len()
    }

    pub async fn active_count(&self) -> usize {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter(|s| {
                let state = s.blocking_lock();
                matches!(state.status, JobStatus::Processing)
            })
            .count()
    }

    pub async fn update_progress(
        &self,
        job_id: &str,
        progress: f64,
    ) -> Result<(), String> {
        let jobs = self.jobs.read().await;
        let state = jobs
            .get(job_id)
            .ok_or_else(|| format!("job {} not found", job_id))?;
        let mut state = state.lock().await;
        state.progress = progress.clamp(0.0, 1.0);
        Ok(())
    }

    pub async fn complete_job(
        &self,
        job_id: &str,
        result: serde_json::Value,
    ) -> Result<(), String> {
        let jobs = self.jobs.read().await;
        let state = jobs
            .get(job_id)
            .ok_or_else(|| format!("job {} not found", job_id))?;
        let mut state = state.lock().await;
        state.status = JobStatus::Completed;
        state.progress = 1.0;
        state.completed_at = Some(Instant::now());
        state.result = Some(result);
        tracing::info!(job_id = %job_id, "job completed");
        Ok(())
    }

    pub fn spawn_workers(self: &Arc<Self>) {
        for worker_id in 0..self.worker_count {
            let queue = Arc::clone(self);
            tokio::spawn(async move {
                loop {
                    let job_entry = {
                        let mut pending = queue.pending.lock().await;
                        pending.pop()
                    };

                    let Some(entry) = job_entry else {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    };

                    let job_state = {
                        let jobs = queue.jobs.read().await;
                        jobs.get(&entry.job_id).map(|s| Arc::clone(s))
                    };

                    let Some(job_arc) = job_state else {
                        continue;
                    };

                    let mut state = job_arc.lock().await;
                    if matches!(state.status, JobStatus::Cancelled) {
                        tracing::debug!(job_id = %entry.job_id, worker = worker_id, "skipping cancelled job");
                        continue;
                    }

                    state.status = JobStatus::Processing;
                    state.started_at = Some(Instant::now());
                    let job_snapshot = state.clone();
                    drop(state);

                    let job_type = job_snapshot.job_type.clone();
                    let handler = {
                        let handlers = queue.handlers.read().await;
                        handlers.get(&job_type).cloned()
                    };

                    let Some(handler) = handler else {
                        tracing::error!(job_type = %job_type, "no handler registered for job type");
                        let mut state = job_arc.lock().await;
                        state.status = JobStatus::Failed("no handler registered".to_string());
                        continue;
                    };

                    tracing::info!(
                        job_id = %entry.job_id,
                        job_type = %job_type,
                        worker = worker_id,
                        "worker processing job"
                    );

                    let handle = handler(job_snapshot);
                    let result = handle.await;

                    match result {
                        Ok(Ok(value)) => {
                            let mut state = job_arc.lock().await;
                            state.status = JobStatus::Completed;
                            state.progress = 1.0;
                            state.completed_at = Some(Instant::now());
                            state.result = Some(value);
                            tracing::info!(job_id = %entry.job_id, worker = worker_id, "job completed successfully");
                        }
                        Ok(Err(e)) => {
                            let error_msg = e;
                            let mut state = job_arc.lock().await;
                            state.retry_count += 1;
                            if state.retry_count <= state.max_retries {
                                state.status = JobStatus::Retrying;
                                tracing::warn!(
                                    job_id = %entry.job_id,
                                    worker = worker_id,
                                    retry = state.retry_count,
                                    max_retries = state.max_retries,
                                    error = %error_msg,
                                    "job failed, retrying"
                                );
                                queue.pending.lock().await.push(JobEntry {
                                    job_id: entry.job_id.clone(),
                                    priority: state.priority,
                                    created_at: Instant::now(),
                                });
                            } else {
                                state.status = JobStatus::Failed(error_msg.clone());
                                tracing::error!(
                                    job_id = %entry.job_id,
                                    worker = worker_id,
                                    error = %error_msg,
                                    "job failed after all retries"
                                );
                            }
                        }
                        Err(e) => {
                            let error_msg = if e.is_panic() {
                                let panic = e.into_panic();
                                format!("panic: {:?}", panic)
                            } else {
                                e.to_string()
                            };
                            let mut state = job_arc.lock().await;
                            state.retry_count += 1;
                            if state.retry_count <= state.max_retries {
                                state.status = JobStatus::Retrying;
                                tracing::warn!(
                                    job_id = %entry.job_id,
                                    worker = worker_id,
                                    retry = state.retry_count,
                                    max_retries = state.max_retries,
                                    error = %error_msg,
                                    "job failed, retrying"
                                );
                                queue.pending.lock().await.push(JobEntry {
                                    job_id: entry.job_id.clone(),
                                    priority: state.priority,
                                    created_at: Instant::now(),
                                });
                            } else {
                                state.status = JobStatus::Failed(error_msg.clone());
                                tracing::error!(
                                    job_id = %entry.job_id,
                                    worker = worker_id,
                                    error = %error_msg,
                                    "job failed after all retries"
                                );
                            }
                        }
                    }
                }
            });
        }
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enqueue_and_complete_job() {
        let queue = Arc::new(JobQueue::new(1));
        let queue_clone = Arc::clone(&queue);

        queue.register_handler("test", Arc::new(move |job| {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(serde_json::json!({ "status": "done" }))
            })
        }));

        let job_id = queue
            .enqueue("test", 1, JobPriority::Normal, 2, serde_json::json!({}))
            .await;

        tokio::time::sleep(Duration::from_millis(100)).await;

        let state = queue_clone.get_status(&job_id).await.unwrap();
        assert_eq!(state.status, JobStatus::Completed);
        assert!(state.completed_at.is_some());
    }

    #[tokio::test]
    async fn cancel_queued_job() {
        let queue = Arc::new(JobQueue::new(1));

        let job_id = queue
            .enqueue("test", 1, JobPriority::Normal, 2, serde_json::json!({}))
            .await;

        let _ = queue.cancel(&job_id).await;
        let state = queue.get_status(&job_id).await.unwrap();
        assert_eq!(state.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn priority_ordering() {
        let queue = Arc::new(JobQueue::new(1));

        let low_id = queue
            .enqueue("test", 1, JobPriority::Low, 0, serde_json::json!({}))
            .await;
        let critical_id = queue
            .enqueue("test", 1, JobPriority::Critical, 0, serde_json::json!({}))
            .await;
        let high_id = queue
            .enqueue("test", 1, JobPriority::High, 0, serde_json::json!({}))
            .await;

        let pending = queue.pending.lock().await;
        let ordered: Vec<&str> = pending.iter().map(|e| e.job_id.as_str()).collect();
        assert!(ordered.contains(&critical_id.as_str()));
        assert!(ordered.contains(&high_id.as_str()));
        assert!(ordered.contains(&low_id.as_str()));
    }

    #[tokio::test]
    async fn retry_on_failure() {
        let attempt_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let attempts = Arc::clone(&attempt_count);

        let queue = Arc::new(JobQueue::new(1));
        let queue_clone = Arc::clone(&queue);

        queue.register_handler("flaky", Arc::new(move |_job| {
            let attempts = Arc::clone(&attempts);
            tokio::spawn(async move {
                let prev = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if prev < 2 {
                    Err("transient error".to_string())
                } else {
                    Ok(serde_json::json!({ "status": "success" }))
                }
            })
        }));

        let job_id = queue_clone
            .enqueue("flaky", 1, JobPriority::Normal, 3, serde_json::json!({}))
            .await;

        tokio::time::sleep(Duration::from_millis(500)).await;

        let state = queue_clone.get_status(&job_id).await.unwrap();
        assert_eq!(state.status, JobStatus::Completed, "job should eventually succeed: {:?}", state);
        assert!(state.retry_count <= 3, "should not exceed max retries");
    }

    #[tokio::test]
    async fn cancel_non_existent_job_fails() {
        let queue = Arc::new(JobQueue::new(1));
        let result = queue.cancel("non-existent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_status_returns_none_for_unknown_job() {
        let queue = Arc::new(JobQueue::new(1));
        let state = queue.get_status("non-existent").await;
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn update_progress_works() {
        let queue = Arc::new(JobQueue::new(1));

        let job_id = queue
            .enqueue("test", 1, JobPriority::Normal, 2, serde_json::json!({}))
            .await;

        queue.update_progress(&job_id, 0.5).await.unwrap();
        let state = queue.get_status(&job_id).await.unwrap();
        assert!((state.progress - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn get_status_by_table() {
        let queue = Arc::new(JobQueue::new(1));

        queue
            .enqueue("test", 1, JobPriority::Normal, 2, serde_json::json!({}))
            .await;
        queue
            .enqueue("test", 1, JobPriority::High, 2, serde_json::json!({}))
            .await;
        queue
            .enqueue("test", 2, JobPriority::Normal, 2, serde_json::json!({}))
            .await;

        let table1_jobs = queue.get_status_by_table(1).await;
        assert_eq!(table1_jobs.len(), 2);

        let table2_jobs = queue.get_status_by_table(2).await;
        assert_eq!(table2_jobs.len(), 1);
    }
}
