# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Before You Start

**Read these first, in order:**
1. [`AGENTS.md`](AGENTS.md) — rules and identity for any agent (human or code) working here.
2. [`PLAN.md`](PLAN.md) — phases, steps, protocol, and what comes next.
3. [`docs/progress/STATUS.md`](docs/progress/STATUS.md) — current phase/step, blockers, handoff notes. Its header is the source of truth; the long "Próxima acción" list below it can lag behind.

**One thing per step.** Commit small, verify with tests, update STATUS and the phase bitácora (`docs/phases/PNN-*.md`) when done. Never work on `main` — use `phase/pNN-...` branches.

Project docs, bitácoras, ADRs and commit messages are written in **Spanish**; keep that.

For a quick map of the code (crate deps, which file queries which table, IPC methods, migrated vs spec-only tables) read `graphify-smart-out/GRAPH_REPORT.md` (gitignored; rebuild with `python graphify-smart-out/build.py <trigger>`).

---

## Quick Commands

```bash
# Gate before every commit: fmt --check + clippy -D warnings + nextest (xtask/src/main.rs)
cargo xtask check

# Tests
cargo nextest run --workspace
cargo nextest run -p symphony-cli --test agents                 # one test file
cargo nextest run -p symphony-daemon --test runtime handoff_    # filter by test name
cargo nextest run -p symphony-tui --test views                  # TUI snapshot tests
INSTA_UPDATE=always cargo nextest run -p symphony-tui           # accept snapshot changes (review the diff!)
SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --test live_handoff   # L3: real CLIs, spends quota — ask Leo first

# Benchmarks (criterion)
cargo bench -p symphony-context --bench compress                # also: -p symphony-router --bench router, -p symphony-store --bench store

# Run
cargo run -p symphony-cli -- --version     # binary: symphony
cargo run -p symphony-daemon --bin symphonyd

# Install locally (symphony finds symphonyd next to its own exe)
symphony daemon stop                        # Windows: otherwise install fails with "Acceso denegado"
cargo install --path crates/cli --locked --force
cargo install --path crates/daemon --locked --force

cargo deny check                            # licenses/advisories (CI runs it)
git cliff -o CHANGELOG.md                   # after release tags
```

MSRV is 1.95 (`Cargo.toml` `rust-version`); CI runs `cargo xtask check` + `cargo deny` on ubuntu/windows/macos plus an MSRV job.

---

## Architecture

**Core principle:** Agent ≠ Model. An agent = task + worktree + branch + checkpoints, persisted by Symphony. The model running it is just the current `agent_runs` row and can be swapped (failover, user switch) without losing work.

### Process model

```
symphony (crates/cli: clap commands; no args → TUI from crates/tui)
  │ IPC: length-prefixed JSON frames, protocol v1 (crates/protocol), named pipe / unix socket
  ▼
symphonyd (crates/daemon) — one per project (ADR-0018, state in <project>/.symphony; user-level config in ~/.symphony) — owns all project state
  ├─ server.rs     dispatch table of IPC methods ("agent.create", "chat.get", "fact.set", "hook.emit", …)
  ├─ runtime.rs    create agent: git worktree first, then ONE transaction (project, session, task,
  │                worktree, agent, initial checkpoint, run); rolls back the worktree on failure
  ├─ executor.rs   live provider CLI processes: spawn via crates/process, read stdout/stderr,
  │                hand lines to the adapter parser, stop/replace the run (failover / switch)
  ├─ bus.rs        event bus: persist first (store writer, batched) → then broadcast to TUI + watch
  ├─ recorder.rs   mirrors canonical events into messages / tool_calls (dedups hook vs stream)
  ├─ handoff.rs    last valid checkpoint + live git state → crates/context assembler
  ├─ routing.rs    calls crates/router (deterministic scoring) for failover and profiles
  ├─ scheduling.rs / resources.rs   crates/scheduler queue + slots; CPU/RAM sampling, OS limits (P08)
  ├─ plan.rs / dependencies.rs / batch.rs   phase plans, task DAG, delegated batches (P08)
  ├─ validation.rs / merge.rs   tiered validation gate; integration into symphony/chat (P08)
  └─ watcher.rs / context_index.rs   notify-based file watcher feeding ctx:// chunks (P09)
```

Provider CLIs also report through their hooks: the hook runs `symphony hook emit`, which reaches the daemon as the `hook.emit` IPC method.

### Crates

| Crate (package) | Role |
|---|---|
| `protocol` | IPC framing + typed, versioned `Request`/`Response`. Transport is chosen by the caller. |
| `core` | Domain enums (`enums.rs`, checked against the DB spec by a test), ULID ids, pure state transitions (`transitions.rs`), config, health state machine, secret redaction. No I/O. |
| `store` | SQLite: `writer.rs` is the single writer thread (bounded channel, batched transactions); `repo.rs`, `context.rs`, `health.rs` are the repositories. Migrations live in top-level `migrations/` (001 core, 002 health/routing, 003 context, 004 multi-agent, 005 validation). |
| `object-store` | Content-addressed blobs (BLAKE3 + zstd, atomic writes). |
| `git` | Git CLI wrapper: worktrees, status, diff. |
| `process` | Process supervisor: spawn, stream, kill-tree (ADR-0002). |
| `adapters/common` | `ProviderAdapter` trait + shared parsing. One crate per provider: `claude`, `codex`, `kimi`, `antigravity`, `copilot`, `opencode` (ADR-0012). |
| `router` | Deterministic model router: availability filter → weighted score per profile → explanation. No I/O, no LLM. |
| `scheduler` | Pure P08 logic: operation classifier (classes 0–4), queue with heavy slots and `explain`, task DAG, phase plans, validation tiers. |
| `context` | Context engine: handoff assembler (modes RAW/SAFE/BALANCED/AGGRESSIVE), `ctx://` URIs, chunking, deterministic compressors, tree-sitter AST levels L0–L5. |
| `daemon` | `symphonyd`; wires everything above. Biggest files: `executor.rs`, `server.rs`, `runtime.rs`. |
| `tui` | Ratatui app: `app.rs` state + pure logic, `ui.rs` render, `io.rs` IPC, `lib.rs` terminal loop. Default view is the Chat. |
| `cli` | `symphony` binary: subcommands (agents, providers, context, fact, daemon, …) and launches the TUI. |
| `testkit` | `fake-agent` binary (deterministic stand-in for provider CLIs) and test fixtures. |

The general chat (P07.5, ADR-0006) is an agent with a fixed worktree/branch `symphony/chat`; a finished turn goes `RUNNING → READY` instead of `COMPLETED`, with one commit per turn.

### Key invariants (PLAN §2)

- **One SQLite writer** (`store::writer`). The daemon reads SQLite directly for views; every write goes through the writer. Nothing outside the daemon touches the DB.
- **One worktree per agent** — no two agents share a directory.
- **Checkpoint continuously**, not on crash; recovery items feed the Recovery Center.
- **Deterministic routing/parsing/scheduling** — no LLM or ML in the critical path.
- **No `unwrap`/`expect`/`panic` in runtime code** — `Result` + `thiserror`; clippy denies `unwrap_used`.
- **Prohibitions:** no Node/Python runtime, no PostgreSQL, no gRPC, no embeddings in v1, no telemetry. See `CONSTRAINTS.md`.

### State machines

States are DB strings defined in `crates/core/src/enums.rs`; legal moves are the exact tables in `crates/core/src/transitions.rs` (tested). Never set a state without going through `transition()`.

- **Agent:** `CREATED, READY, RUNNING, WAITING_PROVIDER, WAITING_RESOURCE, WAITING_DEPENDENCY, TESTING, BLOCKED, PAUSED, COMPLETED, FAILED, CANCELLED`
- **Task:** `BACKLOG, READY, RUNNING, WAITING, BLOCKED, DONE, FAILED, CANCELLED`
- **Run** (`agent_runs`): `STARTING, RUNNING, EXITED, KILLED, FAILED, HANDED_OFF`, with an end reason (`COMPLETED, QUOTA_EXHAUSTED, RATE_LIMITED, AUTH_ERROR, CRASH, NO_HEARTBEAT, USER_SWITCH, USER_STOP, PHASE_DONE`; `PHASE_DONE` from ADR-0013).

Adding a state or transition changes the core → ADR first (like ADR-0009).

---

## Testing levels

| Level | What | Notes |
|---|---|---|
| L1 | Fixtures (recorded provider output in `fixtures/`) | parsers, adapters |
| L2 | `fake-agent` from `testkit` driving the real daemon | runtime, handoff, failover, chat |
| L3 | Real CLIs, `SYMPHONY_LIVE=1` (`live_*.rs` tests) | **only with Leo's permission** — spends quota |

CI never spends quota. Snapshots (`insta`) live in `crates/tui/tests/snapshots/` and `crates/context/src/snapshots/`. Property tests (`proptest`) cover state machines and framing.

If a test leaves an orphaned `symphonyd.exe`, the next build fails with "Acceso denegado" on Windows — kill it first.

---

## Workflow (per phase step)

1. Read the step in PLAN (what, why, docs, **Verifica**) and check STATUS for changes/blockers.
2. Run `cargo xtask check` before touching code; if it fails and STATUS doesn't say so, fix that first.
3. Work on `phase/pNN-*`; commit with Conventional Commits + step tag, e.g. `feat: [P09.S7] …`.
4. Verify with the step's command, then update the bitácora (what, files, blockers, next action), STATUS, and `docs/progress/SESSIONS.md`; lessons go to `docs/progress/LEARNINGS.md`.
5. Push only the phase branch. Phase close = code review, green CI, `merge --no-ff` to `main`, tag `pNN-done` (PLAN §4.6).

**Doc precedence when they disagree:** ADR > PLAN > DB spec > STACK > FLOW > IDEA. New dependencies need a STACK §58 reference or an ADR.

---

## Phase status (see STATUS.md for the live version)

- ✅ P00–P07 (v0.1.0), P07.5 (general chat), P11 (Kimi, Antigravity, Copilot, OpenCode), P10 (health, router, profiles, failover — v0.5.0), P09 (context engine), P08 (multi-agent runtime, ADR-0011/0013; live benchmark with 6 providers approved 2026-10-12).
- `/compact` between tasks (ADR-0013 D2) is **not implemented**: it needs a live measurement first.
- Next (ADR-0014): **P11.5 → P12 → P14 → P15 → P13 → P16**, run continuously; ⛔ items go to STATUS § «Para Leo» and work moves on. Merged phase branches are deleted; history lives in `pNN-done` tags.

---

## When to Ask Leo

- Before live tests (`SYMPHONY_LIVE=1`) — costs his quota.
- Before creating remotes, first pushes of new branches, or publishing releases.
- At gate steps, or when a step is impossible as written — document it, don't guess.
- When docs contradict each other beyond what the precedence rule resolves.

---

## Quick Links

- Spec: [`docs/spec/idea.md`](docs/spec/idea.md) (vision), [`docs/spec/symphony_database.md`](docs/spec/symphony_database.md) (schema), [`docs/spec/Symphony_CLI_User_Flow_and_Views.html`](docs/spec/Symphony_CLI_User_Flow_and_Views.html) (views, journeys A–F), [`docs/spec/Symphony_CLI_Ideal_Technology_Stack.md`](docs/spec/Symphony_CLI_Ideal_Technology_Stack.md) (stack, §58 deps)
- Usage: [`QUICKSTART.md`](QUICKSTART.md) (install, providers and their permission limits)
- ADRs: [`docs/adr/`](docs/adr/) — 0001–0014
