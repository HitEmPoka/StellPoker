- closes #112
- closes #113
- closes #114
- closes #115

### Changes Made:
- **Authentication**: Implemented coordinator authentication using Stellar SEP-10 signed payloads.
- **Rate Limiting**: Added highly configurable per-wallet API rate limiting to the coordinator endpoints.
- **Performance**: Implemented an in-memory/disk cache for coordinator circuit artifacts to drastically speed up proving.
- **Documentation**: Added comprehensive API reference documentation for all coordinator endpoints in `docs/coordinator_api.md`.
