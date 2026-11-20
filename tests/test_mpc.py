"""
Tests for MPC peer communication and heartbeat mechanisms.

Timing note: these tests used to read the wall clock (``time.time()``) and
rely on ``time.sleep()`` margins to decide whether a peer had timed out.
On loaded CI runners that produced rerun-only failures
(``ConnectionResetError: Peer 2 heartbeat timed out``) because a scheduler
stall, or a wall-clock adjustment, could push the measured elapsed time past
or below the timeout. Peers now take an injectable clock: the timeout logic
is tested against a ``FakeClock`` that only moves when the test advances it,
and the real default is the monotonic clock, which never jumps backwards.
"""

import threading
import time
import unittest


class FakeClock:
    """A manually advanced, thread-safe clock for deterministic timing tests."""

    def __init__(self, start=1000.0):
        self._now = float(start)
        self._lock = threading.Lock()

    def __call__(self):
        with self._lock:
            return self._now

    def advance(self, seconds):
        with self._lock:
            self._now += float(seconds)


class MockPeer:
    """Mock peer for testing heartbeat timeout scenarios."""

    def __init__(self, peer_id, heartbeat_interval=0.1, clock=time.monotonic):
        self.peer_id = peer_id
        self.heartbeat_interval = heartbeat_interval
        self._clock = clock
        self._lock = threading.Lock()
        self.last_heartbeat = self._clock()
        self.is_alive = True

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
            self.is_alive = True


class TestPeerHeartbeat(unittest.TestCase):
    """Test suite for peer heartbeat and timeout mechanisms."""

    def setUp(self):
        """Initialize test fixtures."""
        self.peers = {}
        self.heartbeat_threads = {}
        self._stop = threading.Event()

    def tearDown(self):
        """Stop and join every heartbeat thread so none leak into later tests."""
        self._stop.set()
        for thread in self.heartbeat_threads.values():
            thread.join(timeout=2.0)
            self.assertFalse(thread.is_alive(), "heartbeat thread did not stop")

    def _start_heartbeat_emulator(self, peer_id, interval=0.05):
        """Start a background thread that sends periodic heartbeats until stopped."""
        peer = self.peers[peer_id]

        def heartbeat_loop():
            while not self._stop.is_set():
                peer.send_heartbeat()
                self._stop.wait(interval)

        thread = threading.Thread(target=heartbeat_loop, daemon=True)
        thread.start()
        self.heartbeat_threads[peer_id] = thread
        return thread

    def test_peer_heartbeat_active(self):
        """An active peer sending real heartbeats stays alive."""
        peer = MockPeer(peer_id=1)
        self.peers[1] = peer

        self._start_heartbeat_emulator(1, interval=0.05)

        # Generous timeout: heartbeats arrive every 50ms, so only a stall of
        # several seconds could fail this check.
        self.assertTrue(peer.check_heartbeat(timeout=5.0))
        time.sleep(0.2)
        self.assertTrue(peer.check_heartbeat(timeout=5.0))

    def test_peer_heartbeat_timeout(self):
        """A peer is marked dead once its heartbeat is older than the timeout."""
        clock = FakeClock()
        peer = MockPeer(peer_id=2, clock=clock)
        self.peers[2] = peer

        peer.reset_heartbeat()
        self.assertTrue(peer.check_heartbeat(timeout=0.3))

        # Exactly at the timeout boundary the peer is still considered alive
        # because check_heartbeat uses strict greater-than (>), not >=.
        clock.advance(0.3)
        self.assertTrue(peer.check_heartbeat(timeout=0.3))
        self.assertTrue(peer.is_alive)

        # One step past the timeout with no heartbeat: the peer is dead.
        clock.advance(0.2)
        self.assertFalse(
            peer.check_heartbeat(timeout=0.3),
            "Peer should be marked as dead after heartbeat timeout",
        )
        self.assertFalse(peer.is_alive)

    def test_peer_heartbeat_timeout_is_deterministic(self):
        """Repeating the timeout scenario 200 times always gives the same result."""
        for _ in range(200):
            clock = FakeClock()
            peer = MockPeer(peer_id=2, clock=clock)
            self.assertTrue(peer.check_heartbeat(timeout=0.3))
            clock.advance(0.5)
            self.assertFalse(peer.check_heartbeat(timeout=0.3))

    def test_peer_heartbeat_recovery(self):
        """A peer can recover from a timeout with a new heartbeat."""
        clock = FakeClock()
        peer = MockPeer(peer_id=3, clock=clock)
        self.peers[3] = peer

        peer.reset_heartbeat()
        clock.advance(0.2)
        self.assertFalse(peer.check_heartbeat(timeout=0.1))
        self.assertFalse(peer.is_alive)

        # send_heartbeat resets both last_heartbeat and is_alive under the lock.
        peer.send_heartbeat()
        self.assertTrue(peer.check_heartbeat(timeout=0.5))
        self.assertTrue(peer.is_alive)

    def test_peer_heartbeat_timeout_multiple_peers(self):
        """Only the peer that stops sending heartbeats times out."""
        clock = FakeClock()
        for i in range(1, 4):
            self.peers[i] = MockPeer(peer_id=i, clock=clock)
            self.peers[i].reset_heartbeat()

        # Simulate 10 heartbeat rounds of 0.1s; peers 1 and 2 respond, peer 3 is silent.
        for _ in range(10):
            clock.advance(0.1)
            self.peers[1].send_heartbeat()
            self.peers[2].send_heartbeat()

        self.assertTrue(self.peers[1].check_heartbeat(timeout=0.3))
        self.assertTrue(self.peers[2].check_heartbeat(timeout=0.3))
        self.assertFalse(self.peers[3].check_heartbeat(timeout=0.3))

    def test_peer_heartbeat_concurrent_access(self):
        """Concurrent heartbeat checks and sends are thread-safe."""
        clock = FakeClock()
        peer = MockPeer(peer_id=4, clock=clock)
        self.peers[4] = peer
        peer.send_heartbeat()

        results = []
        results_lock = threading.Lock()

        def check_multiple_times():
            for _ in range(50):
                result = peer.check_heartbeat(timeout=0.5)
                with results_lock:
                    results.append(result)

        def send_multiple_times():
            for _ in range(50):
                peer.send_heartbeat()

        threads = [threading.Thread(target=check_multiple_times) for _ in range(3)]
        threads.append(threading.Thread(target=send_multiple_times))
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        self.assertEqual(len(results), 150)
        self.assertTrue(all(results), "All concurrent heartbeat checks should pass")

    def test_peer_heartbeat_timeout_50_runs_no_flake(self):
        """Verify peer 2 heartbeat timeout behaves deterministically over 50 runs (#797, #799, #802, #803)."""
        for i in range(50):
            clock = FakeClock(start=200.0 + i)
            peer = MockPeer(peer_id=2, clock=clock)
            peer.reset_heartbeat()
            self.assertTrue(peer.check_heartbeat(timeout=0.3))
            clock.advance(0.5)
            self.assertFalse(peer.check_heartbeat(timeout=0.3))
            self.assertFalse(peer.is_alive)


if __name__ == "__main__":
    unittest.main()
