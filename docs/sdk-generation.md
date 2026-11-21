# OpenAPI-Driven Client SDK Generation

## Overview

StellPoker automatically generates TypeScript and Rust client SDKs from the canonical OpenAPI specification. This ensures clients never drift from the actual API implementation and eliminates manual SDK maintenance.

## How It Works

1. **Specification Source**: `docs/api/openapi.yaml` is the single source of truth
2. **Trigger**: Whenever OpenAPI spec changes, GitHub Actions generates fresh SDKs
3. **CI Integration**: `.github/workflows/generate-sdk.yml` orchestrates the generation
4. **Publishing**: TypeScript SDK published to npm, Rust SDK to crates.io
5. **Validation**: Generated code is linted and tested before publishing

## Generated SDKs

### TypeScript SDK

**Package**: `@stellpoker/coordinator-sdk`

Install:
```bash
npm install @stellpoker/coordinator-sdk
```

Use:
```typescript
import { CoordinatorApi, Configuration } from '@stellpoker/coordinator-sdk';

const config = new Configuration({
  basePath: 'https://coordinator.stellpoker.com',
  apiKey: process.env.COORDINATOR_API_KEY
});

const api = new CoordinatorApi(config);

// Example: Get table status
const tables = await api.getTables();
console.log(tables.data);
```

**Features**:
- Full TypeScript typings
- Fetch-based HTTP client
- Support for all auth methods (wallet, admin, API key)
- Automatic request/response marshalling
- Promise-based async API

### Rust SDK

**Crate**: `stellpoker_coordinator_sdk`

Add to Cargo.toml:
```toml
[dependencies]
stellpoker_coordinator_sdk = "0.1.0"
tokio = { version = "1.0", features = ["full"] }
```

Use:
```rust
use stellpoker_coordinator_sdk::apis::CoordinatorApi;
use stellpoker_coordinator_sdk::models::*;

#[tokio::main]
async fn main() {
    let client = CoordinatorApi::new("https://coordinator.stellpoker.com".to_string());
    
    match client.get_tables().await {
        Ok(tables) => println!("{:?}", tables),
        Err(e) => eprintln!("Error: {:?}", e),
    }
}
```

**Features**:
- Async/await with tokio
- Full error handling
- Strongly typed request/response models
- Support for all API features

## Generation Pipeline

### Workflow: `generate-sdk.yml`

```
Push to main with openapi.yaml changes
        ↓
    [Matrix jobs for TypeScript and Rust]
        ↓
    [Install OpenAPI Generator]
        ↓
    [Generate SDK from spec]
        ↓
    [Build and test SDK]
        ↓
    [Validate build output]
        ↓
    [Upload to artifact storage]
        ↓
    [Publish to npm/crates.io]
```

### Configuration Files

#### TypeScript Config (`scripts/openapi/ts-config.json`)

```json
{
  "npmName": "@stellpoker/coordinator-sdk",
  "supportsES6": true,
  "typescriptVersion": "~5.3.0",
  "modelPackage": "models",
  "apiPackage": "api"
}
```

#### Rust Config (`scripts/openapi/rust-config.json`)

```json
{
  "crateName": "stellpoker_coordinator_sdk",
  "crateVersion": "0.1.0",
  "edition": "2021",
  "supportAsync": true
}
```

## Integration with API Changes

### Updating the OpenAPI Spec

1. Edit `docs/api/openapi.yaml`
2. Add new endpoints, models, or auth methods
3. Commit and push to main
4. GitHub Actions automatically:
   - Generates new SDKs
   - Validates build succeeds
   - Publishes to registries
   - Creates PR if needed (for auto-commits)

### Example: Adding a New Endpoint

**Before** (old spec):
```yaml
paths:
  /api/tables:
    get:
      summary: List tables
      # ...
```

**After** (updated spec):
```yaml
paths:
  /api/tables:
    get:
      summary: List tables
      # ...
  /api/tables/{id}/config:
    get:
      summary: Get table configuration
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      # ...
```

SDK clients automatically gain the new `getTableConfig()` method.

## Accessing Generated SDKs

### From GitHub Artifacts

1. Go to workflow run: `.github/workflows/generate-sdk.yml`
2. Click **Artifacts** section
3. Download `typescript-sdk` or `rust-sdk` folder

### From Public Registries

**npm**:
```bash
npm search @stellpoker/coordinator-sdk
npm view @stellpoker/coordinator-sdk versions
```

**crates.io**:
```bash
cargo search stellpoker_coordinator_sdk
```

## Validation

### TypeScript SDK Validation

```bash
cd generated/typescript-sdk
npm run lint      # ESLint validation
npm run build     # TypeScript compilation
npm test          # Run test suite
```

### Rust SDK Validation

```bash
cd generated/rust-sdk
cargo clippy      # Linting
cargo check       # Type checking
cargo test        # Test suite
```

## Troubleshooting

### SDK Generation Failed

Check workflow logs:
```bash
gh run list --workflow=generate-sdk.yml --status failure
gh run view <RUN_ID> --log
```

### Build Errors After Generation

Common causes:
1. **OpenAPI spec syntax error**: Validate with `openapi-ts validate docs/api/openapi.yaml`
2. **Generator config mismatch**: Review `scripts/openapi/*-config.json`
3. **TypeScript version mismatch**: Check `tsconfig.json` compatibility

### SDK Not Published

1. Verify `NPM_TOKEN` secret is set in GitHub
2. Check npm publish logs in workflow
3. Verify package.json `version` field is set

## Best Practices

### For API Maintainers

1. **Keep OpenAPI spec in sync** with implementation
   - Add endpoint documentation before implementation
   - Update spec when adding features

2. **Use semantic versioning**
   - Breaking changes = major version
   - New endpoints = minor version
   - Bug fixes = patch version

3. **Test SDKs locally**
   ```bash
   npm link  # or cargo link
   # Test in consuming project
   ```

### For SDK Users

1. **Pin SDK version**
   ```json
   {
     "dependencies": {
       "@stellpoker/coordinator-sdk": "^0.1.0"
     }
   }
   ```

2. **Use generated types** everywhere
   ```typescript
   // ✅ Good
   import type { Table } from '@stellpoker/coordinator-sdk';
   const table: Table = await api.getTable(id);
   
   // ❌ Bad
   const table: any = await api.getTable(id);
   ```

3. **Check changelog** when updating SDKs
   ```bash
   npm show @stellpoker/coordinator-sdk@latest description
   ```

## Related Issues

- **#351**: Cost monitoring (base)
- **#668**: OpenAPI-driven client SDK generation (this feature)
- **#667**: Release automation

## See Also

- [OpenAPI 3.0 Specification](https://spec.openapis.org/oas/v3.0.3)
- [OpenAPI Generator Documentation](https://openapi-generator.tech/)
- API Specification: `docs/api/openapi.yaml`
- TypeScript SDK config: `scripts/openapi/ts-config.json`
- Rust SDK config: `scripts/openapi/rust-config.json`
