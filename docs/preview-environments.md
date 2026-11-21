# Preview Environments (per PR)

Every pull request can get its own isolated docker-compose stack so
reviewers can click through a change instead of reconstructing it locally
(#683). The stack is fully ephemeral: it lives in its own compose project,
on its own network, with ports derived from the PR number.

## Requirements

- Docker Compose **v2.24+** (the preview override uses `!override` port
  syntax — run `docker compose version` to check).
- A checkout of the PR branch (`gh pr checkout <n>`).

## Starting a preview

```bash
make pr-preview PR=123
```

This runs `scripts/pr-preview.sh up 123`, which is equivalent to:

```bash
docker compose \
  -p stellpoker-pr-123 \
  -f docker-compose.yml -f docker-compose.preview.yml \
  up -d --build
```

The preview project name (`stellpoker-pr-<n>`) isolates containers,
networks, and volumes from your normal `make start` dev stack and from other
PRs' previews — you can run several at once.

### Preview URLs

| Endpoint | URL |
|---|---|
| Coordinator API (health) | `http://localhost:$((18080 + PR % 100))/api/health` |
| Soroban RPC | `http://localhost:$((19000 + PR % 100))/soroban/rpc` |

Print them any time with:

```bash
make -s pr-preview PR=123 >/dev/null   # see startup output
scripts/pr-preview.sh url 123          # just the URL
```

Internal services (Soroban, the three active MPC nodes) do not publish host
ports in preview mode — they only talk to each other over the compose
network, which is what keeps concurrent PRs from colliding. The frontend
(`app/`) is not part of the compose stack; run it against a preview with:

```bash
cd app && NEXT_PUBLIC_COORDINATOR_URL=http://localhost:18179 npm run dev
```

(first boot compiles circuits and downloads CRS data — allow a few minutes
before `/api/health` turns healthy; watch
`docker compose -p stellpoker-pr-123 logs -f coordinator`).

## Teardown on merge or close

**A preview must be torn down when its PR is merged or closed** — previews
are manual (no CI runner holds them), so the PR author or merger runs:

```bash
make pr-preview-down PR=123
```

which runs `docker compose -p stellpoker-pr-123 down -v --remove-orphans`
(containers, network, and volumes are removed). Checklist:

- [ ] Merge: run `make pr-preview-down PR=<n>` immediately after merging.
- [ ] Close without merge: same command before/when closing.
- [ ] Stale leftovers (e.g. reviewer's laptop): list projects with
      `docker compose ls` and remove any `stellpoker-pr-*` entry.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `port is already allocated` | Another PR with the same `PR % 100` is running — tear it down, or change that PR's preview ports via `PREVIEW_PORT`/`PREVIEW_SOROBAN_PORT` env vars |
| `!override` parse error | Docker Compose < v2.24 — upgrade |
| Coordinator unhealthy for a long time | `docker compose -p stellpoker-pr-<n> logs coordinator mpc-node-0` — first boot compiles circuits; CRS errors → `scripts/download-crs.sh` |
| Preview vs dev stack interference | They are separate projects; if ports clash, stop one with `make stop` / `make pr-preview-down` |
