//! Gossip-based peer discovery for MPC nodes (Issue #230)
//!
//! Nodes currently learn about each other only from the coordinator. That makes
//! the coordinator a discovery single point of failure: if it is unreachable,
//! surviving nodes cannot find a replacement peer even when one is running and
//! healthy. This module lets nodes exchange peer tables directly, so the
//! membership view survives a coordinator outage.
//!
//! ## What this is and is not
//!
//! This is **discovery**, not membership consensus. The MPC committee itself is
//! fixed at three parties by REP3, and which three parties hold the key shares
//! is decided by the committee registry and a re-key ceremony — never by
//! gossip. A node learning about a peer through gossip does not admit that peer
//! to the committee; it only means the address is known and can be health
//! checked. Treating gossip as authoritative for membership would let any node
//! that can talk to one peer inject a party into the protocol.
//!
//! ## Design
//!
//! Anti-entropy with a per-peer version counter, which is the standard SWIM /
//! Cassandra approach:
//!
//! - Every node keeps a table of `PeerRecord`s keyed by node id.
//! - Each record carries a `version` that only its owner increments. A node is
//!   authoritative about itself and nothing else.
//! - Periodically a node picks a random peer and exchanges digests. Each side
//!   sends back the records where its version is newer.
//! - Merging keeps the higher version. Because only the owner increments its
//!   own version, this converges without a coordinator and without clock
//!   synchronisation between nodes.
//!
//! Version numbers rather than timestamps on purpose: wall clocks between nodes
//! disagree, and a node whose clock is ahead would otherwise win every merge
//! permanently, pinning a stale record in place across the whole cluster.
//!
//! ## Failure detection
//!
//! Suspicion is expressed as state, not as immediate removal. A peer that
//! misses a probe becomes `Suspect`; only after `suspicion_timeout` does it
//! become `Faulty`. A single dropped packet must not evict a healthy node from
//! every other node's table — with three parties, evicting one wrongly is the
//! difference between a working committee and a halted one.
//!
//! Faulty records are retained rather than deleted so the failure gossips. If
//! the record simply vanished, the next anti-entropy round with a node that had
//! not noticed would silently resurrect it.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How a peer is currently regarded by this node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerState {
    /// Responding to probes.
    Alive,
    /// Missed at least one probe; not yet declared faulty.
    Suspect,
    /// Missed probes past the suspicion timeout.
    Faulty,
    /// Left deliberately. Distinct from `Faulty` so a planned drain is not
    /// reported as an incident.
    Left,
}

/// One node's view of another node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecord {
    pub node_id: u32,
    /// Address other nodes should dial for MPC traffic.
    pub address: String,
    pub state: PeerState,
    /// Incremented only by the node this record describes.
    pub version: u64,
    /// Local observation time, in seconds. Never compared across nodes — it
    /// exists only so this node can time out its own suspicions.
    pub last_seen_secs: u64,
}

impl PeerRecord {
    pub fn new(node_id: u32, address: impl Into<String>, now_secs: u64) -> Self {
        Self {
            node_id,
            address: address.into(),
            state: PeerState::Alive,
            version: 1,
            last_seen_secs: now_secs,
        }
    }

    /// True when `other` describes a strictly newer view of the same peer.
    ///
    /// Only the version is compared. A record's own `last_seen_secs` is a local
    /// observation and is meaningless on another node's clock.
    pub fn supersedes(&self, other: &PeerRecord) -> bool {
        self.node_id == other.node_id && self.version > other.version
    }
}

/// A digest exchanged during anti-entropy: node id and the version held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerDigest {
    pub node_id: u32,
    pub version: u64,
}

/// Tunables for the gossip loop.
#[derive(Debug, Clone, Copy)]
pub struct GossipConfig {
    /// How often to run an anti-entropy round.
    pub interval: Duration,
    /// How long a peer may stay `Suspect` before being declared `Faulty`.
    pub suspicion_timeout: Duration,
    /// Peers contacted per round. Low on purpose: with a handful of nodes,
    /// contacting everyone every round is just a broadcast, and the point of
    /// gossip is that load stays bounded as the cluster grows.
    pub fanout: usize,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5),
            // Six intervals. Long enough that a GC pause or a brief network
            // blip does not evict a healthy party, short enough that a genuinely
            // dead node is noticed before a session needs it.
            suspicion_timeout: Duration::from_secs(30),
            fanout: 2,
        }
    }
}

/// Result of merging a remote record into the local table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The peer was previously unknown.
    Added,
    /// A newer version replaced the local record.
    Updated,
    /// The local record was already at least as new.
    Ignored,
    /// Rejected: the record claims to describe this node.
    RejectedSelf,
}

/// This node's view of the cluster.
#[derive(Debug, Clone)]
pub struct PeerTable {
    self_id: u32,
    peers: HashMap<u32, PeerRecord>,
    config: GossipConfig,
}

impl PeerTable {
    pub fn new(self_id: u32, config: GossipConfig) -> Self {
        Self {
            self_id,
            peers: HashMap::new(),
            config,
        }
    }

    pub fn self_id(&self) -> u32 {
        self.self_id
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn get(&self, node_id: u32) -> Option<&PeerRecord> {
        self.peers.get(&node_id)
    }

    /// Seed a peer learned from the coordinator or from static configuration.
    ///
    /// Bootstrapping still needs a starting point — gossip spreads knowledge,
    /// it cannot create the first contact.
    pub fn seed(
        &mut self,
        node_id: u32,
        address: impl Into<String>,
        now_secs: u64,
    ) -> MergeOutcome {
        if node_id == self.self_id {
            return MergeOutcome::RejectedSelf;
        }
        match self.peers.get(&node_id) {
            Some(_) => MergeOutcome::Ignored,
            None => {
                self.peers
                    .insert(node_id, PeerRecord::new(node_id, address, now_secs));
                MergeOutcome::Added
            }
        }
    }

    /// Merge a record received from a peer.
    ///
    /// A node is authoritative about itself, so a record describing *this* node
    /// is rejected outright. Accepting it would let any peer mark this node
    /// faulty, or redirect its advertised address, simply by claiming a higher
    /// version.
    pub fn merge(&mut self, incoming: PeerRecord) -> MergeOutcome {
        if incoming.node_id == self.self_id {
            return MergeOutcome::RejectedSelf;
        }

        match self.peers.get(&incoming.node_id) {
            None => {
                self.peers.insert(incoming.node_id, incoming);
                MergeOutcome::Added
            }
            Some(existing) if incoming.supersedes(existing) => {
                self.peers.insert(incoming.node_id, incoming);
                MergeOutcome::Updated
            }
            Some(_) => MergeOutcome::Ignored,
        }
    }

    /// Merge a batch, reporting how many records actually changed anything.
    pub fn merge_all(&mut self, incoming: impl IntoIterator<Item = PeerRecord>) -> usize {
        incoming
            .into_iter()
            .filter(|_| true)
            .map(|record| self.merge(record))
            .filter(|outcome| matches!(outcome, MergeOutcome::Added | MergeOutcome::Updated))
            .count()
    }

    /// Digest of everything this node knows, for an anti-entropy exchange.
    pub fn digest(&self) -> Vec<PeerDigest> {
        let mut digests: Vec<PeerDigest> = self
            .peers
            .values()
            .map(|p| PeerDigest {
                node_id: p.node_id,
                version: p.version,
            })
            .collect();
        // Deterministic order so an exchange is reproducible in tests and in
        // logs; the protocol itself does not depend on it.
        digests.sort_by_key(|d| d.node_id);
        digests
    }

    /// Records this node holds that are newer than the peer's digest.
    ///
    /// This is the reply half of anti-entropy: the peer says what it has, and
    /// this returns only what it is missing or has an older version of.
    pub fn records_newer_than(&self, remote: &[PeerDigest]) -> Vec<PeerRecord> {
        let remote_versions: HashMap<u32, u64> =
            remote.iter().map(|d| (d.node_id, d.version)).collect();

        let mut newer: Vec<PeerRecord> = self
            .peers
            .values()
            .filter(|p| match remote_versions.get(&p.node_id) {
                None => true,
                Some(&their_version) => p.version > their_version,
            })
            .cloned()
            .collect();

        newer.sort_by_key(|p| p.node_id);
        newer
    }

    /// Node ids this node knows nothing about, or holds an older version of.
    pub fn missing_from_local(&self, remote: &[PeerDigest]) -> Vec<u32> {
        let mut wanted: Vec<u32> = remote
            .iter()
            .filter(|d| d.node_id != self.self_id)
            .filter(|d| match self.peers.get(&d.node_id) {
                None => true,
                Some(local) => d.version > local.version,
            })
            .map(|d| d.node_id)
            .collect();
        wanted.sort_unstable();
        wanted
    }

    /// Record a successful probe: the peer is alive and its version advances.
    ///
    /// The version bump is what lets the recovery propagate. Without it, other
    /// nodes holding a `Suspect` record at the same version would ignore the
    /// update and keep the peer suspect indefinitely.
    pub fn mark_alive(&mut self, node_id: u32, now_secs: u64) {
        if let Some(peer) = self.peers.get_mut(&node_id) {
            let recovering = peer.state != PeerState::Alive;
            peer.state = PeerState::Alive;
            peer.last_seen_secs = now_secs;
            if recovering {
                peer.version += 1;
            }
        }
    }

    /// Record a failed probe.
    ///
    /// Moves `Alive` to `Suspect` but does not touch a peer that has already
    /// left — a drained node reappearing as suspect would read as an incident.
    pub fn mark_suspect(&mut self, node_id: u32, now_secs: u64) {
        if let Some(peer) = self.peers.get_mut(&node_id) {
            if peer.state == PeerState::Alive {
                peer.state = PeerState::Suspect;
                peer.last_seen_secs = now_secs;
                peer.version += 1;
            }
        }
    }

    /// Mark a peer as having left deliberately.
    pub fn mark_left(&mut self, node_id: u32, now_secs: u64) {
        if let Some(peer) = self.peers.get_mut(&node_id) {
            peer.state = PeerState::Left;
            peer.last_seen_secs = now_secs;
            peer.version += 1;
        }
    }

    /// Promote suspects that have been silent past the timeout to `Faulty`.
    ///
    /// Returns the ids promoted, so the caller can log or alert. Records are
    /// kept rather than removed: a deleted record would be re-added by the next
    /// exchange with a node that had not yet noticed the failure.
    pub fn expire_suspects(&mut self, now_secs: u64) -> Vec<u32> {
        let timeout = self.config.suspicion_timeout.as_secs();
        let mut promoted: Vec<u32> = Vec::new();

        for peer in self.peers.values_mut() {
            if peer.state == PeerState::Suspect
                && now_secs.saturating_sub(peer.last_seen_secs) >= timeout
            {
                peer.state = PeerState::Faulty;
                peer.version += 1;
                promoted.push(peer.node_id);
            }
        }

        promoted.sort_unstable();
        promoted
    }

    /// Peers currently considered reachable.
    pub fn alive_peers(&self) -> Vec<&PeerRecord> {
        let mut alive: Vec<&PeerRecord> = self
            .peers
            .values()
            .filter(|p| p.state == PeerState::Alive)
            .collect();
        alive.sort_by_key(|p| p.node_id);
        alive
    }

    /// Choose gossip targets for one round.
    ///
    /// `seed` is supplied by the caller so selection is deterministic in tests
    /// while still being spread across peers in production. Only `Alive` and
    /// `Suspect` peers are contacted — probing a peer already known to be
    /// faulty spends the round's budget on the least likely responder, and a
    /// recovery is picked up by that peer gossiping to others anyway.
    pub fn select_gossip_targets(&self, seed: u64) -> Vec<u32> {
        let mut candidates: Vec<u32> = self
            .peers
            .values()
            .filter(|p| matches!(p.state, PeerState::Alive | PeerState::Suspect))
            .map(|p| p.node_id)
            .collect();
        candidates.sort_unstable();

        if candidates.is_empty() {
            return Vec::new();
        }

        let fanout = self.config.fanout.min(candidates.len());
        let start = (seed as usize) % candidates.len();

        // Rotate from a seed-derived offset so successive rounds cover
        // different peers rather than hammering the numerically lowest ids.
        (0..fanout)
            .map(|i| candidates[(start + i) % candidates.len()])
            .collect()
    }
}

/// Seconds since the Unix epoch, for `last_seen_secs`.
///
/// Only ever compared against other readings from the same node, so a clock
/// that disagrees with its peers does not affect convergence.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(self_id: u32) -> PeerTable {
        PeerTable::new(self_id, GossipConfig::default())
    }

    fn record(node_id: u32, version: u64, state: PeerState) -> PeerRecord {
        PeerRecord {
            node_id,
            address: format!("node-{node_id}:10000"),
            state,
            version,
            last_seen_secs: 1_000,
        }
    }

    // ── Authority over one's own record ─────────────────────────────────────

    #[test]
    fn a_record_describing_this_node_is_rejected() {
        // Otherwise any peer could mark this node faulty, or redirect its
        // advertised address, just by claiming a higher version.
        let mut t = table(1);
        assert_eq!(
            t.merge(record(1, 99, PeerState::Faulty)),
            MergeOutcome::RejectedSelf
        );
        assert!(t.get(1).is_none());
    }

    #[test]
    fn seeding_this_node_is_rejected() {
        let mut t = table(1);
        assert_eq!(t.seed(1, "self:10000", 0), MergeOutcome::RejectedSelf);
        assert!(t.is_empty());
    }

    // ── Merge semantics ─────────────────────────────────────────────────────

    #[test]
    fn an_unknown_peer_is_added() {
        let mut t = table(1);
        assert_eq!(t.merge(record(2, 1, PeerState::Alive)), MergeOutcome::Added);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_newer_version_replaces_the_local_record() {
        let mut t = table(1);
        t.merge(record(2, 1, PeerState::Alive));

        assert_eq!(
            t.merge(record(2, 2, PeerState::Suspect)),
            MergeOutcome::Updated
        );
        assert_eq!(t.get(2).unwrap().state, PeerState::Suspect);
    }

    #[test]
    fn an_older_version_is_ignored() {
        // The case that makes convergence work: a node still gossiping a stale
        // view must not undo a newer one.
        let mut t = table(1);
        t.merge(record(2, 5, PeerState::Alive));

        assert_eq!(
            t.merge(record(2, 3, PeerState::Faulty)),
            MergeOutcome::Ignored
        );
        assert_eq!(t.get(2).unwrap().state, PeerState::Alive);
    }

    #[test]
    fn an_equal_version_is_ignored() {
        // Ties keep what is already held; treating equal as newer would make
        // merges order-dependent and stop them converging.
        let mut t = table(1);
        t.merge(record(2, 4, PeerState::Alive));

        assert_eq!(
            t.merge(record(2, 4, PeerState::Faulty)),
            MergeOutcome::Ignored
        );
        assert_eq!(t.get(2).unwrap().state, PeerState::Alive);
    }

    #[test]
    fn merge_all_counts_only_records_that_changed_something() {
        let mut t = table(1);
        t.merge(record(2, 5, PeerState::Alive));

        let changed = t.merge_all(vec![
            record(2, 1, PeerState::Faulty),  // older, ignored
            record(3, 1, PeerState::Alive),   // new
            record(2, 9, PeerState::Suspect), // newer, updates
        ]);

        assert_eq!(changed, 2);
    }

    #[test]
    fn merging_is_order_independent() {
        // Anti-entropy delivers records in whatever order the network provides;
        // both nodes must still land on the same view.
        let newest = record(2, 7, PeerState::Faulty);
        let middle = record(2, 4, PeerState::Suspect);
        let oldest = record(2, 1, PeerState::Alive);

        let mut forward = table(1);
        forward.merge_all(vec![oldest.clone(), middle.clone(), newest.clone()]);

        let mut backward = table(1);
        backward.merge_all(vec![newest.clone(), middle, oldest]);

        assert_eq!(forward.get(2), backward.get(2));
        assert_eq!(forward.get(2).unwrap().version, 7);
    }

    // ── Anti-entropy exchange ───────────────────────────────────────────────

    #[test]
    fn the_digest_lists_every_known_peer_and_version() {
        let mut t = table(1);
        t.merge(record(3, 2, PeerState::Alive));
        t.merge(record(2, 5, PeerState::Alive));

        let digest = t.digest();

        assert_eq!(digest.len(), 2);
        // Sorted, so an exchange is reproducible.
        assert_eq!(digest[0].node_id, 2);
        assert_eq!(digest[0].version, 5);
        assert_eq!(digest[1].node_id, 3);
    }

    #[test]
    fn only_records_newer_than_the_remote_digest_are_sent() {
        let mut t = table(1);
        t.merge(record(2, 5, PeerState::Alive));
        t.merge(record(3, 2, PeerState::Alive));
        t.merge(record(4, 1, PeerState::Alive));

        let remote = vec![
            PeerDigest {
                node_id: 2,
                version: 5,
            }, // same — not sent
            PeerDigest {
                node_id: 3,
                version: 1,
            }, // older — sent
               // node 4 unknown to the remote — sent
        ];

        let sent = t.records_newer_than(&remote);

        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].node_id, 3);
        assert_eq!(sent[1].node_id, 4);
    }

    #[test]
    fn a_node_asks_for_what_it_is_missing_or_holds_stale() {
        let mut t = table(1);
        t.merge(record(2, 5, PeerState::Alive));

        let remote = vec![
            PeerDigest {
                node_id: 2,
                version: 9,
            }, // remote is newer — want it
            PeerDigest {
                node_id: 3,
                version: 1,
            }, // unknown locally — want it
            PeerDigest {
                node_id: 1,
                version: 4,
            }, // describes self — never want it
        ];

        assert_eq!(t.missing_from_local(&remote), vec![2, 3]);
    }

    #[test]
    fn two_nodes_converge_after_one_exchange() {
        // The property the whole module exists for: no coordinator involved.
        let mut a = table(1);
        let mut b = table(2);

        a.merge(record(3, 4, PeerState::Alive));
        b.merge(record(4, 2, PeerState::Alive));

        let from_a = a.records_newer_than(&b.digest());
        let from_b = b.records_newer_than(&a.digest());
        a.merge_all(from_b);
        b.merge_all(from_a);

        assert_eq!(a.get(3), b.get(3));
        assert_eq!(a.get(4), b.get(4));
    }

    // ── Failure detection ───────────────────────────────────────────────────

    #[test]
    fn a_missed_probe_makes_a_peer_suspect_not_faulty() {
        // One dropped packet must not evict a party from a three-party
        // committee.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);

        t.mark_suspect(2, 105);

        assert_eq!(t.get(2).unwrap().state, PeerState::Suspect);
    }

    #[test]
    fn suspicion_bumps_the_version_so_it_propagates() {
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        let before = t.get(2).unwrap().version;

        t.mark_suspect(2, 105);

        assert!(t.get(2).unwrap().version > before);
    }

    #[test]
    fn a_suspect_is_promoted_to_faulty_only_after_the_timeout() {
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.mark_suspect(2, 100);

        // One second short of the 30s default.
        assert!(t.expire_suspects(129).is_empty());
        assert_eq!(t.get(2).unwrap().state, PeerState::Suspect);

        assert_eq!(t.expire_suspects(130), vec![2]);
        assert_eq!(t.get(2).unwrap().state, PeerState::Faulty);
    }

    #[test]
    fn a_faulty_record_is_kept_rather_than_deleted() {
        // Deleting it would let the next exchange with an unaware node
        // resurrect the peer as alive.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.mark_suspect(2, 100);
        t.expire_suspects(200);

        assert_eq!(t.len(), 1);
        assert_eq!(t.get(2).unwrap().state, PeerState::Faulty);
    }

    #[test]
    fn recovery_bumps_the_version_so_other_nodes_stop_suspecting() {
        // Without the bump, peers holding a Suspect record at the same version
        // would ignore the recovery and keep the node suspect forever.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.mark_suspect(2, 105);
        let suspect_version = t.get(2).unwrap().version;

        t.mark_alive(2, 110);

        assert_eq!(t.get(2).unwrap().state, PeerState::Alive);
        assert!(t.get(2).unwrap().version > suspect_version);
    }

    #[test]
    fn repeated_probes_of_a_healthy_peer_do_not_inflate_the_version() {
        // A version that climbs on every successful probe would make every
        // round produce gossip traffic even when nothing changed.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        let version = t.get(2).unwrap().version;

        t.mark_alive(2, 105);
        t.mark_alive(2, 110);

        assert_eq!(t.get(2).unwrap().version, version);
    }

    #[test]
    fn a_departed_peer_is_not_reported_as_suspect() {
        // A planned drain is not an incident.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.mark_left(2, 105);

        t.mark_suspect(2, 110);

        assert_eq!(t.get(2).unwrap().state, PeerState::Left);
    }

    #[test]
    fn alive_peers_excludes_suspect_and_faulty() {
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.seed(3, "node-3:10000", 100);
        t.seed(4, "node-4:10000", 100);
        t.mark_suspect(3, 100);
        t.mark_suspect(4, 100);
        t.expire_suspects(200);

        let alive: Vec<u32> = t.alive_peers().iter().map(|p| p.node_id).collect();
        assert_eq!(alive, vec![2]);
    }

    // ── Target selection ────────────────────────────────────────────────────

    #[test]
    fn gossip_targets_respect_the_fanout() {
        let mut t = table(1);
        for id in 2..=8 {
            t.seed(id, format!("node-{id}:10000"), 100);
        }

        assert_eq!(
            t.select_gossip_targets(0).len(),
            GossipConfig::default().fanout
        );
    }

    #[test]
    fn successive_rounds_contact_different_peers() {
        // A fixed selection would hammer the lowest ids and leave the rest
        // undiscovered.
        let mut t = table(1);
        for id in 2..=8 {
            t.seed(id, format!("node-{id}:10000"), 100);
        }

        assert_ne!(t.select_gossip_targets(0), t.select_gossip_targets(3));
    }

    #[test]
    fn faulty_peers_are_not_gossip_targets() {
        // Spending a bounded round budget on the least likely responder is
        // wasted; a recovered node is picked up via its own gossip.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.seed(3, "node-3:10000", 100);
        t.mark_suspect(3, 100);
        t.expire_suspects(200);

        assert_eq!(t.select_gossip_targets(0), vec![2]);
    }

    #[test]
    fn suspect_peers_are_still_contacted() {
        // They are the ones most worth probing — that is how recovery is
        // detected at all.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.mark_suspect(2, 100);

        assert_eq!(t.select_gossip_targets(0), vec![2]);
    }

    #[test]
    fn an_empty_table_selects_no_targets() {
        assert!(table(1).select_gossip_targets(0).is_empty());
    }

    #[test]
    fn fanout_larger_than_the_cluster_returns_every_peer_once() {
        let mut t = PeerTable::new(
            1,
            GossipConfig {
                fanout: 10,
                ..GossipConfig::default()
            },
        );
        t.seed(2, "node-2:10000", 100);
        t.seed(3, "node-3:10000", 100);

        let targets = t.select_gossip_targets(0);
        assert_eq!(targets.len(), 2);
        // No duplicates — a wrapping rotation must not contact one peer twice
        // while skipping another.
        assert_ne!(targets[0], targets[1]);
    }

    #[test]
    fn seeding_an_existing_peer_does_not_overwrite_gossiped_state() {
        // A coordinator re-seed must not resurrect a peer gossip has since
        // marked faulty.
        let mut t = table(1);
        t.seed(2, "node-2:10000", 100);
        t.mark_suspect(2, 100);
        t.expire_suspects(200);

        assert_eq!(t.seed(2, "node-2:10000", 300), MergeOutcome::Ignored);
        assert_eq!(t.get(2).unwrap().state, PeerState::Faulty);
    }
}
