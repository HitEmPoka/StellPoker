# Dependency update automation

Dependency bumps are opened automatically by Dependabot
(`.github/dependabot.yml`, Issue #670). Updates are **grouped** so the weekly
review load is a few PRs rather than one per package.

## What gets updated

| Ecosystem | Directory | Schedule | Grouping |
|-----------|-----------|----------|----------|
| Cargo (whole workspace) | `/` | Weekly, Mon 06:00 UTC | `cargo-minor-patch`, `cargo-security` |
| npm (frontend) | `/app` | Weekly, Mon 06:00 UTC | `react-next`, `npm-prod-minor-patch`, `npm-dev-minor-patch`, `npm-security` |
| npm (DB scripts) | `/scripts/db` | Monthly | `db-scripts` |
| GitHub Actions | `/` | Weekly, Mon 06:00 UTC | `github-actions` |
| Docker base images | `/app`, `/services/coordinator`, `/services/node` | Weekly, Mon 06:00 UTC | `docker-base-images` |

Major version bumps are **not** grouped (except React/Next.js, which must move
together): each gets its own PR, since it usually needs code changes.

Security updates are grouped separately from routine bumps and are opened as
soon as an advisory is published, independent of the weekly schedule. They
complement the `cargo deny` advisory check in `dependency-check.yml`.

## What is excluded on purpose

- **`soroban-sdk`** is pinned to a git revision in `[workspace.dependencies]`.
  Bump it by hand and re-run the Soroban budget and storage-layout workflows.
- **Arkworks (`ark-*`)** minor/major versions must match the co-noir and Noir
  backend versions; they are only patch-bumped automatically.
- **`vendor/`** is third-party code we track verbatim and is not scanned.
- **Noir / co-noir** versions are pinned in `flake.nix`, the Dockerfiles and
  `.devcontainer/`; bump them together by hand.

## Reviewing a grouped PR

1. Check CI is green — the full test, fuzz-smoke and contract budget workflows
   run on dependency PRs like any other.
2. Skim the changelog links Dependabot includes for anything touching
   cryptography, serialization or TLS (`rustls`, `ed25519-dalek`, `sha2`, `serde`).
3. If one package in a group breaks the build, comment
   `@dependabot ignore <package> minor version` (or `patch`) on the PR; Dependabot
   recreates the group without it. Open an issue to track the manual bump.
4. Squash-merge so each group is a single revertible commit.

## Changing the policy

Edit `.github/dependabot.yml`. Dependabot validates the file on push to the
default branch and reports errors in **Insights → Dependency graph → Dependabot**.
