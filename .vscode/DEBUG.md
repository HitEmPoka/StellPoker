# VS Code Debugging Guide

This directory contains VS Code configurations for debugging the StellPoker application.

## Setup

### Prerequisites

1. Install required extensions:
   - Rust Analyzer (`rust-lang.rust-analyzer`)
   - LLDB Debugger (`vadimcn.vscode-lldb`)
   - Chrome Debugger (`msjsdiag.debugger-for-chrome`)

2. Install development tools:
   - Rust toolchain: `rustup install stable`
   - Node.js 20+: For frontend debugging
   - Docker & Docker Compose: For service orchestration

### Environment Setup

1. Create a `.env.local` file in the project root with necessary environment variables:
   ```
   RUST_LOG=debug
   ```

2. Ensure the `circuits` and `crs` directories are populated:
   ```bash
   ./scripts/compile-circuits.sh
   ./scripts/download-crs.sh
   ```

## Debugging Configurations

### Rust Services (LLDB Debugger)

#### Coordinator Service
- **Configuration**: `Coordinator Debug`
- **Port**: 8080 (HTTP API)
- **Environment**: Uses local MPC nodes and Soroban RPC
- **How to debug**:
  1. Start MPC nodes and Soroban (use docker-compose or debug them separately)
  2. Press F5 and select "Coordinator Debug"
  3. Set breakpoints in coordinator source files

#### MPC Nodes
- **Configurations**: `MPC Node 0 Debug`, `MPC Node 1 Debug`, `MPC Node 2 Debug`
- **Ports**: 8101-8103 (HTTP API), 10000-10002 (MPC protocol)
- **Environment**: Each node has unique NODE_ID and port configuration
- **How to debug**:
  1. Start Coordinator and other nodes (debug separately or via docker-compose)
  2. Select the specific node configuration
  3. Set breakpoints in MPC node source files

#### Contract Tests
- **Configuration**: `Contract Tests Debug`
- **How to debug**:
  1. Press F5 and select "Contract Tests Debug"
  2. Set breakpoints in contract source files
  3. All test output appears in the debug console

### Frontend (Chrome Debugger)

- **Configuration**: `Frontend Debug (Chrome)`
- **Port**: 3000 (dev server)
- **How to debug**:
  1. Press F5 and select "Frontend Debug (Chrome)"
  2. This automatically starts the Next.js dev server
  3. Chrome opens with debugger attached
  4. Set breakpoints in TypeScript/JavaScript files
  5. Use browser DevTools for DOM/Network inspection

## Compound Configurations

### Full Stack Debug
- **Configuration**: `Full Stack Debug`
- Launches all services simultaneously for end-to-end debugging
- Coordinator, all 3 MPC nodes, and frontend
- Useful for debugging system interactions

## Build Tasks

Access via `Ctrl+Shift+B` or Terminal > Run Task:

- **cargo-check**: Validate Rust code without building
- **cargo-test**: Run all tests
- **docker-compose-up**: Start all services
- **docker-compose-down**: Stop all services
- **coordinator-build**: Build coordinator in release mode
- **mpc-node-build**: Build MPC node in release mode
- **frontend-dev-server**: Start Next.js dev server
- **terminate-frontend-server**: Stop dev server

## Tips & Troubleshooting

### Debugging Rust Services
- Use `RUST_LOG=debug` environment variable for detailed logging
- Set `RUST_BACKTRACE=1` for full stack traces
- Breakpoints work best with `-C` (optimized) or `-C debuginfo=2` builds
- Watch expressions work for local variables and struct fields

### Frontend Debugging
- Chrome DevTools opens automatically when debugging
- You can debug TypeScript with source maps
- Network requests to the API are visible in Network tab
- Use Redux DevTools extension for state inspection

### Common Issues

1. **"Cannot launch debugger"**
   - Ensure LLDB is installed: `llvm-tools-preview`
   - Check that Rust is installed: `rustc --version`

2. **"Port already in use"**
   - Check if services are already running
   - Use `docker-compose down` to stop containers
   - Kill existing processes: `lsof -i :8080` then `kill -9 <PID>`

3. **"Source files not found"**
   - Ensure workspace is opened at the project root
   - Check that breakpoint paths are correct in `.vscode/launch.json`
   - Clean build cache: `cargo clean`

## VS Code Extensions

Recommended extensions are listed in `extensions.json`. Install all recommendations:
1. Click on the notification popup, or
2. Run `Ctrl+Shift+X` and search for "Show Recommended Extensions"

## Further Reading

- [VS Code Debugging Documentation](https://code.visualstudio.com/docs/editor/debugging)
- [Rust Analyzer Documentation](https://rust-analyzer.github.io/)
- [LLDB Debugger Documentation](https://lldb.llvm.org/)
- [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
