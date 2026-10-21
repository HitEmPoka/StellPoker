# Table UI performance budget

Animations on the table (card deal and flip, chip movement, pot updates) must hold **60 fps** on a mid range profile. That is a budget of **16.7 ms per frame**.

## Budget

| Metric | Target | Fail |
| --- | --- | --- |
| Median frame time during the deal animation | ≤ 16.7 ms | > 16.7 ms |
| 95th percentile frame time | ≤ 25 ms | > 33 ms |
| Dropped frames (over 33 ms) per deal | 0 | more than 2 |
| Long tasks over 50 ms during the deal | 0 | any |

## Mid range profile

Chrome DevTools on a desktop, emulating a mid range phone:

- Performance panel, CPU: **4x slowdown**
- Network: **Fast 4G**
- Viewport: 412 × 915 (device toolbar, mid tier Android)
- `prefers-reduced-motion` off, reduced data mode off, so the full flip animation runs

## Measurement method

1. Build and serve the production app (`npm run build && npm run start` in `app/`). Dev mode is slower and is not representative.
2. Open a practice table so hands can be dealt without other players.
3. Apply the profile above.
4. Paste the snippet below into the console, then deal a hand within the next 5 seconds.
5. Repeat for 5 deals and record the worst result in the PR.

```js
(() => {
  const frames = [];
  let last = performance.now();
  const end = last + 5000;
  function tick(now) {
    frames.push(now - last);
    last = now;
    if (now < end) requestAnimationFrame(tick);
    else {
      const sorted = [...frames].sort((a, b) => a - b);
      const pct = (p) => sorted[Math.floor((sorted.length - 1) * p)].toFixed(1);
      console.table({
        frames: frames.length,
        medianMs: pct(0.5),
        p95Ms: pct(0.95),
        dropped: frames.filter((f) => f > 33.4).length,
      });
    }
  }
  requestAnimationFrame(tick);
})();
```

For detail on a regression, record the same deal in the Performance panel and check the frames track for red frames and the main thread for long tasks.

## When to measure

Run this before merging any change to card, chip or pot animations, or to `globals.css` keyframes, and paste the before and after table in the PR.
