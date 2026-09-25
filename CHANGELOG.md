# Changelog

Cambios de Symphony CLI por versión. Se genera con `git cliff` (config en `cliff.toml`).

## [0.1.0] - 2026-09-25

### Funcionalidades

- *(protocol)* Length-framed versioned JSON IPC codec [P02.S1]
- *(core)* ULID ids, DB enums and pure state transitions [P02.S2]
- *(core)* Global and project config with comment-preserving edits [P02.S3]
- *(core,daemon)* Central secret redactor and redacting rolling logs [P02.S4]
- *(daemon)* Single-instance symphonyd with user-only IPC [P02.S5]
- *(cli)* Symphony status and daemon start|stop|status with autostart [P02.S6]
- *(store)* Migration 001 with the 19 phase-1 tables [P03.S1]
- *(store)* Single writer thread with batched events and read-only readers [P03.S2]
- *(store)* Hand-written repositories for the phase-1 lifecycle core [P03.S3]
- *(object-store)* Content-addressed blobs with BLAKE3, zstd and atomic writes [P03.S4]
- *(daemon)* Open the store and recover interrupted sessions at startup [P03.S5]
- *(git)* Git CLI wrapper for worktrees, porcelain status, diff and merge preflight [P04.S1]
- *(git)* Per-worktree dependency strategy; never delete through links [P04.S2]
- *(process)* Contained process groups with streaming, stdin, stats and tree kill [P04.S3]
- *(core)* ANSI sanitizer for CLI output (plain and color-only modes) [P04.S4]
- *(testkit)* Fake-agent provider CLI simulator driven by TOML scripts [P04.S5]
- *(adapters)* ProviderAdapter contract, canonical events and fake adapter [P05.S1]
- *(daemon)* Event bus with persistence, broadcast and per-agent state [P05.S2]
- *(cli,daemon)* Symphony hook emit feeds CLI hooks into the event bus [P05.S3]
- *(adapters)* Claude Code adapter with per-invocation hooks and stream-json IO [P05.S4]
- *(adapters)* Codex adapter with -c hook overrides and rollout quota [P05.S5]
- *(daemon,cli)* Provider registry and symphony providers [P05.S6]
- *(daemon)* Agent creation flow with rollback and executor launch [P06.S1]
- *(daemon)* Mirror conversation and tool calls from canonical events [P06.S2]
- *(daemon)* Incremental checkpoints from significant events [P06.S3]
- *(context)* Handoff assembler v1 from checkpoint + live git [P06.S4]
- *(daemon)* Replace executors without losing agent state [P06.S5]
- *(daemon)* Heartbeat watchdog with reclaim and restart [P06.S6]
- *(cli)* Agent control subcommands over daemon IPC [P06.S7]
- *(daemon)* Event subscription and read methods for the TUI [P07.S1]
- *(tui)* Terminal UI over daemon IPC with views 01-09, 12, 13, 22, 24, 30 [P07.S6]
- *(attach)* Open the agent's session in the official CLI [P07.S5]

### Correcciones

- *(protocol)* Skip socket fchmod on macOS, rely on the private 0700 dir [P02.S6]
- *(cli)* Wait for the instance lock, not the socket, when stopping or autostarting [P03.S5]
- *(cli)* Report a daemon that dies mid-call as stopped once its lock is free [P03.S5]
- *(process)* Count the whole tree in stats when the OS group does not [P04.S3]
- *(object-store)* Grace-period comparisons are inclusive [P04.S3]
- *(protocol)* Only talk to the daemon that owns this home [P05.S7]
- *(adapters)* Hook security review fixes [P05.S7]
- *(protocol)* Verify the peer uid when the OS gives no pid (macOS) [P05.S7]
- *(daemon)* An unrequested signal death is a crash, not a kill [P06.S1]
- *(runtime)* Claude turns end, and slow CLIs are not taken for hung [P07.S7]
- *(windows)* Daemon console, long paths and uncancellable agent creation [P07.S7]

### Rendimiento

- *(store)* Criterion baseline for event inserts and the Home query [P03.S6]

### Pruebas

- *(spike)* Test A resource measurements for Claude Code and Codex [P01.S3]
- *(spike)* Test B hook event bus for Claude Code and Codex [P01.S4]
- *(spike)* Test C command holding via PreToolUse [P01.S5]
- *(spike)* ProcessKit gate spike and 3-OS workflow [P01.S7]
- *(spike)* ProcessKit gate reports unsupported limits instead of aborting [P01.S7]
- *(spike)* Test D forced handoff, 6/6 pass [P01.S6]
- *(cli)* E2E daemon restart after a hard kill [P02.S7]
- *(testkit)* Worktree + fake-agent + stream + tree kill + cleanup [P04.S6]
- *(cli)* Live L3 sessions for Claude Code and Codex, gated by SYMPHONY_LIVE [P05.S7]
- *(daemon)* Forced kill acceptance test (Test D) [P06.S8]
- *(tui)* Journey A live with Claude Code, and agent view fixes [P07.S7]

### Build y CI

- Cargo workspace skeleton with xtask check [P00.S6]
- GitHub Actions matrix, MSRV and cargo-deny jobs [P00.S8]
- *(deny)* Allow 0BSD license used by interprocess dependencies [P02.S5]
