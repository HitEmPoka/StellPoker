# Common Reference String (CRS) Distribution & Verification

## 1. Overview & Soundness Requirements

StellPoker circuits use the UltraHonk / BN254 proving system (via Noir and Barretenberg / co-noir). The proving and verification keys rely on a Common Reference String (CRS), specifically the structured reference string from the Aztec Ignition multi-party trusted setup ceremony (`bn254_g1.dat`).

Because a compromised, tampered, or mismatched CRS completely breaks the cryptographic soundness of zero-knowledge proofs (allowing adversaries to forge valid-looking proofs for illegal poker states, manipulated hands, or fake board cards), all CRS artifacts distributed or used across StellPoker must be integrity-verified against an in-repo pinned cryptographic hash before use.

## 2. Pinned Integrity Checksums

The authoritative SHA-256 checksum for the active BN254 CRS is committed directly to the repository:

- `scripts/crs.sha256` (authoritative source of truth used by scripts)
- `.crs/checksums.sha256` (local directory mirror)

### Current Pinned Hash
```text
c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470  bn254_g1.dat
```

## 3. Automated Verification Workflow

All scripts that download or consume the CRS enforce hard integrity verification:

1. **`scripts/download-crs.sh`**:
   - Parses the expected SHA-256 hash directly from `scripts/crs.sha256`.
   - If a local CRS file already exists, its SHA-256 checksum is computed and validated against the pinned hash.
   - If the file is missing or corrupted, it downloads the sealed transcript into an isolated temporary file (`.tmp.$$`).
   - The temporary download is checksum-verified *before* it is moved into place.
   - If the checksum does not match, the temporary file is purged and the script **fails hard (`exit 1`)** with an explicit error.
   - Supports `--verify-only` for CI/sanity checks without downloading.

2. **`scripts/compile-circuits.sh`**:
   - Invokes `scripts/download-crs.sh` prior to compiling any Noir circuits.
   - Any checksum mismatch immediately halts the compilation pipeline, preventing stale or untrusted CRS parameters from being baked into circuit artifacts or verification keys.

3. **`scripts/deploy-staging.sh` & `scripts/start-local.sh`**:
   - Automatically execute `scripts/download-crs.sh` during node and coordinator provisioning.

## 4. CRS Rotation Procedure

If the ceremony parameters are regenerated, upgraded, or rotated (e.g. migrating to a larger SRS size or a new ceremony transcript):

### Step 1: Obtain Canonical CRS Artifact
Download the canonical CRS file from the verified source (e.g., Aztec Ignition sealed ceremony release) into a secure staging environment.

### Step 2: Compute Canonical SHA-256
Compute the SHA-256 hash using independent tools:
```bash
sha256sum bn254_g1.dat
# or on macOS:
shasum -a 256 bn254_g1.dat
```
Verify the hash against at least two independent ceremony mirrors or participants.

### Step 3: Update In-Repo Checksums
Update both committed checksum files with the new hash:
```bash
echo "<NEW_SHA256_HASH>  bn254_g1.dat" > scripts/crs.sha256
echo "<NEW_SHA256_HASH>  bn254_g1.dat" > .crs/checksums.sha256
```

### Step 4: Validate Verification Scripts
Run the verification script locally to ensure clean download and verification:
```bash
rm -f .crs/bn254_g1.dat
./scripts/download-crs.sh
./scripts/download-crs.sh --verify-only
```

### Step 5: Recompile Circuits & Verification Keys
Recompile all circuits and generate updated verification keys using the rotated CRS:
```bash
./scripts/compile-circuits.sh --force
./scripts/convert-vk.py
```

### Step 6: Distribute to MPC Nodes
Distribute the new CRS and updated verification keys to all committee nodes in lockstep, verifying with `--verify-only` on each node before starting services.
