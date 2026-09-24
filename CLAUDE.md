# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Current state

This directory is **spec only**. There is no code, no git repo, and no Cargo workspace yet. Work starts at **PLAN.md P00.S0** (prevalidation: survey existing tools + manual Test D, needs Leo's permission), then P00.S1. P08–P16 are provisional until the P07.S10 replanning gate. The docs are in Spanish; the human owner is Leo.

`PLAN.md` is the source of truth for work order and protocol. On a first session, read §0–§7 before touching anything. P00.S4 replaces this file with a one-line redirect to `AGENTS.md` + `PLAN.md` (Appendix D has the `AGENTS.md` content). Once `docs/progress/STATUS.md` exists, it and git decide where work resumes, even if memory or context says otherwise.

## What Symphony is

A local Rust runtime that coordinates official AI coding CLIs (Claude Code, Codex, Antigravity, Kimi Code, Copilot) from one terminal. Its core idea: **AGENT ≠ MODEL**. An agent is persistent state (task, worktree, branch, checkpoints) owned by Symphony. The executor/model running it is replaceable, so a quota failure hands off to another provider without re-explaining the task.

Architecture (IDEA §5, STACKDIAG): `symphony` (CLI/TUI) → IPC (named pipe / unix socket, typed versioned JSON) → `symphonyd` daemon holding project state (SQLite + Git + object store), event bus (CLI hooks normalized to events), a two-layer scheduler (hook-level operation classes 0–4 + OS enforcement via Job Objects/cgroups), incremental checkpoints, a context engine, and provider adapters behind one `ProviderAdapter` trait. Adapters are CLI bridges: each official CLI owns its own auth.

## Spec documents and precedence

| Alias | File | Authority for |
|---|---|---|
| IDEA | `idea.md` | Vision, principles, product phases |
| FLOW | `Symphony_CLI_User_Flow_and_Views.html` | User-visible behavior: 35 views, states, copy |
| STACK | `Symphony_CLI_Ideal_Technology_Stack.md` | Crates, tech choices, prohibitions (§36), allowed deps (§58) |
| DB | `symphony_database.md` | 42 tables, enums, indexes, and the phase each one belongs to |
| ER / STACKDIAG | `*.html` diagrams | Visual only. ER is **not** authoritative, DB is |

Precedence when docs conflict: accepted ADRs (`docs/adr/`) > PLAN > DB > STACK > FLOW > IDEA. Never resolve a contradiction silently. Log it in the phase log under "Desviaciones del spec", and write an ADR if it matters. The HTML files are large: grep or extract from them instead of reading them whole.

Plan phases P00–P16 are not the same as the product "Fase 0–7" in IDEA/STACK/DB. The mapping is in PLAN §1.3 (P01 = Fase 0 spike, P02–P07 = Fase 1 core, …).

## Commands (from P00.S6 onward)

```bash
cargo xtask check                      # fmt --check + clippy -D warnings + nextest; run at session start and after each step
cargo nextest run -p <crate>           # one crate
cargo nextest run -p <crate> <filter>  # a single test
cargo run -p symphony-cli -- --version
cargo deny check
```

Test layers: `nextest` (unit), `proptest` (state machines/DAG/scheduler/IPC framing), `trycmd` (CLI), `insta` + `TestBackend` (TUI snapshots), and `fake-agent` from `crates/testkit` (simulated provider CLI, used by nearly all E2E). **CI never spends real subscriptions.** Live tests need `SYMPHONY_LIVE=1` and Leo's permission.

Workspace layout is STACK §4.1 (`crates/{protocol,core,daemon,cli,tui,store,process,scheduler,git,context,adapters/*,testkit,…}`, `xtask/`, `migrations/`, `fixtures/`). Create a crate only in the phase that uses it.

## Hard rules (PLAN §2)

- Never store "current model" on `agents`. It lives on the open `agent_run`.
- Single SQLite writer: hooks and external processes never open the DB. Everything goes through the daemon over IPC.
- One worktree per agent. Heavy operations go through the scheduler.
- Router, scheduler, dispatcher, and error parsers are deterministic. No LLM on the critical path. "Smart" features sit behind a flag and have a fallback.
- The core makes no HTTP calls and has no telemetry. Never read, copy, or extract provider credentials or OAuth tokens.
- Banned in the core: Node/Python runtime, Electron, Postgres, Redis, NATS/Kafka, required Docker, ORMs, gRPC, an HTTP server in v1, vector DB/embeddings in v1, an LLM orchestrator, in-process native plugins, and libgit2 as the main git backend (use the system `git` CLI).
- Errors: `thiserror` in libraries, `miette` in binaries. No `unwrap()`/`todo!()`/`unimplemented!()` in runtime paths, and no stubs committed as done.
- Any crate not in STACK §58 must be justified in the phase log. Heavy ones (runtime, network, ML) need an ADR.
- Never delete or weaken a test to make it pass.

## Workflow protocol

- Work one plan step at a time on `phase/pNN-<name>` branches, never on `main`. A phase merges to `main` with a merge commit (no squash) and tag `pNN-done`.
- Commits use Conventional Commits with the step in the title and trailers:
  `feat(scheduler): ... [P08.S3]` + `Plan-Step: P08.S3`, `Agent: claude-code/<model>`, `Tests: <exact command + result>`.
- After each step: run the step's **Verifica** check, update the phase log `docs/phases/PNN-*.md` and `docs/progress/STATUS.md`, then commit. Log sessions in `SESSIONS.md` and add non-obvious findings to `LEARNINGS.md` right away.
- If a session is about to stop mid-step: make a `wip(...)` commit on the phase branch, noting if it doesn't compile, and fill in "Handoff para el siguiente agente" in STATUS.
- **Ask Leo before:** running a real provider CLI (it uses quota), creating or pushing the remote, publishing a release, when a gate fails (P00.S0, P01, P07.S10, P08, P14), when docs contradict each other beyond the precedence rule, or when installing system-level software or changing global CLI config.
