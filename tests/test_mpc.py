"""
Tests for MPC peer communication and heartbeat mechanisms.
"""

import unittest
import time
import threading
from unittest.mock import Mock, patch, MagicMock


class MockPeer:
    """Mock peer for testing heartbeat timeout scenarios."""

    def __init__(self, peer_id, heartbeat_interval=0.1, clock=None):
        self.peer_id = peer_id
        self.heartbeat_interval = heartbeat_interval
        self._clock = clock or time.monotonic
        self.last_heartbeat = self._clock()
        self.is_alive = True
        self._lock = threading.Lock()

    def send_heartbeat(self):
        """Send a heartbeat to keep the peer alive."""
        with self._lock:
            self.last_heartbeat = self._clock()
            self.is_alive = True

    def check_heartbeat(self, timeout=0.5):
        """Check if peer is still alive based on heartbeat timeout."""
        with self._lock:
            if not self.is_alive:
                return False
            elapsed = self._clock() - self.last_heartbeat
            if elapsed > timeout:
                self.is_alive = False
                return False
            return True

    def reset_heartbeat(self):
        """Reset the heartbeat timer."""
        with self._lock:
            self.last_heartbeat = self._clock()


class TestPeerHeartbeat(unittest.TestCase):
    """Test suite for peer heartbeat and timeout mechanisms."""

    def setUp(self):
        """Initialize test fixtures."""
        self.peers = {}
        self.heartbeat_threads = {}

    def tearDown(self):
        """Clean up test resources."""
        for peer in self.peers.values():
            with peer._lock:
                peer.is_alive = False
        for thread in self.heartbeat_threads.values():
            if thread.is_alive():
                thread.join(timeout=1.0)

    def _start_heartbeat_emulator(self, peer_id, interval=0.1):
        """Start a background thread that sends periodic heartbeats."""
        def heartbeat_loop():
            while self.peers[peer_id].is_alive:
                self.peers[peer_id].send_heartbeat()
                time.sleep(interval)

        thread = threading.Thread(target=heartbeat_loop, daemon=True)
        thread.start()
        self.heartbeat_threads[peer_id] = thread

    def test_peer_heartbeat_active(self):
        """Test that an active peer maintains heartbeat."""
        peer = MockPeer(peer_id=1, heartbeat_interval=0.1)
        self.peers[1] = peer

        # Start heartbeat emulator
        self._start_heartbeat_emulator(1, interval=0.05)

        # Check heartbeat immediately
        self.assertTrue(peer.check_heartbeat(timeout=0.5))

        # Sleep and check again - should still be alive
        time.sleep(0.2)
        self.assertTrue(peer.check_heartbeat(timeout=0.5))

    def test_peer_heartbeat_timeout(self):
        """
        Test that peer is marked as dead when heartbeat times out.

        This test has historically been flaky due to timing issues.
        The fix ensures proper synchronization and state management.
        """
        now = [100.0]
        peer = MockPeer(peer_id=2, heartbeat_interval=0.1, clock=lambda: now[0])
        self.peers[2] = peer

        # Initialize heartbeat
        peer.reset_heartbeat()

        # Verify peer is initially alive
        self.assertTrue(peer.check_heartbeat(timeout=0.3))

        # Simulate no heartbeat by advancing beyond the timeout
        now[0] += 0.5

        # Peer should now be dead due to timeout
        is_alive = peer.check_heartbeat(timeout=0.3)
        self.assertFalse(is_alive, "Peer should be marked as dead after heartbeat timeout")

    def test_peer_heartbeat_recovery(self):
        """Test that a peer can recover from timeout with new heartbeat."""
        peer = MockPeer(peer_id=3, heartbeat_interval=0.1)
        self.peers[3] = peer

        # Mark peer as dead initially
        peer.is_alive = False
        peer.reset_heartbeat()

        # Peer should be dead
        self.assertFalse(peer.check_heartbeat(timeout=0.1))

        # Send a heartbeat to recover
        peer.is_alive = True
        peer.send_heartbeat()

        # Peer should now be alive
        self.assertTrue(peer.check_heartbeat(timeout=0.5))

    def test_peer_heartbeat_timeout_multiple_peers(self):
        """Test heartbeat timeout across multiple peers with proper isolation."""
        # Create multiple peers
        for i in range(1, 4):
            peer = MockPeer(peer_id=i, heartbeat_interval=0.1)
            self.peers[i] = peer
            peer.reset_heartbeat()

        # Start heartbeat for peers 1 and 2 only
        self._start_heartbeat_emulator(1, interval=0.05)
        self._start_heartbeat_emulator(2, interval=0.05)

        # Peer 3 does not receive heartbeat
        self.peers[3].is_alive = True

        # Give heartbeats time to establish
        time.sleep(0.2)

        # Peers 1 and 2 should be alive
        self.assertTrue(self.peers[1].check_heartbeat(timeout=0.3))
        self.assertTrue(self.peers[2].check_heartbeat(timeout=0.3))

        # Peer 3 should timeout due to no heartbeat activity
        time.sleep(0.5)
        self.assertFalse(self.peers[3].check_heartbeat(timeout=0.3))

    def test_peer_heartbeat_concurrent_access(self):
        """Test thread-safe concurrent access to peer heartbeat state."""
        peer = MockPeer(peer_id=4, heartbeat_interval=0.1)
        self.peers[4] = peer

        results = []

        def check_multiple_times():
            for _ in range(10):
                result = peer.check_heartbeat(timeout=0.5)
                results.append(result)
                time.sleep(0.01)

        # Send initial heartbeat
        peer.send_heartbeat()

        # Start multiple threads checking heartbeat concurrently
        threads = [threading.Thread(target=check_multiple_times) for _ in range(3)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        # All checks should pass since heartbeat is recent
        self.assertTrue(all(results), "All concurrent heartbeat checks should pass")


if __name__ == "__main__":
    unittest.main()
