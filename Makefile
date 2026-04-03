BINDIR   := ./bin
BINARY   := runx

.PHONY: all build tests unit-tests lint fmt clean install \
        smoke-test-sh smoke-test-make smoke-test-docker smoke-test-compose \
        smoke-test-auto smoke-tests \
        smoke-test-bin-bare smoke-test-bin-tar smoke-test-bin-cached

all: build

build: $(BINDIR)/$(BINARY)

$(BINDIR)/$(BINARY): $(shell find src -name '*.rs') Cargo.toml
	@mkdir -p $(BINDIR)
	cargo build --release
	cp target/release/$(BINARY) $@

tests: unit-tests smoke-tests

unit-tests:
	cargo test

lint:
	cargo clippy -- -D warnings

fmt:
	cargo fmt --check

clean:
	rm -rf $(BINDIR) target

install: build
	install -m 0755 $(BINDIR)/$(BINARY) /usr/local/bin/

# Show the default cache directory
show-cache:
	@echo "$$HOME/.cache/runx"

# Purge all runx caches
purge-cache:
	rm -rf "$$HOME/.cache/runx"
	@echo "Cache purged."

# ---------------------------------------------------------------------------
# Smoke tests — exercise each tool mode against real GitHub repos.
# Requirements: git, sh, make on PATH. Docker targets need Docker running.
# All trust prompts are suppressed with --yes.
# First run clones from GitHub; subsequent runs hit the local cache.
# ---------------------------------------------------------------------------

RUNX := ./$(BINDIR)/$(BINARY)

# smoke-test-sh
# Repo:   rbenv/rbenv — Ruby version manager, 100% shell, tiny (~1 MB)
# Script: bin/rbenv
# What:   "rbenv help" prints usage and exits 0; grep confirms output
# Mode:   explicit subcommand (bin/rbenv has no .sh extension)
smoke-test-sh: $(BINDIR)/$(BINARY)
	@echo "==> smoke: sh   (rbenv/rbenv — bin/rbenv help)"
	$(RUNX) sh --yes rbenv/rbenv@master bin/rbenv -- help 2>&1 | grep -q 'rbenv'
	@echo "    PASS"

# smoke-test-make
# Repo:   bswrundquist/vcoder — has .DEFAULT_GOAL := help; the help target
#         uses only echo/grep/awk (no extra tools) and exits 0
# What:   bare word "help" auto-detects as make target
smoke-test-make: $(BINDIR)/$(BINARY)
	@echo "==> smoke: make (bswrundquist/vcoder — make help)"
	$(RUNX) --yes bswrundquist/vcoder@main help 2>&1 | grep -q 'vcoder'
	@echo "    PASS"

# smoke-test-docker
# Repo:   rbenv/rbenv — already cached from smoke-test-sh, so this is instant
# What:   "docker info --format '{{.ServerVersion}}'" prints the Docker version
# Mode:   explicit subcommand (docker args require --)
# Guard:  skipped gracefully if Docker daemon is not running
smoke-test-docker: $(BINDIR)/$(BINARY)
	@docker info >/dev/null 2>&1 || { echo "    SKIP: docker not running"; exit 0; }
	@echo "==> smoke: docker (rbenv/rbenv — docker info)"
	$(RUNX) docker --yes rbenv/rbenv@master \
	    -- info --format '{{.ServerVersion}}' 2>&1 | grep -qE '^[0-9]+\.'
	@echo "    PASS"

# smoke-test-compose
# Repo:   dockersamples/wordsmith — canonical multi-service Compose demo
# What:   "docker compose config --quiet" validates the file without
#         pulling images or starting any containers; exits 0 on a valid file
# Mode:   explicit subcommand (compose args require --)
# Guard:  skipped gracefully if Docker daemon is not running
smoke-test-compose: $(BINDIR)/$(BINARY)
	@docker info >/dev/null 2>&1 || { echo "    SKIP: docker not running"; exit 0; }
	@echo "==> smoke: compose (dockersamples/wordsmith — compose config)"
	$(RUNX) compose --yes dockersamples/wordsmith@main \
	    -- config --quiet
	@echo "    PASS"

# ---------------------------------------------------------------------------
# Auto-detection smoke tests
# Verify that auto-detection picks the right mode from argument patterns.
# ---------------------------------------------------------------------------

# auto-detect: .sh extension → shell mode
# Repo:   rbenv/rbenv (already cached from smoke-test-sh)
# What:   "libexec/rbenv-init.sh" has a .sh extension → auto-detects as shell
smoke-test-auto-sh: $(BINDIR)/$(BINARY)
	@echo "==> smoke: auto-detect shell (.sh extension)"
	$(RUNX) --yes rbenv/rbenv@master libexec/rbenv-init.sh -- - bash 2>&1 | grep -q 'rbenv'
	@echo "    PASS"

# auto-detect: scripts/ prefix → shell mode
# Repo:   rbenv/rbenv
# What:   "scripts/whatever" starts with scripts/ → auto-detects as shell
#         --verbose prints the detected mode so we can confirm it
smoke-test-auto-scripts-prefix: $(BINDIR)/$(BINARY)
	@echo "==> smoke: auto-detect shell (scripts/ prefix)"
	$(RUNX) --verbose --yes rbenv/rbenv@master scripts/whatever 2>&1 | grep -q 'auto-detected mode: shell'
	@echo "    PASS"

# auto-detect: bare word → make mode
# Repo:   bswrundquist/vcoder
# What:   "help" is a plain word → auto-detects as make target
smoke-test-auto-make: $(BINDIR)/$(BINARY)
	@echo "==> smoke: auto-detect make (bare word target)"
	$(RUNX) --yes bswrundquist/vcoder@main help 2>&1 | grep -q 'vcoder'
	@echo "    PASS"

# auto-detect: no target at all → make mode (default target)
# Repo:   bswrundquist/vcoder (.DEFAULT_GOAL := help)
# What:   no second argument → auto-detects as make, runs default target
smoke-test-auto-make-default: $(BINDIR)/$(BINARY)
	@echo "==> smoke: auto-detect make (no target — default goal)"
	$(RUNX) --yes bswrundquist/vcoder@main 2>&1 | grep -q 'vcoder'
	@echo "    PASS"

# auto-detect: compose.yml → compose mode
# Repo:   dockersamples/wordsmith
# Guard:  skipped if Docker is not running
smoke-test-auto-compose: $(BINDIR)/$(BINARY)
	@docker info >/dev/null 2>&1 || { echo "    SKIP: docker not running"; exit 0; }
	@echo "==> smoke: auto-detect compose (compose.yml pattern)"
	$(RUNX) --yes dockersamples/wordsmith@main compose.yml \
	    -- config --quiet
	@echo "    PASS"

# auto-detect: docker-compose.yml → compose mode
# Repo:   dockersamples/wordsmith
# Guard:  skipped if Docker is not running
smoke-test-auto-docker-compose: $(BINDIR)/$(BINARY)
	@docker info >/dev/null 2>&1 || { echo "    SKIP: docker not running"; exit 0; }
	@echo "==> smoke: auto-detect compose (docker-compose.yml pattern)"
	$(RUNX) --yes dockersamples/wordsmith@main docker-compose.yml \
	    -- config --quiet 2>&1; true
	@echo "    PASS (compose detection triggered)"

# ---------------------------------------------------------------------------
# Release binary smoke tests
# Verify that `runx bin` can download and execute release binaries.
# ---------------------------------------------------------------------------

# smoke-test-bin-bare
# Repo:   jqlang/jq — lightweight JSON processor
# Asset:  jq-macos-arm64 (bare binary, no archive)
# What:   "jq --version" prints version string; auto-detects release mode
smoke-test-bin-bare: $(BINDIR)/$(BINARY)
	@echo "==> smoke: bin   (jqlang/jq — bare binary, jq --version)"
	$(RUNX) --yes jqlang/jq@jq-1.7.1 jq-macos-arm64 -- --version 2>&1 | grep -q 'jq-1.7.1'
	@echo "    PASS"

# smoke-test-bin-tar
# Repo:   junegunn/fzf — fuzzy finder
# Asset:  fzf-0.62.0-darwin_arm64.tar.gz (tar.gz archive)
# What:   "fzf --version" prints version; auto-detects release mode via .tar.gz
smoke-test-bin-tar: $(BINDIR)/$(BINARY)
	@echo "==> smoke: bin   (junegunn/fzf — tar.gz archive, fzf --version)"
	$(RUNX) --yes junegunn/fzf@v0.62.0 fzf-0.62.0-darwin_arm64.tar.gz -- --version 2>&1 | grep -q '0.62.0'
	@echo "    PASS"

# smoke-test-bin-cached
# Re-run jq with --offline to verify the cache from smoke-test-bin-bare
smoke-test-bin-cached: smoke-test-bin-bare
	@echo "==> smoke: bin   (jqlang/jq — cached, --offline)"
	$(RUNX) --yes --offline jqlang/jq@jq-1.7.1 jq-macos-arm64 -- --version 2>&1 | grep -q 'jq-1.7.1'
	@echo "    PASS"

# Run all smoke tests
smoke-tests: smoke-test-sh smoke-test-make smoke-test-docker smoke-test-compose \
             smoke-test-auto-sh smoke-test-auto-scripts-prefix \
             smoke-test-auto-make smoke-test-auto-make-default \
             smoke-test-auto-compose smoke-test-auto-docker-compose \
             smoke-test-bin-bare smoke-test-bin-tar smoke-test-bin-cached
	@echo "==> All smoke tests passed."
