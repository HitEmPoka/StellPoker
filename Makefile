SHELL := /usr/bin/env bash

.DEFAULT_GOAL := help

## help: Show this help (default target).
.PHONY: help
help:
	@echo "Stellar Poker — common development commands"
	@echo ""
	@grep -E '^## [a-zA-Z0-9_-]+:' $(MAKEFILE_LIST) | sed -E 's/^## /  /' | sort

## build: Build Soroban contracts and the frontend.
.PHONY: build
build:
	cargo build --workspace
	cd app && npm run build

## test: Run contract, circuit, and frontend tests.
.PHONY: test
test:
	cargo test --workspace
	cd app && npm test

## lint: Run formatting and lint checks (Rust + TypeScript).
.PHONY: lint
lint:
	cargo fmt --all --check
	cargo clippy --workspace -- -D warnings
	cd app && npm run lint

## deploy-local: Deploy contracts to the local Soroban network.
.PHONY: deploy-local
deploy-local:
	./scripts/deploy-local.sh

## start: Start the local dev stack (Soroban, MPC nodes, coordinator) via docker-compose.
.PHONY: start
start:
	docker-compose up -d

## stop: Stop the local dev stack.
.PHONY: stop
stop:
	docker-compose down

## pr-preview: Start an isolated preview stack for a PR. Usage: make pr-preview PR=123.
.PHONY: pr-preview
pr-preview:
	@test -n "$(PR)" || (echo "usage: make pr-preview PR=<n>" >&2; exit 2)
	./scripts/pr-preview.sh up $(PR)

## pr-preview-down: Tear down a PR preview stack (run on merge/close). Usage: make pr-preview-down PR=123.
.PHONY: pr-preview-down
pr-preview-down:
	@test -n "$(PR)" || (echo "usage: make pr-preview-down PR=<n>" >&2; exit 2)
	./scripts/pr-preview.sh down $(PR)

## clean: Remove build artifacts (cargo target dirs, frontend build output and node_modules).
.PHONY: clean
clean:
	cargo clean
	rm -rf app/.next app/node_modules

## preflight: Verify prerequisites (tools, env, services health) before building.
.PHONY: preflight
preflight:
	./scripts/preflight.sh

## ci: Run the same checks CI runs (fmt, clippy, tests, build) locally before pushing.
.PHONY: ci
ci: lint test build
	@echo "All CI checks passed locally."
