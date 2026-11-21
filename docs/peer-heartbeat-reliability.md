# Peer heartbeat reliability

The peer-heartbeat timeout test exercises a connection that can be torn down while the timeout watcher is still active. The test setup should therefore treat cleanup as part of the assertion boundary:

- wait for every peer task to stop before closing its transport;
- cancel and await heartbeat watchers during teardown;
- use an explicit deadline supplied by the test instead of relying on scheduler timing;
- assert the expected timeout exception before cleanup begins.

This keeps connection resets from racing the timeout assertion and gives follow-up fixes a single reliability contract to preserve.