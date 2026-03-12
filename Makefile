BINDIR   := ./bin
BINARIES := shx dockerx dcx makex runx

# nix-shell -p go wraps the entire go invocation in --run so all flags reach go.
GOBUILD  = nix-shell -p go --run "go build -ldflags '-s -w' -o $@ $<cmd>"
GOTEST   = nix-shell -p go --run "go test ./... -v -timeout 120s"
GOVET    = nix-shell -p go --run "go vet ./..."

.PHONY: all build test lint clean install \
        smoke-test-shx smoke-test-makex smoke-test-dockerx smoke-test-dcx smoke-test

all: build

build: $(addprefix $(BINDIR)/,$(BINARIES))

$(BINDIR)/shx: $(shell find internal cmd/shx -name '*.go') go.mod
	@mkdir -p $(BINDIR)
	nix-shell -p go --run "go build -ldflags '-s -w' -o $@ ./cmd/shx"

$(BINDIR)/dockerx: $(shell find internal cmd/dockerx -name '*.go') go.mod
	@mkdir -p $(BINDIR)
	nix-shell -p go --run "go build -ldflags '-s -w' -o $@ ./cmd/dockerx"

$(BINDIR)/dcx: $(shell find internal cmd/dcx -name '*.go') go.mod
	@mkdir -p $(BINDIR)
	nix-shell -p go --run "go build -ldflags '-s -w' -o $@ ./cmd/dcx"

$(BINDIR)/makex: $(shell find internal cmd/makex -name '*.go') go.mod
	@mkdir -p $(BINDIR)
	nix-shell -p go --run "go build -ldflags '-s -w' -o $@ ./cmd/makex"

$(BINDIR)/runx: $(shell find internal cmd/runx -name '*.go') go.mod
	@mkdir -p $(BINDIR)
	nix-shell -p go --run "go build -ldflags '-s -w' -o $@ ./cmd/runx"

test:
	nix-shell -p go --run "go test ./... -v -timeout 120s"

# Race detector
test-race:
	nix-shell -p go --run "go test -race ./... -timeout 120s"

# Run just the fast unit tests (no git network ops)
test-unit:
	nix-shell -p go --run "go test ./internal/core/... -v -run 'TestParse|TestURL|TestSplit|TestCache|TestImmutable|TestMutable|TestCopy' -timeout 60s"

lint:
	nix-shell -p go --run "go vet ./..."

clean:
	rm -rf $(BINDIR)

# Install binaries to GOPATH/bin or /usr/local/bin
install: build
	install -m 0755 $(BINDIR)/shx $(BINDIR)/dockerx $(BINDIR)/dcx $(BINDIR)/makex $(BINDIR)/runx /usr/local/bin/

# Show the default cache directory
show-cache:
	@echo "$$HOME/.cache/runx"

# Purge all runx caches
purge-cache:
	rm -rf "$$HOME/.cache/runx"
	@echo "Cache purged."

# ---------------------------------------------------------------------------
# Smoke tests — exercise each tool against real GitHub repos.
# Requirements: git, sh, make on PATH. Docker targets need Docker running.
# All trust prompts are suppressed with --yes.
# First run clones from GitHub; subsequent runs hit the local cache.
# ---------------------------------------------------------------------------

# smoke-test-shx
# Repo:   rbenv/rbenv — Ruby version manager, 100% shell, tiny (~1 MB)
# Script: bin/rbenv
# What:   "rbenv help" prints usage and exits 0; grep confirms output
smoke-test-shx: $(BINDIR)/shx
	@echo "==> smoke: shx  (rbenv/rbenv — bin/rbenv help)"
	./$(BINDIR)/shx --yes rbenv/rbenv@master bin/rbenv help 2>&1 | grep -q 'rbenv'
	@echo "    PASS"

# smoke-test-makex
# Repo:   bswrundquist/vcoder — has .DEFAULT_GOAL := help; the help target
#         uses only echo/grep/awk (no extra tools) and exits 0
# What:   omitting the target runs the default (help), printing available targets
smoke-test-makex: $(BINDIR)/makex
	@echo "==> smoke: makex (bswrundquist/vcoder — make help)"
	./$(BINDIR)/makex --yes bswrundquist/vcoder@main help 2>&1 | grep -q 'vcoder'
	@echo "    PASS"

# smoke-test-dockerx
# Repo:   rbenv/rbenv — already cached from smoke-test-shx, so this is instant
# What:   "docker info --format '{{.ServerVersion}}'" prints the Docker version
#         and exits 0; proves dockerx clones, sets CWD, and execs docker correctly.
#         Building an actual image would test Docker, not dockerx.
# Guard:  skipped gracefully if Docker daemon is not running
smoke-test-dockerx: $(BINDIR)/dockerx
	@docker info >/dev/null 2>&1 || { echo "    SKIP: docker not running"; exit 0; }
	@echo "==> smoke: dockerx (rbenv/rbenv — docker info)"
	./$(BINDIR)/dockerx --yes rbenv/rbenv@master \
	    -- info --format '{{.ServerVersion}}' 2>&1 | grep -qE '^[0-9]+\.'
	@echo "    PASS"

# smoke-test-dcx
# Repo:   dockersamples/wordsmith — canonical multi-service Compose demo
#         (web + dispatcher + db), has a docker-compose.yml at repo root
# What:   "docker compose config --quiet" validates the file without
#         pulling images or starting any containers; exits 0 on a valid file
# Guard:  skipped gracefully if Docker daemon is not running
smoke-test-dcx: $(BINDIR)/dcx
	@docker info >/dev/null 2>&1 || { echo "    SKIP: docker not running"; exit 0; }
	@echo "==> smoke: dcx  (dockersamples/wordsmith — compose config)"
	./$(BINDIR)/dcx --yes dockersamples/wordsmith@main \
	    -- config --quiet
	@echo "    PASS"

# Run all smoke tests
smoke-test: smoke-test-shx smoke-test-makex smoke-test-dockerx smoke-test-dcx
	@echo "==> All smoke tests passed."
