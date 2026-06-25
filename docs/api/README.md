# Coordinator API Documentation

Versioned API documentation for the StellPoker coordinator service, published
to GitHub Pages on each release tag.

## How it works

1. **OpenAPI spec** (`openapi.yaml`) — the current API specification is maintained
   here. Update it when endpoints, parameters, or response schemas change.

2. **Versioning** — when a tag matching `v*` is pushed, the
   `publish-api-docs.yml` workflow:
   - Copies the current `openapi.yaml` into `docs/api/<tag>/`
   - Compares it against the previous version's spec
   - Generates a changelog highlighting breaking changes
   - Updates the `versions.json` manifest
   - Deploys to GitHub Pages

3. **Viewing** — the `index.html` page uses Swagger UI to render the spec.
   A version selector in the top bar lets you browse any published release.

## Adding a new version

The workflow runs automatically on tag push. Tags must match `v*` (e.g. `v0.2.0`).

Before tagging, ensure:
- `docs/api/openapi.yaml` reflects all API changes
- The coordinator's `Cargo.toml` version is bumped if needed
- Manual entries in `versions.json` are updated if desired (the workflow will
  auto-generate the entry on publish)

## Breaking changes

The comparison script detects:
- Removed paths or HTTP methods
- Removed required parameters
- Changed response status codes (removed 2xx)
- Changed parameter types or formats

If breaking changes are detected, a banner is shown at the top of the docs page
with details and a link to the changelog.

## Local preview

```bash
# Serve locally
python3 -m http.server 8000 --directory docs/api
# Open http://localhost:8000
```
