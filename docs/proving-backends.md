# Solo proving backend evaluation

Solo mode currently uses the same Noir/UltraHonk proving path as multiplayer.
The following acceptance matrix is the baseline for evaluating optional
accelerators; it keeps the browser path opt-in until every backend produces the
same proof bytes and public inputs.

| Backend | Target | Status | Gate |
| --- | --- | --- | --- |
| Native Barretenberg | server/desktop | baseline | existing circuit tests and proof verification |
| GPU Barretenberg | CUDA-capable host | evaluate | benchmark proving time and peak memory against baseline |
| WASM prover | modern browser | evaluate | Web Worker proof, no main-thread blocking, deterministic public inputs |

Evaluation procedure:

1. Compile each circuit with the same pinned Noir compiler and verification key.
2. Run ten warm and ten measured proofs for `deal_valid`, `reveal_board_valid`,
   and `showdown_valid` using identical vectors.
3. Record wall time, peak memory, proof size, and verification result. A backend
   is only eligible when all proofs verify and its public inputs byte-for-byte
   match the native baseline.
4. For browser-WASM, run in a Worker with a bounded heap and cancel the Worker
   on navigation; secrets must never be copied into page-visible logs.
5. Publish the measurements before enabling a feature flag. If a backend is
   unavailable, the coordinator falls back to native proving without changing
   the protocol or contract ABI.

This separates performance evaluation from protocol changes and gives GPU and
browser implementations a reproducible compatibility target.
