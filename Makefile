# @file Makefile
# @description Root developer interface for the local documentation reader.
# @created Diego Lafuente <diego.lafuente@cognativinc.com>

DOCS_VIEWER := tools/docs-viewer

.PHONY: documentation docs help

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
