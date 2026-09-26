# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Before You Start

**Read these first, in order:**
1. [`AGENTS.md`](AGENTS.md) — rules and identity for any agent (human or code) working here.
2. [`PLAN.md`](PLAN.md) — phases, steps, protocol, and what comes next.
3. [`docs/progress/STATUS.md`](docs/progress/STATUS.md) — where we are now and what's blocked.

**One thing per step.** Commit small, verify with tests, update STATUS and the phase bitácora when done. Never work on `main` — use `phase/pNN-...` branches.

---

## Quick Commands

```bash
# Build and test everything
cargo xtask check          # format + clippy + nextest (required before commit)

# Run tests
cargo nextest run --workspace
cargo nextest run -p symphony-cli --test agents    # single test file
cargo nextest run -p symphony-tui::views            # snapshot tests

# Run the binaries
cargo run -p symphony-cli -- --version
cargo run -p symphony-daemon --bin symphonyd

# Format and lint
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings

# Update snapshots (TUI views)
INSTA_UPDATE=always cargo nextest run -p symphony-tui

# Generate CHANGELOG
git cliff -o CHANGELOG.md    # after release tags

# Build release binary
cargo build -p symphony-daemon -p symphony-cli --release
```

---

## Architecture at a Glance

**Core principle:** Agent ≠ Model. Agents persist in SQLite; models can swap.

### Crate map

| Crate | Purpose | Key files |
|-------|---------|-----------|
| **protocol** | IPC contract (framed JSON, versioned) | `src/lib.rs` → `Request`, `Response`, `Subscribe` |
| **core** | Domain types (states, enums, IDs) | `src/lib.rs` → `AgentState`, `TaskStatus`, ulid newtype |
| **store** | SQLite writer, repos, object store | `src/writer.rs` (single writer), `src/repo/` (entidades), `src/object_store.rs` |
| **daemon** | Runtime: event bus, adapters, scheduling | `src/runtime.rs` (agent creation/state), `src/providers.rs` (detect/list), `src/main.rs` |
| **cli** | Terminal UI (v0.1 TUI, v0 CLI cmds) + IPC client | `src/main.rs` (commands), `tests/` (fixtures) |
| **tui** | Ratatui app (views, state, I/O, daemon loop) | `src/app.rs` (state machine), `src/ui.rs` (render), `src/lib.rs` (terminal loop) |
| **adapters/claude, codex, common** | Provider detection, spawning, event parsing | `src/lib.rs` impl `ProviderAdapter` trait |
| **git** | Git CLI wrapper: worktrees, status, diff | `src/lib.rs` → safe porcelain calls |
| **process** | ProcessSupervisor: spawn, kill-tree, stream | `src/lib.rs` → trait + OS impl (ProcessKit or nix/windows-sys) |
| **object-store** | Content-addressed storage (BLAKE3 + zstd) | `src/lib.rs` → `put()`, `get()`, atomic writes |
| **testkit** | `fake-agent` CLI for testing without real providers | `src/bin/fake_agent.rs` → responds to `--version`, streams work |
| **context** | (stub for P09, not yet in use) | — |

### Data flow

```
symphony (CLI/TUI) 
  ↓ IPC (named pipe / unix socket, JSON, protocol v1)
symphonyd (daemon)
  ├─ Event bus ← hooks from each provider (agent/run/task events)
  ├─ Store writer ← persists events to SQLite, blobs to object-store
  ├─ Runtime ← creates agents, spawns adapters, emits IPC replies
  ├─ Adapters (Claude, Codex, later: Kimi/Antigravity/Copilot)
  └─ Git (worktrees per agent, status, diff, merge preflight)
```

### Key invariants (PLAN §2)

- **One SQLite writer thread** — all other callers go through daemon IPC, never directly.
- **One worktree per agent** — no two agents share a directory.
- **Checkpoint before failure** — recovery items are written continuously, not on crash.
- **Deterministic routing** — rules, parsers, scheduler are not LLM; no ML in the critical path.
- **No unwrap/expect/panic in runtime paths** — fail with `Result` and error codes (`thiserror`).
- **Prohibitions:** no Node/Python runtime, no PostgreSQL, no gRPC, no embeddings v1, no telemetry.

---

## Testing

| Type | How | When |
|------|-----|------|
| Unit | `cargo nextest run -p <crate>` | Every crate with logic |
| Snapshot (TUI) | `INSTA_UPDATE=always cargo nextest run -p symphony-tui` | Every view change |
| Integration | `cargo nextest run -p symphony-cli --test <name>` | E2E flows, provider detection |
| Property-based | `proptest` in unit tests | State machines, framing, routing |
| E2E (live) | `SYMPHONY_LIVE=1 cargo nextest run` | **with Leo's permission only** — uses real CLIs and cuota |

**CI never spends cuota.** L1 (fixtures), L2 (`fake-agent`), L3 (live with permission).

---

## State Machine & Lifecycle

**Agent states** (DB `agents.state`): CREATED → READY → RUNNING → STOPPED / FAILED / INTERRUPTED / RECOVERED

**Run states** (DB `agent_runs.status`): QUEUED → STARTED → ENDED / FAILED / INTERRUPTED

**Transitions** are pure functions in `core::transitions::*`. Never create invalid state directly.

**Recovery**: interrupted sessions → `INTERRUPTED` + `recovery_items` (FLOW Journey E).

---

## Workflow (per phase step)

1. **Read the step** in PLAN (what, why, docs, verifica).
2. **Check STATUS.md** — has the spec changed? Are there blockers?
3. **Run `cargo xtask check`** before you touch code — if it fails and STATUS doesn't report it, fix that first.
4. **Work on `phase/pNN-*` branch** (never `main`).
5. **Commit small** — one logical change per commit, `Conventional Commits` + `[PNN.SX]` step tag.
6. **Verify** with the step's test command (`Verifica` section).
7. **Update the bitácora** (`docs/phases/PNN-*.md`) — what you did, files changed, blockers, next action.
8. **Update STATUS.md** — mark step done, point to next one.
9. **`git push` only the phase branch** — merge to main only at phase close (PLAN §4.6, Leo's approval).

---

## Important Files & Decisions

| File | Why it matters |
|------|---|
| `PLAN.md` | Source of truth for what comes next, order, protocol. Changes go via ADR. |
| `docs/progress/STATUS.md` | Current phase + step. Read before any work. |
| `docs/adr/*.md` | Decisions that changed spec, architecture, or order. Precedence over PLAN. |
| `CONSTRAINTS.md` | Hard limits: safety, performance, dependencies, no `unsafe`. Verified in CI. |
| `docs/spec/symphony_database.md` | Schema: 42 tables, enums, indices. Don't commit migrations without it. |
| `Cargo.toml` [workspace.dependencies] | Approved crates. Add new ones only with STACK §58 ref or an ADR. |
| `.github/workflows/ci.yml` | Runs `cargo xtask check` + `cargo deny` on ubuntu/windows/macos, MSRV 1.95. |

---

## Common Pitfalls

- **Unwrap in runtime code:** Use `Result<T, E>` + `thiserror`. CI rejects `unwrap_used`.
- **Adding deps without STACK reference:** Check `docs/spec/Symphony_CLI_Ideal_Technology_Stack.md` §58. If not there, justify in the bitácora or write an ADR.
- **Forgetting to update snapshots:** TUI tests use `insta`. Run `INSTA_UPDATE=always cargo nextest run -p symphony-tui` when views change, review diffs before commit.
- **Two agents in one worktree:** Never. Each agent gets its own isolated Git worktree.
- **Querying SQLite directly from adapters:** Go through daemon IPC. The writer is single-threaded; concurrent reads are fine, but never write outside it.
- **Letting tests leave orphaned daemons:** If `symphonyd` hangs after a test, the next build fails with "access denied" on Windows. Kill the process or restart before retrying.

---

## Phase Roadmap (v0.1 done, now at S10)

- ✅ **P00–P07**: Core (daemon, SQLite, adapters, TUI, release)
- 🟡 **P07.S10**: Usage phase — Leo uses v0.1 for 2+ weeks, logs learnings → triggers ADR replan
- ⏳ **P08–P16**: Multi-agent scheduler, Context Engine, Failover, more providers, GUI (Tauri), v1.0

**P08+ are provisional** until we have usage data. No work on those phases until ADR-replan is approved.

---

## When to Ask Leo

- Before running live tests (`SYMPHONY_LIVE=1`) — costs his cuota.
- Before creating the remote repo, first push, or publishing releases — check with him.
- If a gate fails (P00.S0, P01, P07.S10, P08, P14) — these are decision points.
- If you find a contradiction between docs — the precedence is ADR > PLAN > DB > STACK > FLOW > IDEA.
- If a step is impossible as written — document it, don't guess.

---

## Quick Links

- Spec: [`docs/spec/idea.md`](docs/spec/idea.md) (vision), [`docs/spec/symphony_database.md`](docs/spec/symphony_database.md) (schema)
- Flow: [`docs/spec/Symphony_CLI_User_Flow_and_Views.html`](docs/spec/Symphony_CLI_User_Flow_and_Views.html) (35 views, journeys A–E)
- Stack: [`docs/spec/Symphony_CLI_Ideal_Technology_Stack.md`](docs/spec/Symphony_CLI_Ideal_Technology_Stack.md) (tech choices, prohibitions)
- Progress: [`docs/progress/STATUS.md`](docs/progress/STATUS.md), [`docs/progress/LEARNINGS.md`](docs/progress/LEARNINGS.md)
- ADRs: [`docs/adr/`](docs/adr/) — 0001 (crate versions), 0002 (process layer), 0003 (hooks hold), 0004 (handoff viability), 0005 (interaction mode)
