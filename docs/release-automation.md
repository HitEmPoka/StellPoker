# Automated Release Workflow

## Overview

StellPoker uses conventional commits to drive automated releases. The release pipeline detects version changes, generates SBOMs, creates releases, and publishes artifacts entirely through CI/CD.

## Commit Convention

Releases are triggered by commits following the [Conventional Commits](https://www.conventionalcommits.org/) specification:

```
type(scope): description

feat:     New feature → minor version bump
fix:      Bug fix → patch version bump
feat!:    Breaking change → major version bump
```

### Examples

```bash
# Feature - triggers minor version bump
git commit -m "feat(api): add cost anomaly detection endpoint"

# Bug fix - triggers patch version bump
git commit -m "fix(coordinator): resolve session timeout race condition"

# Breaking change - triggers major version bump
git commit -m "feat(contracts)!: migrate from Soroban 20.x to 21.x"

# No version bump
git commit -m "docs(readme): update installation instructions"
```

## Workflow: `.github/workflows/release.yml`

### Stage 1: Detect Version Change

```
Commits since last tag
    ↓
Count: feat, fix, breaking changes
    ↓
Determine: major / minor / patch / none
    ↓
Generate changelog excerpt
```

**Output**: `NEW_VERSION`, `needs_release`

### Stage 2: Build and Generate SBOM

```
Build Rust binaries (release profile)
    ↓
Build Next.js frontend
    ↓
Run Syft for SBOM generation
    ↓
Generate both SPDX and CycloneDX formats
    ↓
Validate SBOM completeness
```

**Artifacts**: 
- `stellpoker` (binary)
- `app-build.tar.gz` (frontend)
- `sbom.spdx.json`
- `sbom-cyclonedx.json`

### Stage 3: Create Release

```
Generate semver from commit history
    ↓
Create Git tag (vX.Y.Z)
    ↓
Push tag to main
    ↓
Update CHANGELOG.md
    ↓
Create GitHub Release with artifacts
```

### Stage 4: Publish

```
Build Docker image
    ↓
Push to Docker Hub
    ↓
Tag as :latest
    ↓
Publish npm/crates.io packages (if applicable)
```

## Version Detection

The workflow automatically calculates the next version:

```
Last version: v1.2.3

Commits since:
  - 3x feat() → need minor bump
  - 2x fix()
  - 0x feat!()

Result: v1.3.0
```

### Detection Logic

| Scenario | Result |
|----------|--------|
| Any `feat!()` or `BREAKING CHANGE` | Major bump |
| Any `feat()` | Minor bump |
| Any `fix()` | Patch bump |
| Only `docs()`, `chore()` | No release |

## SBOM Generation

### What's Included

**SPDX Format** (`sbom.spdx.json`):
- All dependencies (direct and transitive)
- License information
- Source repository references
- Hashes (SHA-256, SHA-1)

**CycloneDX Format** (`sbom-cyclonedx.json`):
- XML/JSON component metadata
- Vulnerability tracking links
- Supply chain assurance

### Validation

```bash
# Verify SBOM
jq '.components | length' artifacts/sbom.spdx.json
# Expected: 300+ components

# Check for critical CVEs
syft packages . -o table | grep -i critical
```

## Creating a Release Manually

### Trigger Release

```bash
# Merge feature branch with conventional commits
git commit -m "feat: add new feature"
git push origin main

# Workflow automatically triggers
# Monitor at: https://github.com/HitEmPoka/StellPoker/actions
```

### Check Release Status

```bash
gh run list --workflow=release.yml --status completed
gh run view <RUN_ID> --log
```

### View Release

```bash
gh release list
gh release view v1.3.0
```

## Docker Images

### Published Locations

- **Docker Hub**: `stellpoker/coordinator:v1.3.0` and `latest`
- **GitHub Container Registry**: `ghcr.io/HitEmPoka/StellPoker:v1.3.0`

### Pull Image

```bash
docker pull stellpoker/coordinator:latest
docker run -it stellpoker/coordinator:latest --help
```

## Changelog Management

### Automatic Updates

When a release is created:
1. Workflow fetches all commits since last tag
2. Formats each as `- message (hash)`
3. Prepends to `CHANGELOG.md`
4. Commits and pushes

### Manual Edits

Before release:
```bash
# Edit CHANGELOG.md
vim CHANGELOG.md

# Commit
git add CHANGELOG.md
git commit -m "docs: update CHANGELOG for v1.3.0"
```

### Format

```markdown
## [1.3.0] - 2024-01-15

### Features
- Add cost anomaly detection endpoint (abc1234)
- Support RTL layouts in UI (def5678)

### Bug Fixes
- Fix session timeout race condition (ghi9012)

### Breaking Changes
None

[1.3.0]: https://github.com/HitEmPoka/StellPoker/releases/tag/v1.3.0
```

## GitHub Secrets Required

| Secret | Purpose |
|--------|---------|
| `GITHUB_TOKEN` | Create releases, push tags (auto-provided) |
| `DOCKER_USERNAME` | Docker Hub authentication |
| `DOCKER_PASSWORD` | Docker Hub authentication |
| `NPM_TOKEN` | npm package publishing |
| `CRATES_IO_TOKEN` | crates.io publishing |

## Troubleshooting

### Release Didn't Trigger

Check if commits follow convention:
```bash
git log --oneline -10
```

Expected output:
```
abc1234 feat: new feature
def5678 fix: bug fix
ghi9012 docs: update readme
```

If "docs:" commit is latest, no release is created (expected).

### Wrong Version Bumped

Review commit messages:
```bash
git log --format="%h %s" $(git describe --tags --abbrev=0)..HEAD
```

Count:
- `feat!:` or `BREAKING CHANGE` → Major
- `feat:` → Minor
- `fix:` → Patch

Adjust next commits accordingly.

### SBOM Generation Failed

Check workflow logs:
```bash
gh run view <RUN_ID> --log | grep -i "sbom\|syft"
```

Common issues:
1. **Syft not installed**: Workflow handles download
2. **Invalid dependencies**: Run `cargo tree` locally
3. **License detection**: Add `license` field to Cargo.toml

### Docker Push Failed

Verify credentials:
```bash
gh secret list | grep DOCKER
```

Re-encrypt if needed:
```bash
gh secret set DOCKER_PASSWORD < <(echo $DOCKER_TOKEN)
```

## Best Practices

### 1. Commit Discipline

```bash
# ✅ Good - clear, conventional
git commit -m "feat(cost): implement anomaly detection

Adds daily cost spike detection comparing against 7-day and 30-day baselines.
Triggers SNS alerts when threshold exceeded."

# ❌ Bad - ambiguous scope
git commit -m "add feature"
```

### 2. Atomic Commits

One feature = one commit (or squash before push):
```bash
git rebase -i main  # Squash multiple commits
git commit --amend  # Update if needed
```

### 3. Use Breaking Change Footer

For major version bumps:
```bash
git commit -m "refactor(api)!: redesign table configuration

This removes the legacy \`config_v1\` endpoint in favor of \`config_v2\`.

BREAKING CHANGE: \`GET /api/tables/{id}/config_v1\` removed. Use \`config_v2\` instead."
```

### 4. Pre-Release Validation

Before pushing to main:
```bash
cargo test --all
npm run test  # in app/
cargo clippy
npm run lint
```

## Related Issues

- **#667**: Release automation (this feature)
- **#668**: OpenAPI SDK generation (integrated with releases)
- **#669**: Cost anomaly detection (released via this workflow)

## See Also

- [Conventional Commits](https://www.conventionalcommits.org/)
- [Semantic Versioning](https://semver.org/)
- [SPDX Specification](https://spdx.org/)
- [CycloneDX Format](https://cyclonedx.org/)
- Workflow: `.github/workflows/release.yml`
- Changelog: `CHANGELOG.md`
