# Log Redaction and Secret Scanning Policy

## Overview

StellPoker implements defense-in-depth secret scanning to ensure no sensitive material (card data, session keys, credentials) appears in any log output. This policy is enforced through:

1. **Pre-commit scanning** (gitleaks)
2. **CI/CD validation** (GitHub Actions)
3. **Golden test suite** (Rust tests)
4. **Runtime redaction** (application level)

## What Gets Scanned

### Golden Test Scenarios

The test suite `tests/test_log_redaction.rs` verifies the following patterns **never** appear in logs:

#### 1. Poker Card Data

**Pattern**: Card rank + suit (e.g., "AS", "KH", "2D")

```rust
// ❌ BAD - card data in logs
log::info!("Player hole cards: AS KH");

// ✅ GOOD - redacted
log::info!("Player dealt cards (redacted)");
```

#### 2. Session Keys & Shared Secrets

**Pattern**: Base64-encoded cryptographic material

```rust
// ❌ BAD
log::debug!("session_key={}", base64_encode(&secret));

// ✅ GOOD
log::debug!("session established (key redacted)");
```

#### 3. Credit Card Numbers

**Pattern**: Luhn-valid 16-digit sequences

```
❌ 4532015112830366
✅ (payment processed, card number redacted)
```

#### 4. Stellar Secret Keys

**Pattern**: "S" + 55 base32 characters

```
❌ SAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
✅ Key: S••••••••••••••••••••••••••••••••••••••••••••••••••••••••
```

#### 5. AWS Credentials

**Pattern**: `AKIA` + 16 alphanumeric

```
❌ AKIAIOSFODNN7EXAMPLE
✅ (AWS credentials redacted)
```

#### 6. Passwords & API Tokens

**Pattern**: `password=` or `api_token=` followed by value

```
❌ password=supersecret
✅ password=(redacted)
```

#### 7. Database Connection Strings

**Pattern**: Protocol + credentials + host

```
❌ postgres://user:pass@host:5432/db
✅ postgres://****:****@host:5432/db
```

#### 8. Private Keys

**Pattern**: PEM-formatted private key headers

```
❌ -----BEGIN RSA PRIVATE KEY-----
✅ (private key redacted)
```

## Test Execution

### Run Locally

```bash
cargo test --test test_log_redaction --release -- --nocapture
```

### CI/CD Integration

Automatically runs on every push via `.github/workflows/log-redaction-check.yml`:

```yaml
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
```

## Test Scenarios

### Golden Test: Game Scenario

File: `tests/fixtures/test_game_scenario.json`

Simulates a 3-player poker hand and validates:
1. Hole cards never logged
2. Board cards never exposed
3. Winner determination logged without revealing cards
4. Dealer position redacted

```json
{
  "players": [
    {
      "address": "GAAAAAAA...",
      "hole_cards": ["AS", "KS"]  // Never appears in logs
    }
  ],
  "board": ["2C", "3D", "4H", "5S", "6C"],  // Board position okay, cards never logged
  "expected_winner": "GAAAAAAA..."
}
```

### Pattern Detection Tests

The test suite verifies patterns are correctly identified:

```rust
#[test]
fn test_redaction_patterns() {
    let test_cases = vec![
        ("4532015112830366", true, "CREDIT_CARD"),
        ("SAAAAAAAA...AAA", true, "STELLAR_SECRET"),
        ("password=secret", true, "PASSWORD"),
    ];
    
    for (text, should_match, _) in test_cases {
        let findings = check_for_secrets(text);
        assert_eq!(!findings.is_empty(), should_match);
    }
}
```

## Application-Level Redaction

### In Rust Code

Use the `redact` module when logging user/session data:

```rust
// From services/coordinator/src/redact.rs
use coordinator::redact::Redact;

log::info!("Player {} joined", player_address.redact());
// Output: Player G...V joined (addr redacted)

log::debug!("Session {} established", session_key.redact());
// Output: Session ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••• established
```

### Structured Logging

Avoid logging entire objects:

```rust
// ❌ BAD - may contain secrets
log::info!("Request: {:?}", request);

// ✅ GOOD - explicit fields only
log::info!(
    "Request received",
    method = "GET",
    path = "/api/tables",
    player = player_id.redact()
);
```

## CI/CD Secret Scanning

### Pre-commit Hook

Runs automatically before each local commit:

```bash
gitleaks protect --config .gitleaks.toml --staged
```

### GitHub Actions

- **gitleaks**: Scans all commits and PRs
- **Log redaction tests**: Verifies no patterns in logs
- **Workflow validation**: Checks CI configs don't hardcode secrets

### Gitleaks Configuration

File: `.gitleaks.toml` + `.gitleaksignore`

Excludes known false positives (test fixtures, documentation):

```toml
[providers.detect.disable]
aws = ""  # Too noisy
gcp = ""
docker-cred = ""
```

```
docs/
.env.example
CHANGELOG.md  # May contain placeholder credentials
```

## Incident Response

### If a Secret Is Committed

1. **Immediately revoke** the credential
2. **Verify** it didn't reach any environment:
   ```bash
   git log --grep="secret" --oneline | head
   gh secret list  # Check GitHub secrets
   ```
3. **Add pattern** to `.gitleaksignore` or `.gitleaks.toml`
4. **Amend commit** (if on feature branch):
   ```bash
   git reset --soft HEAD~1
   git add -A && git commit -m "..."
   ```
5. **File a post-incident** follow-up task

## Best Practices

### 1. Never Log Sensitive Data

```rust
// ❌ BAD
log::info!("Player cards: {:?}", hole_cards);
log::debug!("Session secret: {}", session.secret);

// ✅ GOOD
log::info!("Player dealt cards");
log::debug!("Session established (ID: {})", session.id);
```

### 2. Use Structured Logging

```rust
// ✅ GOOD - explicit what's logged
log::info!(
    event = "player_action",
    action = "fold",
    player = player_id.redact(),
);
```

### 3. Redact in Debug Output

```rust
impl Debug for SecretKey {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.debug_struct("SecretKey")
            .field("fingerprint", &"<redacted>")
            .finish()
    }
}
```

### 4. Test Log Output

```rust
#[test]
fn test_player_action_logs() {
    let logs = capture_logs(|| {
        handle_fold_action(player_id, secret_key);
    });
    
    assert!(!logs.contains(&secret_key.to_string()));
}
```

### 5. Use Environment Variables

```rust
// ✅ GOOD - secret in env, never in code
let api_key = env::var("COORDINATOR_API_KEY")
    .expect("COORDINATOR_API_KEY not set");
log::debug!("API key loaded from env");
```

## Testing Checklist

Before committing:

- [ ] Run `cargo test test_log_redaction`
- [ ] Check `cargo clippy` for secret patterns
- [ ] Run `gitleaks protect --staged`
- [ ] Review `git diff` for hardcoded secrets
- [ ] Verify environment variables used for credentials

## Related Documentation

- **Issue #13**: Log redaction policy (baseline)
- **Issue #666**: Test verification (this feature)
- Log redaction module: `services/coordinator/src/redact.rs`
- Test suite: `tests/test_log_redaction.rs`
- CI workflow: `.github/workflows/log-redaction-check.yml`
- Gitleaks config: `.gitleaks.toml`

## Emergency Contacts

For suspected secret exposure:

1. **Immediately**: Revoke the credential
2. **Within 1 hour**: Notify security team
3. **Within 24 hours**: File incident report

## See Also

- [gitleaks Documentation](https://gitleaks.io/)
- [OWASP: Secrets Management](https://owasp.org/www-community/Sensitive_Data_Exposure)
- Secret Scanning: `.github/workflows/log-redaction-check.yml`
