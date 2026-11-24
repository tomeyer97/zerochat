# zerochat: `make` builds the web UI, embeds it in the server and opens it in your browser.
# Pass server flags with ARGS, e.g. `make run ARGS="--no-open --listen 127.0.0.1:4400"`.

ARGS ?=
WEB_SRC := $(shell find web/src -type f) web/index.html web/vite.config.ts web/tsconfig.json

.DEFAULT_GOAL := run
.PHONY: check-node run remote dev release install test clean help goldens types package notices check-targets fmt lint

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-8s %s\n", $$1, $$2}'

# Every target that uses npm needs it first: node and npm in the range of web/package.json's engines.
check-node:
	@scripts/check-node.sh

# npm ci only when the lockfile changed since the last install (the Node check runs every time).
web/node_modules/.package-lock.json: web/package-lock.json | check-node
	npm --prefix web ci

# The server embeds web/dist at compile time (crates/zerochat/build.rs), so build it first.
web/dist/index.html: web/node_modules/.package-lock.json $(WEB_SRC)
	npm --prefix web run build

# zsf is built next to zerochat (target/debug/zsf), where zerochat looks for it, so Features works.
run: web/dist/index.html ## Build everything (zerochat and zsf) and serve on 127.0.0.1:4380 (or the next free port)
	cargo build -p zsf
	cargo run -p zerochat -- $(ARGS)

remote: web/dist/index.html ## Serve behind a reverse proxy that logs you in: no Host-header (DNS-rebinding) check, no browser
	cargo build -p zsf
	cargo run -p zerochat -- --any-host --no-open $(ARGS)

dev: web/node_modules/.package-lock.json ## Server + Vite hot reload on http://127.0.0.1:5174
	cargo build -p zsf
	@trap 'kill 0' INT TERM EXIT; \
	cargo run -p zerochat -- --no-open $(ARGS) & \
	npm --prefix web run dev

release: web/dist/index.html ## Optimized binary at target/release/zerochat
	cargo build --release -p zerochat

install: web/dist/index.html ## Install `zerochat` and `zsf` into ~/.cargo/bin
	cargo install --path crates/zerochat
	cargo install --path crates/zsf

package: check-node ## Build the npm packages locally (static musl on Linux when installed, else a labelled glibc test build) and list them with `npm pack --dry-run`; publishes nothing
	scripts/package.sh

notices: web/node_modules/.package-lock.json ## Write target/THIRD_PARTY_NOTICES.md (licences of the code in the binaries; needs cargo-about)
	scripts/third-party-notices.sh

check-targets: ## `cargo check` for Windows and macOS (scripts/check-targets.sh)
	scripts/check-targets.sh

test: web/node_modules/.package-lock.json ## Rust, web and npm-shim tests
	cargo test --workspace
	npm --prefix web test
	node --test npm/test/*.test.js

goldens: web/node_modules/.package-lock.json ## Golden tests (tests/GOLDENS.md); UPDATE_GOLDENS=1 to accept new output
	cargo test -p zerochat goldens::
	cargo test -p zsf --test goldens
	npm --prefix web test

fmt: web/node_modules/.package-lock.json ## Format the Rust code (rustfmt) and the web UI (Biome, with its safe lint fixes)
	cargo fmt --all
	npm --prefix web run fix

lint: web/node_modules/.package-lock.json ## The checks CI runs: rustfmt, clippy with warnings as errors, Biome (writes nothing)
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	npm --prefix web run lint

types: ## Regenerate web/src/generated/ (TypeScript twins of the Rust API types) from the Rust types
	UPDATE_TYPES=1 cargo test -p zerochat api_types::

clean: ## Remove build output (keeps node_modules)
	cargo clean
	rm -rf web/dist
