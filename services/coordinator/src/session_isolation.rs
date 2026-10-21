//! Multi-tenant isolation checks for concurrent coordinator sessions.
//!
//! Issue #509: concurrent tables and players share a single coordinator
//! process. Every access to a session-scoped resource must be authorized
//! against the session record the caller is bound to. Any attempt to cross a
//! session boundary — reading another player's hole cards, acting on another
//! session, or subscribing to another table's state topic — is denied and
//! recorded in a bounded, in-memory denial audit that can be flushed to the
//! tamper-evident Postgres audit log.
//!
//! The module is a pure, dependency-free state machine so it can be exercised
//! by the isolation test-suite in isolation from the rest of the (base-broken)
//! coordinator crate.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// A caller's claim about which session scope it is operating in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionClaim {
    pub table_id: u32,
    /// Player address named in the request path/body.
    pub address: String,
    /// Bound seat index, when the endpoint is seat-scoped (e.g. acting).
    pub seat_index: Option<usize>,
}

/// Session-scoped operations subject to isolation checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IsolationOperation {
    ReadHoleCards,
    SubmitAction,
    SubscribeState,
}

impl IsolationOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            IsolationOperation::ReadHoleCards => "isolation_read_hole_cards",
            IsolationOperation::SubmitAction => "isolation_submit_action",
            IsolationOperation::SubscribeState => "isolation_subscribe_state",
        }
    }
}

/// Why an isolation check denied a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationDenial {
    /// Caller is not seated at the target table.
    NotSeated,
    /// Request-claimed address differs from the authenticated caller.
    IdentityMismatch,
    /// The signed seat index does not match the caller's bound seat.
    SeatMismatch,
    /// Subscribe token is scoped to a different table.
    CrossTableSubscribe,
    /// Subscribe token does not identify a seated player.
    InvalidSessionToken,
}

impl IsolationDenial {
    pub fn as_str(&self) -> &'static str {
        match self {
            IsolationDenial::NotSeated => "not_seated",
            IsolationDenial::IdentityMismatch => "identity_mismatch",
            IsolationDenial::SeatMismatch => "seat_mismatch",
            IsolationDenial::CrossTableSubscribe => "cross_table_subscribe",
            IsolationDenial::InvalidSessionToken => "invalid_session_token",
        }
    }
}

/// One recorded denial of a cross-session attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenialRecord {
    pub seq: u64,
    pub at_unix: u64,
    pub table_id: u32,
    pub caller_address: String,
    pub operation: IsolationOperation,
    pub denial: IsolationDenial,
    pub request_id: Option<String>,
}

impl DenialRecord {
    /// Fields for the append-only `audit_logs` row: (action, endpoint,
    /// error_message, table_id, session_id).
    pub fn to_audit_fields(&self) -> (String, String, String, i32, Option<String>) {
        let endpoint = format!("/api/table/{}/{}", self.table_id, self.operation.as_str());
        let msg = format!(
            "isolation denial: {} for {} on table {} by {}",
            self.denial.as_str(),
            self.operation.as_str(),
            self.table_id,
            self.caller_address
        );
        (
            self.operation.as_str().to_string(),
            endpoint,
            msg,
            self.table_id as i32,
            self.request_id.clone(),
        )
    }
}

/// Bounded, in-memory roll of denials. Counters survive `drain()` so audit
/// flush doesn't lose the aggregate; records are trimmed to `max_records`.
#[derive(Clone, Debug)]
pub struct IsolationAudit {
    records: VecDeque<DenialRecord>,
    max_records: usize,
    total_denials: u64,
    next_seq: u64,
}

impl IsolationAudit {
    pub fn with_capacity(max_records: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(max_records),
            max_records,
            total_denials: 0,
            next_seq: 0,
        }
    }

    pub fn record(&mut self, denial: DenialRecord) {
        if denial.seq == 0 {
            let mut r = denial;
            r.seq = self.next_seq;
            self.next_seq += 1;
            self.records.push_back(r);
        } else {
            self.next_seq = self.next_seq.max(denial.seq + 1);
            self.records.push_back(denial);
        }
        self.total_denials += 1;
        while self.records.len() > self.max_records {
            self.records.pop_front();
        }
    }

    pub fn total_denials(&self) -> u64 {
        self.total_denials
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &DenialRecord> {
        self.records.iter()
    }

    /// Remove and return the records for flushing to persistent storage.
    pub fn drain(&mut self) -> Vec<DenialRecord> {
        self.records.drain(..).collect()
    }
}

impl Default for IsolationAudit {
    fn default() -> Self {
        Self::with_capacity(1024)
    }
}

/// Record a denial with the current clock, incrementing the audit seq.
pub fn record_denial(
    audit: &mut IsolationAudit,
    table_id: u32,
    caller_address: &str,
    operation: IsolationOperation,
    denial: IsolationDenial,
    request_id: Option<String>,
) {
    let at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    audit.record(DenialRecord {
        seq: 0, // assigned by audit
        at_unix,
        table_id,
        caller_address: caller_address.to_string(),
        operation,
        denial,
        request_id,
    });
}

/// Authorize a hole-card read (Issue #509).
///
/// The signed caller (`authenticated_address`) must be the player named in the
/// request (`claim.address`) and must be seated at `session_players`. A claim
/// naming somebody else's seat is a cross-session (identity) leak attempt.
/// Returns the bound seat index.
pub fn authorize_cards_read(
    claim: &SessionClaim,
    session_players: &[String],
    authenticated_address: &str,
    insecure_dev_auth: bool,
) -> Result<usize, IsolationDenial> {
    if !insecure_dev_auth && claim.address != authenticated_address {
        return Err(IsolationDenial::IdentityMismatch);
    }
    let idx = session_players
        .iter()
        .position(|p| p == &claim.address)
        .ok_or(IsolationDenial::NotSeated)?;
    if let Some(seat) = claim.seat_index {
        if seat != idx {
            return Err(IsolationDenial::SeatMismatch);
        }
    }
    Ok(idx)
}

/// Authorize a seat-scoped action (e.g. betting, cancel, showdown trigger).
///
/// The caller must claim a seat bound to its own authenticated address.
pub fn authorize_action(
    claim: &SessionClaim,
    session_players: &[String],
    authenticated_address: &str,
    insecure_dev_auth: bool,
) -> Result<usize, IsolationDenial> {
    if !insecure_dev_auth && claim.address != authenticated_address {
        return Err(IsolationDenial::IdentityMismatch);
    }
    let idx = session_players
        .iter()
        .position(|p| p == &claim.address)
        .ok_or(IsolationDenial::NotSeated)?;
    match claim.seat_index {
        Some(seat) if seat == idx => Ok(idx),
        Some(_) => Err(IsolationDenial::SeatMismatch),
        None => Ok(idx),
    }
}

/// Authorize a WebSocket state subscription (Issue #509).
///
/// A subscription must carry a session token (table_id it was issued for) and
/// be made by a player seated at that table. Subscribing to a different table
/// with an otherwise-valid token is a cross-session attempt.
pub fn authorize_subscribe(
    subscription_table: u32,
    token_table: Option<u32>,
    token_player: Option<&str>,
    seated_addrs: &[String],
) -> Result<(), IsolationDenial> {
    let token_table = token_table.ok_or(IsolationDenial::InvalidSessionToken)?;
    if token_table != subscription_table {
        return Err(IsolationDenial::CrossTableSubscribe);
    }
    let token_player = token_player.ok_or(IsolationDenial::InvalidSessionToken)?;
    if !seated_addrs.iter().any(|p| p == token_player) {
        return Err(IsolationDenial::NotSeated);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players() -> Vec<String> {
        vec!["alice".to_string(), "bob".to_string(), "carol".to_string()]
    }

    fn record_id(n: u64) -> Option<String> {
        Some(format!("req-{}", n))
    }

    #[test]
    fn cards_read_rejects_cross_session_identity() {
        // Bob is signed in as alice's seat on table 1 but asks for bob's cards
        // (a different session's identity) → denied + audited.
        let claim = SessionClaim {
            table_id: 1,
            address: "bob".to_string(),
            seat_index: None,
        };
        let mut audit = IsolationAudit::default();
        match authorize_cards_read(&claim, &players(), "alice", false) {
            Err(IsolationDenial::IdentityMismatch) => {}
            other => panic!("expected identity mismatch, got {:?}", other),
        }
        record_denial(
            &mut audit,
            1,
            "alice",
            IsolationOperation::ReadHoleCards,
            IsolationDenial::IdentityMismatch,
            record_id(1),
        );
        assert_eq!(audit.total_denials(), 1);
        let rec = audit.iter().next().unwrap();
        assert_eq!(rec.operation, IsolationOperation::ReadHoleCards);
        assert_eq!(rec.denial, IsolationDenial::IdentityMismatch);
        assert_eq!(rec.caller_address, "alice");
    }

    #[test]
    fn cards_read_allows_own_seat() {
        let claim = SessionClaim {
            table_id: 1,
            address: "bob".to_string(),
            seat_index: None,
        };
        assert_eq!(
            authorize_cards_read(&claim, &players(), "bob", false),
            Ok(1)
        );
    }

    #[test]
    fn cards_read_rejects_unseated_caller() {
        let claim = SessionClaim {
            table_id: 1,
            address: "mallory".to_string(),
            seat_index: None,
        };
        // Mallory signs her own request but is not seated on this table.
        assert_eq!(
            authorize_cards_read(&claim, &players(), "mallory", false),
            Err(IsolationDenial::NotSeated)
        );
    }

    #[test]
    fn action_rejects_wrong_seat_binding() {
        // Carol both signed-and-claimed her own address but claimed seat 0
        // which is bound to alice.
        let claim = SessionClaim {
            table_id: 2,
            address: "carol".to_string(),
            seat_index: Some(0),
        };
        assert_eq!(
            authorize_action(&claim, &players(), "carol", false),
            Err(IsolationDenial::SeatMismatch)
        );
    }

    #[test]
    fn subscribe_rejects_cross_table_token() {
        // Token issued for table 1, player tries to subscribe to table 2.
        let mut audit = IsolationAudit::default();
        let res = authorize_subscribe(2, Some(1), Some("alice"), &players());
        assert_eq!(res, Err(IsolationDenial::CrossTableSubscribe));
        record_denial(
            &mut audit,
            2,
            "alice",
            IsolationOperation::SubscribeState,
            IsolationDenial::CrossTableSubscribe,
            record_id(3),
        );
        assert_eq!(audit.total_denials(), 1);
    }

    #[test]
    fn subscribe_rejects_missing_token() {
        assert_eq!(
            authorize_subscribe(1, None, None, &players()),
            Err(IsolationDenial::InvalidSessionToken)
        );
        let claim = SessionClaim {
            table_id: 1,
            address: "alice".to_string(),
            seat_index: Some(0),
        };
        // Hollow token (player not found) also denied.
        assert_eq!(
            authorize_subscribe(1, Some(1), Some("nobody"), &players()),
            Err(IsolationDenial::NotSeated)
        );
        assert_eq!(
            authorize_cards_read(&claim, &players(), "alice", false),
            Ok(0)
        );
    }

    #[test]
    fn subscribe_allows_seated_same_table() {
        let seated = vec!["dave".to_string()];
        assert_eq!(
            authorize_subscribe(9, Some(9), Some("dave"), &seated),
            Ok(())
        );
    }

    #[test]
    fn cross_session_matrix_denies_and_audits_every_boundary() {
        // Simulate two concurrent sessions sharing the coordinator (Issue #509):
        // table 1 seats [alice, bob], table 2 seats [carol, dave].
        let t1 = vec!["alice".to_string(), "bob".to_string()];
        let t2 = vec!["carol".to_string(), "dave".to_string()];
        let mut audit = IsolationAudit::default();

        // 1) Cross-session READ: carol (table 2) asks table 1 for bob's cards.
        let cross_read = SessionClaim {
            table_id: 1,
            address: "bob".to_string(),
            seat_index: None,
        };
        assert_eq!(
            authorize_cards_read(&cross_read, &t1, "carol", false),
            Err(IsolationDenial::IdentityMismatch)
        );
        record_denial(
            &mut audit,
            1,
            "carol",
            IsolationOperation::ReadHoleCards,
            IsolationDenial::IdentityMismatch,
            record_id(10),
        );

        // 2) Cross-session WRITE: dave claims seat 0 on table 1 (bound to alice).
        let cross_write = SessionClaim {
            table_id: 1,
            address: "dave".to_string(),
            seat_index: Some(0),
        };
        assert_eq!(
            authorize_action(&cross_write, &t1, "dave", false),
            Err(IsolationDenial::NotSeated)
        );
        record_denial(
            &mut audit,
            1,
            "dave",
            IsolationOperation::SubmitAction,
            IsolationDenial::NotSeated,
            record_id(11),
        );

        // 3) Cross-session SUBSCRIBE: carol reuses her table-2 token on table 1.
        assert_eq!(
            authorize_subscribe(1, Some(2), Some("carol"), &t1),
            Err(IsolationDenial::CrossTableSubscribe)
        );
        record_denial(
            &mut audit,
            1,
            "carol",
            IsolationOperation::SubscribeState,
            IsolationDenial::CrossTableSubscribe,
            record_id(12),
        );

        // Legitimate in-session traffic stays allowed and un-audited.
        assert_eq!(
            authorize_cards_read(
                &SessionClaim {
                    table_id: 2,
                    address: "carol".to_string(),
                    seat_index: None,
                },
                &t2,
                "carol",
                false
            ),
            Ok(0)
        );
        assert_eq!(authorize_subscribe(2, Some(2), Some("dave"), &t2), Ok(()));

        // Exactly three boundary crossings were denied AND audited.
        assert_eq!(audit.total_denials(), 3);
        assert_eq!(audit.len(), 3);
        let kinds: Vec<IsolationOperation> = audit.iter().map(|r| r.operation).collect();
        assert!(kinds.contains(&IsolationOperation::ReadHoleCards));
        assert!(kinds.contains(&IsolationOperation::SubmitAction));
        assert!(kinds.contains(&IsolationOperation::SubscribeState));

        // Every record drains into audit-log fields without panicking.
        let drain = audit.drain();
        assert_eq!(drain.len(), 3);
        for rec in drain {
            let (action, endpoint, msg, tid, _sid) = rec.to_audit_fields();
            assert!(action.starts_with("isolation_"));
            assert!(endpoint.contains("/api/table/"));
            assert!(msg.contains("isolation denial"));
            assert_eq!(tid, 1);
        }
        assert!(audit.is_empty());
        assert_eq!(audit.total_denials(), 3);
    }
}
