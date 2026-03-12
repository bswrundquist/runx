# runx

Execute shell scripts, Makefile targets, Docker commands, and Docker Compose
**directly from Git repositories** — no manual clone, no setup.

Think of it as `npx`/`uvx` for git repos.

```
shx     acme/tools@main scripts/release.sh -- --dry-run
makex   acme/infra@main bootstrap
dockerx acme/app@v1.2.0 -- build -t acme/app:dev .
dcx     acme/platform@main -- up -d
```

---

## Why

Common dev workflow pain:

```bash
git clone git@github.com:acme/tools
cd tools
git checkout v1.2.0
chmod +x scripts/release.sh
./scripts/release.sh --dry-run
```

With `shx`:

```bash
shx acme/tools@v1.2.0 scripts/release.sh -- --dry-run
```

Same result. No residual clone. Warm-cache repeat runs skip the network entirely.

---

## Install

### From source (requires Go 1.22+)

```bash
git clone https://github.com/bswr/runx
cd runx
make build
make install          # copies to /usr/local/bin
```

### Single umbrella binary

```bash
make build
cp bin/runx ~/bin/
runx sh acme/tools@main scripts/release.sh
```

Or install all thin aliases (`shx`, `dockerx`, `dcx`, `makex`) for muscle memory.

---

## Which tool to use?

Prefer the simplest tool that gets the job done. The four tools are ordered by
increasing complexity — both in what they require on the host and in what they
assume about the repo:

| Tool | Needs | Use when the repo has |
|------|-------|-----------------------|
| `shx` | a shell (`sh`, `bash`, …) | a standalone script |
| `makex` | `make` | a `Makefile` with named targets |
| `dockerx` | Docker daemon | a `Dockerfile` / `docker buildx` workflow |
| `dcx` | Docker daemon + Compose | a `compose.yml` / multi-service stack |

**Start with `shx`.** If the repo already has a script that does what you
want, run it directly — no extra tooling required.

**Reach for `makex` next.** A `Makefile` is still just shell commands, but it
gives you named targets, dependency ordering, and parallelism without pulling
in a container runtime.

**Use `dockerx` when isolation matters.** The Docker daemon is a real
dependency; only use it when you need the build-context reproducibility or
image-layer caching that Docker provides.

**Use `dcx` last.** Compose adds service orchestration, networking, and
volumes on top of Docker. It is the most powerful option and the hardest to
reason about — save it for multi-service workflows that genuinely need it.

---

## Tools

### `shx` — run a shell script

```
shx [flags] <repo[@ref]> <script-path> [-- [script args...]]
```

```bash
shx acme/tools@main scripts/release.sh
shx acme/tools@1a2b3c4 scripts/bootstrap.sh -- --dry-run
shx https://github.com/acme/tools@v1.2.0 scripts/test.sh -- -v
```

- Runs the script from the repo root
- Respects the script's shebang line
- Fixes missing execute bits automatically
- Always uses a mutable workspace (scripts often write temp files)

---

### `makex` — run make targets

```
makex [flags] <repo[@ref]> [target] [-- [make args...]]
```

```bash
makex acme/infra@main bootstrap
makex acme/infra@v1.2.0 test -- -j4
makex acme/infra@main -- -f build/Makefile deploy
makex acme/monorepo@main             # runs default make target
makex acme/infra@main build -- FOO=bar JOBS=8
```

- Target is optional; omitting it runs the default make target
- Additional make flags and variables go after `--`
- Uses `-f` for explicit Makefile path via the passthrough
- Always uses a mutable workspace (make writes build artifacts)

---

### `dockerx` — run docker commands

```
dockerx [flags] <repo[@ref]> -- <docker args...>
```

```bash
dockerx acme/app@main -- build -t acme/app:dev .
dockerx acme/app@v1.2.0 -- run --rm app:test
dockerx acme/app@main -- buildx bake
```

- Sets `cwd` to repo root before invoking `docker`
- Passes all args after `--` straight through to the docker CLI
- Uses immutable checkout (docker reads the build context; it doesn't write to it)

---

### `dcx` — run docker compose

```
dcx [flags] <repo[@ref]> -- <compose args...>
```

```bash
dcx acme/platform@main -- up -d
dcx acme/platform@9f8e7d6 -- logs api
dcx acme/platform@main -- -f docker-compose.prod.yml up
dcx acme/platform@main -- down --volumes
```

- Runs `docker compose` from the repo root
- Auto-detects compose files: `compose.yml`, `compose.yaml`, `docker-compose.yml`, `docker-compose.yaml`
- Always uses a mutable workspace (compose creates bind mounts, env files, overrides)

---

## Repo Reference Syntax

| Format | Example |
|--------|---------|
| `owner/repo` | `acme/tools` |
| `owner/repo@branch` | `acme/tools@main` |
| `owner/repo@tag` | `acme/tools@v1.2.0` |
| `owner/repo@commit` | `acme/tools@a1b2c3d` |
| Full HTTPS URL | `https://github.com/acme/tools@main` |
| SSH URL | `git@github.com:acme/tools.git@v1.2.0` |
| GitLab / self-hosted | `git@gitlab.com:group/project.git@main` |

`owner/repo` shorthand always maps to GitHub. For other hosts, use a full URL.

---

## Shared Flags (all tools)

| Flag | Description |
|------|-------------|
| `--refresh` | Re-fetch from remote even if a cached ref exists |
| `--offline` | Disable network access; fail if ref is not cached |
| `--cache-dir <dir>` | Override cache root (default: `~/.cache/runx`) |
| `--yes` | Skip interactive trust prompt (this run only) |
| `--trust` | Permanently trust this repo (saves to trust store) |
| `--pin` | Print the pinned-commit command after resolving a mutable ref |
| `--commit <sha>` | Override ref resolution with a specific commit SHA |
| `--verbose` | Print verbose git and tool output |

---

## Caching

```
~/.cache/runx/
  repos/<urlhash>/        # bare git repos (one per remote)
  trees/<urlhash>/<sha>/  # immutable materialized checkouts
  trust.json              # permanently trusted repos
```

### How it works

1. **Ref resolution** — branches and tags resolve to a commit SHA before execution. The SHA is always shown so you know exactly what you're running.
2. **Bare repo cache** — `git clone --bare` on first use. `git fetch` on subsequent mutable-ref runs. Immutable refs (commit SHAs) skip the fetch.
3. **Ref TTL cache** — resolved `(ref → commit)` mappings are cached for 5 minutes per bare repo. Repeat invocations of the same branch command skip the network during the TTL window.
4. **Immutable tree** — `git archive | tar -x` extracts the commit tree into `trees/<urlhash>/<sha>/`. This directory is never written to.
5. **Mutable workspace** — tools that write files (`shx`, `dcx`, `makex`) get a `cp -r` copy of the immutable tree in a temp dir. Deleted on exit.

### Warm-cache behavior

```bash
# First run: clones bare repo + materializes tree
makex acme/infra@main bootstrap    # ~3s

# Second run (same commit, within TTL): zero network, zero git ops
makex acme/infra@main bootstrap    # ~0.1s
```

### Pinning for reproducibility

```bash
# Mutable ref: always resolves to latest
shx acme/tools@main scripts/deploy.sh

# runx shows the resolved commit:
#   runx: main → a1b2c3d4e5f6

# Pin it with --pin:
shx --pin acme/tools@main scripts/deploy.sh
#   runx: main → a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2
#   runx: pin → https://github.com/acme/tools@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2

# Use the pinned form in CI:
shx acme/tools@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2 scripts/deploy.sh
```

---

## Security and Trust Model

**Running arbitrary code from the internet is dangerous. runx does not pretend otherwise.**

### What runx does

- Shows a trust prompt the first time you run from a new repository
- Always displays the resolved commit SHA so you see exactly what you're running
- Warns loudly when running from mutable refs (branches/tags)
- Stores permanently trusted repos in `~/.cache/runx/trust.json`
- Keeps immutable cached checkouts separate from mutable workspaces

### What runx does NOT do

- Sandbox execution (no seccomp, no container, no network isolation)
- Verify signatures or provenance
- Audit script content

### Trust prompt

```
runx: ⚠️  executing code from an untrusted repository

  Repository: https://github.com/acme/tools
  Ref:        main → a1b2c3d4e5f6

  WARNING: 'main' is a mutable ref. Pin to a commit SHA for reproducibility:
           https://github.com/acme/tools@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2

  Type 'yes' to proceed once, 'trust' to remember, or Ctrl-C to abort:
```

- `yes` — proceed this run only
- `trust` — proceed and permanently remember this repo
- `Ctrl-C` — abort

### Flags for automation

```bash
shx --yes   acme/tools@main scripts/deploy.sh   # skip prompt once
shx --trust acme/tools@main scripts/deploy.sh   # skip and permanently trust
```

---

## Umbrella Binary

`runx` dispatches to all tools via subcommands:

```bash
runx sh      acme/tools@main scripts/release.sh
runx make    acme/infra@main bootstrap
runx docker  acme/app@main -- build .
runx compose acme/platform@main -- up -d
```

The separate binaries (`shx`, `dockerx`, `dcx`, `makex`) are thin wrappers around the same logic. Install whichever you prefer.

---

## Naming

### Primary names (chosen)

| Binary | Alternatives considered |
|--------|------------------------|
| `shx` | `gitsh`, `rxsh`, `shelx`, `runsh` |
| `dockerx` | `dockx`, `gitdocker`, `rxdocker` |
| `dcx` | `composex`, `gitdc`, `rxcompose` |
| `makex` | `mkx`, `gitmake`, `rxmake` |
| `runx` | `gitx`, `xrun`, `gitrun` |

**Why these names?**
- Short and memorable
- The `x` suffix signals "execute from a remote source" (like `npx`, `uvx`)
- The prefix is the underlying tool (`sh`, `docker`, `dc`, `make`)
- `shx` over `shellx`: one less syllable, easier to type

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUNX_CACHE_DIR` | `~/.cache/runx` | Override the cache root |

---

## Architecture

```
runx/
  cmd/
    runx/main.go     umbrella binary — dispatches subcommands
    shx/main.go      thin wrapper → tools.RunSHX
    dockerx/main.go  thin wrapper → tools.RunDockerX
    dcx/main.go      thin wrapper → tools.RunDCX
    makex/main.go    thin wrapper → tools.RunMakeX
  internal/
    core/
      repo.go        RepoRef: parse owner/repo[@ref], SSH/HTTPS URLs
      flags.go       Flags struct, AddFlags, SplitOnDash
      cache.go       Cache: directory layout, URLHash, tree existence
      git.go         Git: clone, fetch, ResolveRef, Archive; TTL ref cache
      workspace.go   ImmutableWorkspace, MutableWorkspace, copyDir
      exec.go        RunChild: signal forwarding, exit code propagation
      trust.go       TrustStore: first-run prompts, trust.json
      engine.go      Engine.Prepare: orchestrates everything
    tools/
      shx.go         RunSHX
      dockerx.go     RunDockerX
      dcx.go         RunDCX
      makex.go       RunMakeX
```

**Dependencies:** zero external packages. Stdlib only.

**External tools required:** `git`, `tar` (for archive materialization). Plus `docker`, `make` as needed per tool.

---

## Roadmap

- [ ] **`runx.yaml` manifest** — named entrypoints in a repo (`runx list acme/tools`)
- [ ] **Cache eviction** — LRU or size-based pruning (`runx cache clean`)
- [ ] **Parallel materialization** — archive + copy concurrently
- [ ] **Windows support** — replace `tar` with Go native extraction, adjust process groups
- [ ] **Shallow clones** — `--depth` for large repos with optional deepening
- [ ] **File locking** — safe concurrent invocations of the same repo
- [ ] **`makex --subdir`** — run make from a subdirectory for monorepos
- [ ] **Checksum verification** — optional pinned SHA verification against a lockfile

---

## License

MIT
