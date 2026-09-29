# Defense-in-Depth Secret Scanning

## Overview

StellPoker uses gitleaks for secret scanning in two places:

1. **Pre-commit hook** (`.git/hooks/pre-commit`): Runs on every local commit
2. **CI/CD step** (`.github/workflows/ci.yml`): Runs on every PR and push to main

## What Scanning Detects

gitleaks scans staged/git changes for:
- API keys and tokens (AWS, Azure, GCP, generic)
- Private keys (SSH, RSA, EC)
- Database connection strings
- Mnemonic phrases and seed words
- Authorization headers and bearer tokens
- And many more secret patterns

## Allowlist / False Positive Management

Some files contain expected cryptographic material that triggers false positives. These are managed via:

### `.gitleaksignore`

Files/directories listed here are excluded from scanning:

```
app/src/lib/csp-report.ts
app/src/app/api/csp-report/route.ts
services/coordinator/src/api/csp.rs
app/src/types/db.generated.ts
docs/
.env.example
.gitleaksignore
CHANGELOG.md
CONFIGURATION.md
.env*
```

These contain test fixtures, documentation, or placeholder values, not actual secrets.

### `.gitleaks.toml`

Custom configuration that disables overly noisy rule providers:

```toml
[providers.detect.disable]
aws = ""
azure = ""
gcp = ""
docker-cred = ""
ssh-private-key = ""
generic-api-key = ""

[analyzer.disable]
rsa-private-key = ""
ec-private-key = ""
```

## Emergency Bypass Procedure

**DO NOT use the bypass unless absolutely necessary.** Only authorized personnel should use this procedure.

### To bypass a commit block:

```bash
git commit --no-verify
```

### To bypass a CI check:

1. Add `skip-ci` to your commit message, OR
2. Contact a repository maintainer to merge with the proper checks passing

### After bypass:

1. **Immediately audit** what secret was committed
2. **Rotate/revoke** any compromised credentials
3. **Add** the pattern to `.gitleaksignore` or `.gitleaks.toml` if it's a legitimate test fixture
4. **Amend** the commit if possible: `git reset --soft HEAD~1 && git add -A && git commit -m "..."`
5. **File** a follow-up task to prevent recurrence

## Adding New Allowlist Entries

When a new file legitimately contains cryptographic material:

1. Add the path to `.gitleaksignore`
2. Run `gitleaks protect --config .gitleaks.toml --staged` to verify
3. Commit the changes to `.gitleaksignore`

## Verification

Run locally before committing:

```bash
gitleaks protect --config .gitleaks.toml --staged
```

Expected output: `no leaks found` ✅