# @file Makefile
# @description Root developer interface for the local documentation reader.
# @created Diego Martín Lafuente <meerita@icloud.com>

DOCS_VIEWER := tools/docs-viewer

.PHONY: documentation docs help fmt fmt-check check clippy test i18n-check

## help: List the available targets
help:
	@grep -hE '^## ' $(MAKEFILE_LIST) | sed -e 's/^## //' | awk -F': ' '{ printf "  %-20s %s\n", $$1, $$2 }'

## documentation: Launch the local documentation reader (http://127.0.0.1:4000)
documentation:
	@command -v bun >/dev/null 2>&1 || { echo "bun is required: https://bun.sh"; exit 1; }
	@if [ ! -d "$(DOCS_VIEWER)" ]; then \
		echo "$(DOCS_VIEWER) does not exist. It is not tracked in git and must be present on disk to run this target."; \
		exit 1; \
	fi
	@if [ ! -d "$(DOCS_VIEWER)/node_modules" ]; then \
		echo "Installing docs-viewer dependencies..."; \
		cd $(DOCS_VIEWER) && bun install; \
	fi
	@echo "Serving docs at http://127.0.0.1:4000 (Ctrl-C to stop)"
	@cd $(DOCS_VIEWER) && bun run dev

## docs: Alias for `documentation`
docs: documentation

## fmt: Format all workspace sources
fmt:
	cargo fmt --all

## fmt-check: Check formatting without writing changes
fmt-check:
	cargo fmt --all --check

## check: Type-check the whole workspace
check:
	cargo check --workspace --all-targets

## clippy: Lint the whole workspace and deny warnings
clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

## test: Run the workspace test suite
test:
	cargo test --workspace --all-targets

## i18n-check: Validate localization catalogues and scan for prose leaks
i18n-check:
	cargo run --quiet --package localization-check
