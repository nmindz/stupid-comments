CARGO ?= cargo
NODE  ?= node
ROOT  ?= $(HOME)/.local
CRATE := crates/stupid-comments
BIN   := stupid-comments
DSH   := plugins/stupid-comments/dsh
PI    := plugins/stupid-comments/pi
DSH_PROFILE ?= tui

.DEFAULT_GOAL := help
.PHONY: help build test dsh-test pi-test lint version validate check install uninstall dsh-install dsh-uninstall pi-install pi-uninstall selfcheck clean

help: ## List the available targets
	@awk -F':.*## ' '/^[a-z][a-z-]*:.*## /{printf "  %-14s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Compile the release binary
	$(CARGO) build --release

test: ## Run the test suite
	$(CARGO) test

dsh-test: build ## Drive the DSH adapter against the release binary
	$(NODE) $(DSH)/test.mjs

pi-test: build ## Drive the Pi extension against the release binary
	$(NODE) $(PI)/test.mjs

lint: ## Run clippy across every target
	$(CARGO) clippy --all-targets

version: ## Write one version into every manifest (VERSION=X.Y.Z)
	$(NODE) scripts/sync-version.mjs $(VERSION)

validate: ## Validate the Claude Code, DSH and Pi plugin manifests
	claude plugin validate plugins/stupid-comments
	claude plugin validate .
	$(NODE) --check $(DSH)/index.js
	$(NODE) --check $(PI)/index.js
	$(NODE) scripts/validate-dsh-manifest.mjs
	$(NODE) scripts/validate-pi-manifest.mjs

check: test dsh-test pi-test lint validate ## Everything CI would run

install: ## Install the binary (ROOT defaults to ~/.local)
	$(CARGO) install --path $(CRATE) --root $(ROOT) --force
	@if command -v $(BIN) >/dev/null 2>&1; then \
		echo "installed: $$(command -v $(BIN)) -> $$($(BIN) --version)"; \
	else \
		echo "WARNING: $(ROOT)/bin is not on PATH."; \
		echo "Every plugin looks the binary up on PATH, so it will stay inert."; \
		echo "Add it to PATH, or reinstall with ROOT=\$$HOME/.cargo"; \
	fi

uninstall: ## Remove the installed binary
	$(CARGO) uninstall --root $(ROOT) $(BIN)

dsh-install: ## Register this checkout with a dsh profile (DSH_PROFILE defaults to tui)
	dsh plugin --profile $(DSH_PROFILE) add $(CURDIR)

dsh-uninstall: ## Undo dsh-install
	dsh plugin --profile $(DSH_PROFILE) remove $(BIN)

pi-install: ## Register this checkout as a Pi package
	pi install $(CURDIR)

pi-uninstall: ## Undo pi-install
	pi remove $(CURDIR)

selfcheck: build ## Enforce this repo's own comment policy on itself
	./target/release/$(BIN) check .

clean: ## Remove build artifacts
	$(CARGO) clean
