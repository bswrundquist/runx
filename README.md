# runx

Execute shell scripts, Makefile targets, Docker commands, and Docker Compose
**directly from Git repositories** — no manual clone, no setup.

Think of it as `npx`/`uvx` for git repos.

---

## Usage

`runx` is a single binary with subcommands:

```bash
runx sh      acme/tools@main scripts/release.sh -- --dry-run
runx make    acme/infra@main bootstrap
runx docker  acme/app@main -- build -t acme/app:dev .
runx compose acme/platform@main -- up -d
```

Auto-detection also works — `runx` infers the tool from the arguments:

```bash
runx acme/tools@main scripts/release.sh        # .sh extension → shell
runx acme/infra@main bootstrap                  # bare word → make target
runx acme/infra@main                            # no target → default make target
```

### Subcommands

| Subcommand | What it runs | Needs on host |
|------------|-------------|---------------|
| `sh` | A shell script from the repo | a shell (`sh`, `bash`, ...) |
| `make` | A Makefile target | `make` |
| `docker` | Docker CLI with repo as build context | Docker daemon |
| `compose` | Docker Compose from the repo | Docker daemon + Compose |
| `bin` | A GitHub release binary | network access |

### Flags

| Flag | Description |
|------|-------------|
| `--refresh` | Re-fetch from remote even if a cached ref exists |
| `--offline` | Disable network access; fail if ref is not cached |
| `--cache-dir <dir>` | Override cache root (default: `~/.cache/runx`) |
| `--yes` | Skip interactive trust prompt (this run only) |
| `--trust` | Permanently trust this repo (implies `--yes`) |
| `--pin` | Print the pinned-commit command after resolving a mutable ref |
| `--commit <sha>` | Override ref resolution with a specific commit SHA |
| `--verbose` | Print verbose output |

### Repo reference syntax

| Format | Example |
|--------|---------|
| `owner/repo` | `acme/tools` |
| `owner/repo@branch` | `acme/tools@main` |
| `owner/repo@tag` | `acme/tools@v1.2.0` |
| `owner/repo@commit` | `acme/tools@a1b2c3d` |
| Full HTTPS URL | `https://github.com/acme/tools@main` |
| SSH URL | `git@github.com:acme/tools.git@v1.2.0` |

`owner/repo` shorthand maps to GitHub. For other hosts, use a full URL.

---

## Developing

### Prerequisites

- [Nix](https://nixos.org/) with flakes enabled
- [direnv](https://direnv.net/) (recommended)

The dev shell provides Rust (cargo, rustc, clippy, rustfmt), GNU Make, and git.
On macOS it also supplies `libiconv` for linking.

### Setup

```bash
git clone https://github.com/bswr/runx
cd runx
direnv allow          # or: nix develop
```

### Build

```bash
make build            # release binary → ./bin/runx
```

### Tests

```bash
make unit-tests       # run all unit + integration tests (cargo test)
make lint             # cargo clippy
make fmt              # cargo fmt --check
make smoke-tests      # end-to-end tests against real GitHub repos (requires network + git)
```

### Install

```bash
make install          # copies bin/runx to /usr/local/bin
```

---

## Caching

```
~/.cache/runx/
  repos/<urlhash>/        # bare git repos (one per remote)
  trees/<urlhash>/<sha>/  # immutable materialized checkouts
  trust.json              # permanently trusted repos
```

1. **Ref resolution** — branches/tags resolve to a commit SHA. The SHA is always printed.
2. **Bare repo cache** — `git clone --bare` on first use, `git fetch` on subsequent mutable-ref runs. Immutable refs skip the fetch.
3. **Ref TTL cache** — resolved `(ref -> commit)` mappings cached for 5 minutes per bare repo.
4. **Immutable tree** — `git archive | tar -x` into `trees/<urlhash>/<sha>/`. Never written to.
5. **Mutable workspace** — tools that write files (`sh`, `compose`, `make`) get a `cp -r` copy in a temp dir, deleted on exit.

### Warm-cache behavior

```bash
# First run: clones + materializes tree
runx make acme/infra@main bootstrap    # ~3s

# Second run (same commit, within TTL): zero network
runx make acme/infra@main bootstrap    # ~0.1s
```

### Pinning

```bash
runx --pin make acme/tools@main deploy
#   runx: main -> a1b2c3d4e5f6
#   runx: pin -> acme/tools@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2
```

---

## Security

**Running arbitrary code from the internet is dangerous. runx does not pretend otherwise.**

- Shows a trust prompt the first time you run from a new repository
- Always displays the resolved commit SHA
- Warns on mutable refs (branches/tags)
- `--yes` skips the prompt once; `--trust` permanently records trust

runx does **not** sandbox execution, verify signatures, or audit script content.

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUNX_CACHE_DIR` | `~/.cache/runx` | Override the cache root |

---

## License

MIT
