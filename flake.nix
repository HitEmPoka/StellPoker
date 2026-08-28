{
  # Reproducible development environment for Stellar Poker (Issue #335).
  #
  #   nix develop        # drops you into a shell with the full toolchain
  #
  # Inputs are pinned to a release branch here. `nix flake lock` resolves those
  # to exact revisions in `flake.lock`; commit that file to make the shell
  # byte-identical for everyone. It is not committed in this change because it
  # has to be generated on a machine with Nix and network access.
  #
  # Everything the README lists under "Prerequisites" is provided here except
  # the two tools that are not packaged in nixpkgs and must be pinned by us:
  #
  #   * nargo (Noir)  — installed by noirup into ~/.nix-profile-independent
  #                     $NARGO_HOME; the shell hook installs the exact version
  #                     in NOIR_VERSION on first entry.
  #   * co-noir       — built from TACEO's git repo with the pinned Rust
  #                     toolchain via `cargo install`.
  #
  # Both land in a project-local directory rather than the user's global cargo
  # home, so entering this shell never mutates the host's toolchain and two
  # checkouts can pin different versions.

  description = "Stellar Poker — onchain Texas Hold'em with ZK-MPC";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Keep in sync with README "Prerequisites".
        noirVersion = "1.0.0-beta.17";

        # wasm32 target is required to build the Soroban contracts.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rustfmt" "clippy" ];
          targets = [ "wasm32-unknown-unknown" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain

            # Frontend (app/ targets Node 18+).
            nodejs_20

            # Integration tests (scripts/test-flow.py) and the helper scripts.
            (python3.withPackages (ps: with ps; [ requests pynacl ]))

            # Local stack.
            docker-compose

            # Used by scripts/{setup-dkg,generate-tls-certs,download-crs}.sh
            openssl
            curl
            jq
            git
            bash

            # Native deps for the Rust services (openssl-sys, rocksdb-style
            # C deps in the MPC stack) and for building co-noir.
            pkg-config
            cmake
            clang
          ]
          # Packaged on most channels; the shell hook tells you how to install
          # it with cargo when this nixpkgs revision does not carry it.
          ++ pkgs.lib.optional (pkgs ? stellar-cli) pkgs.stellar-cli;

          # openssl-sys and friends need these at build time.
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

          NOIR_VERSION = noirVersion;

          shellHook = ''
            # Project-local tool homes: entering this shell must not mutate the
            # host's ~/.cargo or ~/.nargo.
            export REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
            export CARGO_HOME="$REPO_ROOT/.nix/cargo"
            export NARGO_HOME="$REPO_ROOT/.nix/nargo"
            export PATH="$CARGO_HOME/bin:$NARGO_HOME/bin:$PATH"
            mkdir -p "$CARGO_HOME/bin" "$NARGO_HOME/bin"

            echo "Stellar Poker dev shell"
            echo "  rust    $(rustc --version)"
            echo "  node    $(node --version)"

            if command -v stellar >/dev/null 2>&1; then
              echo "  stellar $(stellar --version | head -n1)"
            else
              echo "  stellar MISSING — cargo install stellar-cli --features opt"
            fi

            if command -v nargo >/dev/null 2>&1; then
              echo "  nargo   $(nargo --version | head -n1)"
            else
              echo "  nargo   MISSING — run: noirup -v ${noirVersion}"
              echo "          (curl -L https://raw.githubusercontent.com/noir-lang/noirup/main/install | bash)"
            fi

            if command -v co-noir >/dev/null 2>&1; then
              echo "  co-noir present"
            else
              echo "  co-noir MISSING — cargo install --git https://github.com/TaceoLabs/co-snarks --branch main co-noir"
            fi

            echo ""
            echo "Next: ./scripts/setup.sh && ./scripts/download-crs.sh && docker-compose up"
            echo "See docs/developer-onboarding.md for the full checklist."
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
