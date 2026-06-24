# Google Cloud Platform (GCP) GKE Kubernetes Configuration

# GCP VPC Network
resource "google_compute_network" "main" {
  name                    = "${var.project_name}-vpc"
  auto_create_subnetworks = false
}

# GCP Subnet
resource "google_compute_subnetwork" "main" {
  name          = "${var.project_name}-subnet"
  ip_cidr_range = "10.0.0.0/20"
  region        = var.gcp_region
  network       = google_compute_network.main.id

  secondary_ip_range {
    range_name    = "pods"
    ip_cidr_range = "10.4.0.0/14"
  }

  secondary_ip_range {
    range_name    = "services"
    ip_cidr_range = "10.8.0.0/20"
  }
}

# GKE Cluster
resource "google_container_cluster" "main" {
  name     = "${var.project_name}-gke-cluster"
  location = var.gcp_region
  project  = var.gcp_project_id

  # We can't create a cluster with no node pool defined, but we want to only use
  # separately managed node pools. So we create the smallest possible default
  # node pool and immediately delete it.
  remove_default_node_pool = true
  initial_node_count       = 1

  network    = google_compute_network.main.name
  subnetwork = google_compute_subnetwork.main.name

  ip_allocation_policy {
    cluster_secondary_range_name  = "pods"
    services_secondary_range_name = "services"
  }

  addons_config {
    http_load_balancing {
      disabled = false
    }
    horizontal_pod_autoscaling {
      disabled = false
    }
  }

  workload_identity_config {
    workload_pool = "${var.gcp_project_id}.iam.goog.com"
  }

  logging_service    = "logging.googleapis.com/kubernetes"
  monitoring_service = "monitoring.googleapis.com/kubernetes"

  resource_labels = {
    environment = var.environment
    project     = var.project_name
  }
}

# GKE Node Pool
resource "google_container_node_pool" "main" {
  name       = "${var.project_name}-node-pool"
  location   = var.gcp_region
  cluster    = google_container_cluster.main.name
  project    = var.gcp_project_id
  node_count = var.gke_node_count

  autoscaling {
    min_node_count = var.gke_node_count
    max_node_count = var.gke_node_count * 3
  }

  management {
    auto_repair  = true
    auto_upgrade = true
  }

  node_config {
    preemptible  = var.environment != "prod"
    machine_type = var.gke_machine_type

    disk_size_gb = 100
    disk_type    = "pd-standard"

    oauth_scopes = [
      "https://www.googleapis.com/auth/cloud-platform"
    ]

    labels = {
      environment = var.environment
      pool        = "main"
    }

    metadata = {
      disable-legacy-endpoints = "true"
    }

    shielded_instance_config {
      enable_secure_boot          = true
      enable_integrity_monitoring = true
    }

    workload_metadata_config {
      mode = "GKE_METADATA"
    }
  }
}

# Service Account for GKE Workloads
resource "google_service_account" "gke" {
  account_id   = "${var.project_name}-gke-sa"
  display_name = "Service account for GKE workloads"
  project      = var.gcp_project_id
}

# Workload Identity Binding
resource "google_service_account_iam_member" "gke_workload_identity" {
  service_account_id = google_service_account.gke.name
  role               = "roles/iam.workloadIdentityUser"
  member             = "serviceAccount:${var.gcp_project_id}.svc.id.goog[default/stellpoker]"
}

# Cloud SQL Instance (PostgreSQL)
resource "google_sql_database_instance" "main" {
  name             = "${var.project_name}-cloudsql"
  database_version = "POSTGRES_15"
  region           = var.gcp_region
  project          = var.gcp_project_id

  settings {
    tier      = "db-custom-2-7680"
    disk_type = "PD_SSD"
    disk_size = var.db_allocated_storage

    database_flags {
      name  = "cloudsql_iam_authentication"
      value = "on"
    }

    database_flags {
      name  = "log_statement"
      value = var.environment == "prod" ? "ddl" : "all"
    }

    database_flags {
      name  = "log_duration"
      value = var.environment == "prod" ? "off" : "on"
    }

    ip_configuration {
      ipv4_enabled    = false
      private_network = google_compute_network.main.id
      require_ssl     = true
    }

    backup_configuration {
      enabled                        = true
      start_time                     = "03:00"
      point_in_time_recovery_enabled = true
      transaction_log_retention_days = 7
    }

    insights_config {
      query_insights_enabled  = var.enable_monitoring
      query_string_length     = 1024
      query_plans_per_minute  = 5
      record_application_tags = false
    }

    user_labels = {
      environment = var.environment
      project     = var.project_name
    }
  }

  deletion_protection = var.environment == "prod"

  depends_on = [google_service_networking_connection.private_vpc_connection]
}

# Private VPC Connection for Cloud SQL
resource "google_compute_global_address" "private_ip_address" {
  name          = "${var.project_name}-private-ip"
  purpose       = "VPC_PEERING"
  address_type  = "INTERNAL"
  prefix_length = 16
  network       = google_compute_network.main.id
  project       = var.gcp_project_id
}

resource "google_service_networking_connection" "private_vpc_connection" {
  network                 = google_compute_network.main.id
  service                 = "servicenetworking.googleapis.com"
  reserved_peering_ranges = [google_compute_global_address.private_ip_address.name]
}

# Cloud SQL Database
resource "google_sql_database" "coordinator" {
  name     = "coordinator"
  instance = google_sql_database_instance.main.name
  project  = var.gcp_project_id
}

# Cloud SQL User
resource "google_sql_user" "coordinator" {
  name     = var.db_username
  instance = google_sql_database_instance.main.name
  password = var.db_password
  project  = var.gcp_project_id
}

# Cloud Load Balancer
resource "google_compute_backend_service" "main" {
  name    = "${var.project_name}-backend-service"
  project = var.gcp_project_id

  protocol = "HTTP"

  health_checks = [google_compute_health_check.main.id]

  log_config {
    enable      = var.enable_monitoring
    sample_rate = 1.0
  }
}

resource "google_compute_health_check" "main" {
  name    = "${var.project_name}-health-check"
  project = var.gcp_project_id

  http_health_check {
    port               = 8080
    request_path       = var.alb_health_check_path
    check_interval_sec = var.alb_health_check_interval
    timeout_sec        = var.alb_health_check_timeout
  }
}

# Cloud CDN
resource "google_compute_backend_service" "cdn" {
  name    = "${var.project_name}-cdn-backend"
  project = var.gcp_project_id

  protocol = "HTTP"

  health_checks = [google_compute_health_check.main.id]

  cdn_policy {
    cache_mode                   = "CACHE_ALL_STATIC"
    default_ttl                  = var.cdn_default_ttl
    max_ttl                      = var.cdn_max_ttl
    negative_caching             = true
    negative_caching_policy {
      code = 404
      ttl  = 120
    }
    negative_caching_policy {
      code = 410
      ttl  = 120
    }
  }

  log_config {
    enable      = var.enable_monitoring
    sample_rate = 1.0
  }
}

# Logging
resource "google_logging_project_sink" "gke_logs" {
  count           = var.enable_monitoring ? 1 : 0
  name            = "${var.project_name}-gke-logs-sink"
  destination     = "logging.googleapis.com/projects/${var.gcp_project_id}/logs/${var.project_name}-gke"
  filter          = "resource.type=\"k8s_cluster\"\nresource.labels.cluster_name=\"${google_container_cluster.main.name}\""
  project         = var.gcp_project_id
  unique_writer_identity = true
}

# Outputs
output "gke_cluster_name" {
  description = "GKE cluster name"
  value       = google_container_cluster.main.name
}

output "gke_cluster_endpoint" {
  description = "GKE cluster endpoint"
  value       = google_container_cluster.main.endpoint
}

output "cloudsql_instance_name" {
  description = "Cloud SQL instance name"
  value       = google_sql_database_instance.main.name
}

output "cloudsql_private_ip" {
  description = "Cloud SQL private IP address"
  value       = google_sql_database_instance.main.private_ip_address
}
