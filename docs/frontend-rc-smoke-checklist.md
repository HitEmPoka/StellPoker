# Frontend Release Candidate (RC) Smoke Checklist

The RC channel gives frontend deploys a promote-after-smoke path so a bad
build never ships straight to production. On every push to `main`, CI
publishes `ghcr.io/<owner>/stellpoker/frontend:rc` from the **same build
digest** as the git-SHA image (see `.github/workflows/docker-build-push.yml`).
Deploy that tag to the RC URL, run the checklist below, then promote.

## 1. Deploy RC

```bash
# Pull and run the RC image (same digest as the main-branch SHA build)
docker pull ghcr.io/<owner>/stellpoker/frontend:rc
docker run -d --name stellpoker-frontend-rc -p 3100:3000 \
  -e NEXT_PUBLIC_COORDINATOR_URL=<rc-coordinator-url> \
  ghcr.io/<owner>/stellpoker/frontend:rc
```

Or point the staging compose override at the RC tag:

```bash
STAGING_FRONTEND_IMAGE=ghcr.io/<owner>/stellpoker/frontend:rc \
  ./scripts/deploy-staging.sh
```

Record the RC URL used (e.g. `https://rc.stellpoker.example`) — all checks
below run against it.

## 2. Smoke checklist

Run every item against the RC URL before promoting. All must pass.

- [ ] **App responds** — `curl -s -o /dev/null -w "%{http_code}" $RC_URL` returns `200`
- [ ] **Static assets load** — no 404s in browser console for JS/CSS chunks
- [ ] **Build matches main** — image digest of `:rc` equals digest of `:<sha>`:
      `docker buildx imagetools inspect ghcr.io/<owner>/stellpoker/frontend:rc`
- [ ] **Coordinator connectivity** — wallet connect + table list calls reach
      the RC coordinator without CORS/network errors
- [ ] **Wallet flow** — connect a test wallet, view open tables, open a table
      detail page
- [ ] **Real Freighter smoke** — run the optional extension tier from
      [docs/wallet-integration-testing.md](wallet-integration-testing.md#real-extension-smoke-runbook)
      with a pre-approved profile, or confirm the latest nightly
      `Wallet Real Extension Smoke` run is green
- [ ] **Game page loads** — navigate to a table page; board/seats render
- [ ] **No console errors** — zero uncaught exceptions on load and navigation
- [ ] **Environment vars** — `NEXT_PUBLIC_*` values point at RC services, not prod

## 3. Promote

Only after every checklist item passes:

```bash
OWNER=<github-owner> ./scripts/promote-rc.sh
```

This re-tags the RC digest as `:latest` without rebuilding. Verify digests
match, then roll the production deploy to the new `:latest` digest.

## 4. Rollback

Promotion is a tag move, so rollback is re-pointing `:latest` at the previous
digest (or re-running the previous release). No rebuild required.
