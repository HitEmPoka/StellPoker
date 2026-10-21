# Contributing to StellPoker Circuits

Welcome to the StellPoker circuit development guide. This document explains how to set up your environment, write, test, and integrate Noir circuits for the project.

## 1. Noir Toolchain Setup
StellPoker uses [Noir](https://noir-lang.org/) for writing zero-knowledge circuits.
- Install `nargo` (the Noir package manager) using the noirup script.
- Ensure your `nargo` version matches the one specified in the project (check `circuits/Nargo.toml` if applicable).
- Run `nargo compile` in the `circuits/` directory to build the ACIR artifacts.

## 2. Writing and Testing Circuits
- **Structure:** Circuits are located in the `circuits/` directory. Each sub-folder typically represents an independent Noir project.
- **Testing:** Write Noir tests alongside your circuits. Run tests using `nargo test` in the respective circuit directory.
- **Proving:** Use the `nargo prove` and `nargo verify` commands to test proof generation and verification locally.

## 3. Constraint Optimization
- Aim to keep the number of constraints minimal as proof generation happens in constrained environments.
- Use built-in Noir functions where possible.
- Avoid complex loops and unbounded array operations.

## 4. Adding New Circuits
When adding a new circuit:
1. Create a new directory under `circuits/` (e.g., `circuits/new_feature`).
2. Initialize it with `nargo init`.
3. Add it to the build pipeline in our CI/CD setup or Makefile.
4. Ensure compiled artifacts are output to the directory expected by the Coordinator (configured via `CIRCUIT_DIR`).

## 5. MPC Integration Checklist
To integrate a new circuit with the MPC (Multi-Party Computation) coordinator nodes:
- [ ] Ensure the circuit compiles to a valid ACIR artifact.
- [ ] Place the compiled `.json` artifacts in `CIRCUIT_DIR` (default: `./circuits`).
- [ ] Generate the necessary CRS (Common Reference String) for the new circuit and place it in `CRS_DIR` (default: `./crs`).
- [ ] Update the Coordinator configuration to load the new circuit files.
- [ ] Test the integration locally by running the 3 MPC nodes (`MPC_NODE_0`, `MPC_NODE_1`, `MPC_NODE_2`).
- [ ] If gating the circuit behind a feature flag, test with `FEATURE_FLAG_NEW_CIRCUITS=1`.
