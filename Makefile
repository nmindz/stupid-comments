CARGO ?= cargo
ROOT  ?= $(HOME)/.local
CRATE := crates/stupid-comments
BIN   := stupid-comments

.DEFAULT_GOAL := help
.PHONY: help build test lint validate check install uninstall selfcheck clean

help: ## List the available targets
	@awk -F':.*## ' '/^[a-z][a-z-]*:.*## /{printf "  %-10s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Compile the release binary
	$(CARGO) build --release

test: ## Run the test suite
	$(CARGO) test

lint: ## Run clippy across every target
	$(CARGO) clippy --all-targets

validate: ## Validate the plugin and marketplace manifests
	claude plugin validate plugins/stupid-comments
	claude plugin validate .

check: test lint validate ## Everything CI would run

install: ## Install the binary (ROOT defaults to ~/.local)
	$(CARGO) install --path $(CRATE) --root $(ROOT) --force
	@if command -v $(BIN) >/dev/null 2>&1; then \
		echo "installed: $$(command -v $(BIN)) -> $$($(BIN) --version)"; \
	else \
		echo "WARNING: $(ROOT)/bin is not on PATH."; \
		echo "The plugin looks the binary up on PATH, so it will stay inert."; \
		echo "Add it to PATH, or reinstall with ROOT=\$$HOME/.cargo"; \
	fi

uninstall: ## Remove the installed binary
	$(CARGO) uninstall --root $(ROOT) $(BIN)

selfcheck: build ## Enforce this repo's own comment policy on itself
	./target/release/$(BIN) check .

clean: ## Remove build artifacts
	$(CARGO) clean
