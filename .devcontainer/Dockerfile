# Dev container / Codespaces image for Stellar Poker (Issue #671).
#
# This mirrors `devShells.default` in flake.nix so `nix develop` and
# "Reopen in Container" give the same toolchain. When you change one, change
# the other — scripts/check-devenv-parity.sh (run in CI) fails on drift in the
# pinned versions and the package list.
#
# flake.nix                          -> here
#   rust stable + rust-src/rustfmt/clippy, wasm32-unknown-unknown
#                                    -> rustup (base image) + components below
#   nodejs_20                        -> devcontainer node feature, version 20
#   python3 + requests + pynacl      -> python3-requests, python3-nacl
#   docker-compose                   -> docker-outside-of-docker feature
#   openssl curl jq git bash         -> apt
#   pkg-config cmake clang           -> apt (+ libclang-dev, libssl-dev)
#   stellar-cli                      -> cargo install (STELLAR_CLI_VERSION)
#   nargo (noirup, NOIR_VERSION)     -> noirup -v $NOIR_VERSION
#   co-noir (git main)               -> cargo install --git ... co-noir

FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Keep in sync with `noirVersion` in flake.nix.
ARG NOIR_VERSION=1.0.0-beta.17
ARG STELLAR_CLI_VERSION=28.1.0

ENV NOIR_VERSION=${NOIR_VERSION}

RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        bash \
        clang \
        cmake \
        curl \
        git \
        jq \
        libclang-dev \
        libssl-dev \
        openssl \
        pkg-config \
        python3 \
        python3-nacl \
        python3-requests \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# bindgen (used by native deps in the MPC stack) finds libclang through this,
# matching LIBCLANG_PATH in the flake.
ENV LIBCLANG_PATH=/usr/lib/llvm-14/lib

USER vscode

# Rust: stable toolchain with the same components and wasm target as the flake.
RUN rustup toolchain install stable --profile minimal \
        --component rust-src --component rustfmt --component clippy \
    && rustup default stable \
    && rustup target add wasm32-unknown-unknown

# Stellar CLI (Soroban contract build/deploy).
RUN cargo install --locked stellar-cli --version "${STELLAR_CLI_VERSION}" --features opt

# co-noir from TACEO's main branch, same source as flake.nix and the service
# Dockerfiles.
RUN cargo install --git https://github.com/TaceoLabs/co-snarks --branch main co-noir

# Nargo at the pinned Noir version.
RUN curl -fsSL https://raw.githubusercontent.com/noir-lang/noirup/main/install | bash \
    && /home/vscode/.nargo/bin/noirup -v "${NOIR_VERSION}"

ENV PATH="/home/vscode/.nargo/bin:${PATH}"
