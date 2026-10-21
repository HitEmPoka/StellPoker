# Wallet Integration Testing

This document describes the wallet integration testing framework implemented for issue #297, and the optional real-extension smoke added for issue #651.

## Two-tier testing

Wallet coverage is split so pull requests stay fast and deterministic while
release checks still exercise the real browser extension:

| Tier | Runs | Wallet | Command |
|---|---|---|---|
| Always-on mocks | Every PR / local test run | `FreighterMock` test double | `cd app && npm test -- wallet` |
| Real-extension smoke | Nightly and manual release checks | Official Freighter extension in Chromium | See [Real-extension smoke runbook](#real-extension-smoke-runbook) |

The mock tier is the source of truth for connection, signature, transaction,
account-switching, and network behaviour. The real-extension tier verifies the
parts a mock cannot: that the official extension injects its API into the page
and that the frontend detects it.

## Overview

Comprehensive test suite covering wallet connection flows, signature requests, transaction approval/rejection, account switching, and network detection using a sophisticated Freighter mock.

## Test Structure

### Core Test Files

1. **`wallet-integration.test.ts`** - Main wallet integration tests
2. **`transaction-simulation-integration.test.ts`** - Simulation + wallet flow tests
3. **`mocks/freighter-mock.ts`** - Sophisticated Freighter API mock
4. **`test-utils.ts`** - Shared testing utilities

## FreighterMock Features

### Complete API Coverage
```typescript
const freighterMock = new FreighterMock({
  isInstalled: true,
  address: "GABCDEFG...",
  connected: true,
  signResponse: "signature_string",
  networkPassphrase: "Test SDF Network ; September 2015"
});
```

### Dynamic State Management
```typescript
// Simulate account switching
freighterMock.simulateAccountSwitch("GNEWADDRESS...");

// Simulate user rejection
freighterMock.simulateUserRejection();

// Simulate disconnection
freighterMock.simulateDisconnection();

// Reset to initial state
freighterMock.reset();
```

### Installation/Uninstallation
```typescript
// Install on all possible window properties
freighterMock.install();

// Clean removal
freighterMock.uninstall();
```

## Test Coverage

### ✅ Wallet Connection Flow
- Successful connection to Freighter
- Connection failure when not installed
- Access request rejection handling
- Disconnected wallet state handling

### ✅ Signature Requests  
- String response format
- Object response format (`{ signature: "..." }`)
- User rejection handling
- Invalid response handling
- Malformed response handling

### ✅ Transaction Approval/Rejection
- Successful transaction signing
- User rejection of transactions
- Transaction XDR handling
- Network parameter validation

### ✅ Account Switching
- Detection of account changes
- Handling disconnection during switch
- Address update propagation
- Session invalidation

### ✅ Network Detection
- Testnet network detection
- Mainnet network detection
- Network details retrieval
- Network passphrase validation

### ✅ Silent Reconnection
- Successful reconnect with stored credentials
- Graceful handling when no stored data
- Reconnection failure handling
- localStorage interaction

### ✅ Error Handling
- Wallet API errors
- Malformed responses
- Network errors
- Timeout scenarios

### ✅ Integration Testing
- End-to-end wallet + simulation flow
- Prevention of execution on failed simulation
- Full transaction lifecycle testing

## Usage Examples

### Basic Test Setup
```typescript
import { FreighterMock } from "./mocks/freighter-mock";

describe("Wallet Tests", () => {
  let freighterMock: FreighterMock;

  beforeEach(() => {
    freighterMock = new FreighterMock();
    freighterMock.install();
  });

  afterEach(() => {
    freighterMock.uninstall();
  });

  it("connects to wallet", async () => {
    const { connectFreighterWallet } = await import("../lib/freighter");
    const session = await connectFreighterWallet();
    
    expect(session.walletType).toBe("freighter");
    expect(session.address).toBeDefined();
  });
});
```

### Testing Error Scenarios
```typescript
it("handles user rejection", async () => {
  const { connectFreighterWallet } = await import("../lib/freighter");
  const session = await connectFreighterWallet();
  
  freighterMock.simulateUserRejection();
  
  await expect(session.signMessage("test"))
    .rejects.toThrow("User rejected request");
});
```

### Testing Account Switching
```typescript
it("detects account changes", async () => {
  const { connectFreighterWallet } = await import("../lib/freighter");
  let session = await connectFreighterWallet();
  
  expect(session.address).toBe("GABCDEFG...");
  
  freighterMock.simulateAccountSwitch("GNEWADDR...");
  
  session = await connectFreighterWallet();
  expect(session.address).toBe("GNEWADDR...");
});
```

## Mock Configurations

### Successful Flow
```typescript
new FreighterMock({
  isInstalled: true,
  connected: true,
  address: "GABCDEFG...",
  signResponse: "signature_string"
})
```

### Error Scenarios
```typescript
// Not installed
new FreighterMock({ isInstalled: false })

// Access denied
new FreighterMock({ accessError: "User denied access" })

// Signature rejection
new FreighterMock({ signError: "User rejected request" })

// Not connected
new FreighterMock({ connected: false })
```

### Different Networks
```typescript
// Testnet
new FreighterMock({
  networkPassphrase: "Test SDF Network ; September 2015"
})

// Mainnet  
new FreighterMock({
  networkPassphrase: "Public Global Stellar Network ; September 2015"
})
```

## Running Tests

```bash
# Run all wallet tests
npm test wallet

# Run with coverage
npm run test:coverage

# Run integration tests specifically
npm test wallet-integration

# Run simulation integration tests
npm test transaction-simulation-integration
```

## Mock Verification

The mock provides access to spy functions for verification:

```typescript
const api = freighterMock.getMockApi();

expect(api.getAddress).toHaveBeenCalledTimes(1);
expect(api.signMessage).toHaveBeenCalledWith("test message", undefined);
expect(api.requestAccess).toHaveBeenCalled();
```

## Testing Best Practices

1. **Isolation**: Each test gets fresh mock instances
2. **Cleanup**: Always uninstall mocks in `afterEach`  
3. **State Management**: Use mock methods to change state during tests
4. **Error Testing**: Test both success and failure paths
5. **Integration**: Test wallet integration with other modules

## Real-extension smoke runbook

The real-extension suite lives in
`app/e2e/tests/wallet-real-extension.spec.ts`. It is skipped by default so the
normal Playwright suite never requires a browser extension or a funded wallet.
Set `REAL_FREIGHTER_SMOKE=1` to opt in.

### What it verifies

1. Chromium starts with the official Freighter extension loaded.
2. The extension injects one of its supported window APIs.
3. The app detects Freighter and shows **CONNECT FREIGHTER**.
4. Optionally (`REAL_FREIGHTER_CONNECT=1`), a pre-approved unlocked profile can
   complete the connection and show the connected address.

### Prerequisites

- Google Chrome or Chromium supported by the installed Playwright version.
- The official Freighter extension (ID
  `bcacfldlkkdogcmkkibnjlakofdplcbk`). The helper script downloads the current
  Chrome Web Store build; pin or mirror it if releases must be reproducible.
- For the optional connect step: a testnet wallet that is unlocked, funded with
  testnet XLM, and has already approved `http://localhost:3000`.

### 1. Download and unpack Freighter

```bash
python3 scripts/fetch-freighter-extension.py --out /tmp/freighter-extension --force
```

The script strips the CRX header and writes an unpacked extension with a
`manifest.json`. To use a locally unpacked build instead, point
`FREIGHTER_EXTENSION_PATH` at that directory.

### 2. Start the frontend

```bash
cd app
npm run dev
```

### 3. Run the smoke

In another terminal, from the repository root:

```bash
cd app
REAL_FREIGHTER_SMOKE=1 \
FREIGHTER_EXTENSION_PATH=/tmp/freighter-extension \
npx playwright test \
  --config=e2e/playwright.config.ts \
  --project=chromium \
  tests/wallet-real-extension.spec.ts \
  --workers=1
```

On a headless Linux machine, wrap the command in `xvfb-run -a`. Chromium's
default headless shell does not load extensions; the smoke launches a headed
persistent context by default.

### 4. Full connection check (manual/nightly with a prepared profile)

Create a persistent profile, open it with the extension loaded, unlock
Freighter, approve the site once, then keep using that profile:

```bash
mkdir -p ~/.cache/stellpoker-freighter-profile
cd app
REAL_FREIGHTER_SMOKE=1 \
REAL_FREIGHTER_CONNECT=1 \
FREIGHTER_EXTENSION_PATH=/tmp/freighter-extension \
FREIGHTER_USER_DATA_DIR=~/.cache/stellpoker-freighter-profile \
xvfb-run -a npx playwright test \
  --config=e2e/playwright.config.ts \
  --project=chromium \
  tests/wallet-real-extension.spec.ts \
  --workers=1
```

The optional step fails if the profile is locked, the site is not approved, or
the wallet has no account. Do not commit the profile: it contains wallet
secrets.

### Environment variables

| Variable | Purpose |
|---|---|
| `REAL_FREIGHTER_SMOKE` | Must be `1` to enable the suite |
| `FREIGHTER_EXTENSION_PATH` | Path to the unpacked Freighter extension |
| `REAL_FREIGHTER_CONNECT` | Set to `1` for the optional full connection check |
| `REAL_FREIGHTER_HEADLESS` | Set to `1` to use headless Chromium (extensions may not load) |
| `FREIGHTER_USER_DATA_DIR` | Reuse a persistent, pre-approved profile instead of a temp profile |
| `E2E_BASE_URL` | Frontend URL (default `http://localhost:3000`) |

### Nightly automation

`.github/workflows/wallet-real-extension.yml` runs the extension injection and
detection smoke every night and on `workflow_dispatch`. It downloads the
official extension, installs Chromium, starts the frontend, and uploads the
Playwright report on failure. The full connection check requires a prepared
profile and is intentionally a manual release step rather than a CI secret.

### Troubleshooting

- **`FREIGHTER_EXTENSION_PATH does not exist`** — rerun
  `scripts/fetch-freighter-extension.py`, or point at an unpacked extension
  directory rather than the `.crx` file.
- **`CONNECT FREIGHTER` is not visible** — the extension did not inject. Check
  that Chromium was launched with `--load-extension`, that the extension is
  enabled in the profile, and that the page is not the headless shell.
- **`Unrecognized or altered Freighter wallet extension ID`** — the app's
  integrity check only allows the official Chrome/Firefox IDs. Use the official
  build, not a renamed development copy.
- **Full connection times out** — unlock Freighter, approve the site once in
  `FREIGHTER_USER_DATA_DIR`, and disable auto-lock for the duration of the run.
- **Web server already in use** — the Playwright config starts `npm run dev`
  when `CI=true`; stop any manual dev server first or run with `E2E_BASE_URL`
  pointing at it.

## Future Enhancements

- React component testing with wallet hooks
- Multi-wallet testing scenarios
- Performance testing under load
- Mobile wallet testing
- Hardware wallet simulation
