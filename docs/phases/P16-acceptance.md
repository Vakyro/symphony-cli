# P16.S1 · Checklist de aceptación

> Matriz de trazabilidad y verificación de aceptación de Symphony CLI (PLAN.md § P16.S1).
> Cada elemento cuenta con evidencia verificable existente en el repositorio (tests, comandos, snapshots, código fuente o vistas de GUI).
>
> **Convenciones de resultado:**
> - ✅ Cubierto: existe evidencia verificable en el repositorio.
> - 🟡 Parcial: funcionalidad parcialmente implementada o con diferencias documentadas respecto a la especificación inicial.
> - ❌ No existe: elemento especificado no implementado.
> - ⛔ Bloqueado / Necesita a Leo: requiere suscripción real o ejecución manual en vivo (`SYMPHONY_LIVE=1`).

---

## 1. Vistas de FLOW §18 (36 vistas)

Fuente: `docs/spec/Symphony_CLI_User_Flow_and_Views.html` § 18 (Vistas 00 a 35).

| Elemento | Evidencia | Resultado |
|---|---|---|
| **Vista 00: Chat (inicio)** | Snapshot: `crates/tui/tests/snapshots/views__view_00_chat_empty.snap`<br>Test: `crates/tui/tests/views.rs::view_00_chat_empty`<br>GUI: `apps/gui/src/views.tsx::Chat` | ✅ |
| **Vista 01: Launch / Boot** | Snapshots: `crates/tui/tests/snapshots/views__view_01_launch_checking.snap`, `views__view_01_launch_not_a_repo.snap`<br>Test: `crates/tui/tests/views.rs::view_01_launch_checking`<br>GUI: `apps/gui/src/views.tsx` (id 01) | ✅ |
| **Vista 02: First-run Setup** | Snapshot: `crates/tui/tests/snapshots/views__view_02_first_run.snap`<br>Test: `crates/tui/tests/views.rs::view_02_first_run`<br>GUI: `apps/gui/src/views.tsx::FirstRun` | ✅ |
| **Vista 03: Provider Setup** | Snapshot: `crates/tui/tests/snapshots/views__view_03_provider_setup.snap`<br>Test: `crates/tui/tests/views.rs::view_03_provider_setup`<br>GUI: `apps/gui/src/views.tsx::Providers` | ✅ |
| **Vista 04: Home / Dashboard** | Snapshots: `crates/tui/tests/snapshots/views__view_04_home_empty.snap`, `views__view_04_home_with_agents_and_a_problem.snap`<br>Test: `crates/tui/tests/views.rs::view_04_home_empty`<br>GUI: `apps/gui/src/views.tsx::Home` | ✅ |
| **Vista 05: New Agent** | Snapshot: `crates/tui/tests/snapshots/views__view_05_new_agent.snap`<br>Test: `crates/tui/tests/views.rs::view_05_new_agent`<br>GUI: `apps/gui/src/views.tsx::NewAgent` | ✅ |
| **Vista 06: Agent Overview** | Snapshot: `crates/tui/tests/snapshots/views__view_06_agent_overview.snap`<br>Test: `crates/tui/tests/views.rs::view_06_agent_overview`<br>GUI: `apps/gui/src/views.tsx` (id 06) | ✅ |
| **Vista 07: Agent Conversation** | Snapshot: `crates/tui/tests/snapshots/views__view_07_agent_conversation_with_executor_change.snap`<br>Test: `crates/tui/tests/views.rs::view_07_agent_conversation_with_executor_change`<br>GUI: `apps/gui/src/views.tsx` (id 07) | ✅ |
| **Vista 08: Agent Activity** | Snapshot: `crates/tui/tests/snapshots/views__view_08_agent_activity.snap`<br>Test: `crates/tui/tests/views.rs::view_08_agent_activity`<br>GUI: `apps/gui/src/views.tsx` (id 08) | ✅ |
| **Vista 09: Agent Changes** | Snapshot: `crates/tui/tests/snapshots/views__view_09_agent_changes.snap`<br>Test: `crates/tui/tests/views.rs::view_09_agent_changes`<br>GUI: `apps/gui/src/views.tsx` (id 09) | ✅ |
| **Vista 10: Agent Validation** | TUI: integrado en pestaña y pantalla de validación (`crates/tui/src/app.rs::Screen::Validation`)<br>Test: `crates/tui/tests/views.rs::view_28_validation_shows_each_tier_and_the_failure`<br>GUI: `apps/gui/src/views.tsx` (id 10) | ✅ |
| **Vista 11: Agent Context** | TUI: `Screen::ContextOverview` activado por agente (`crates/tui/src/app.rs:35`)<br>Test: `crates/tui/tests/views.rs::view_20_context_overview`<br>GUI: `apps/gui/src/views.tsx` (id 11) | ✅ |
| **Vista 12: Agent History** | Snapshot: `crates/tui/tests/snapshots/views__view_12_24_agent_history_and_failover_event.snap`<br>Test: `crates/tui/tests/views.rs::view_12_24_agent_history_and_failover_event`<br>GUI: `apps/gui/src/views.tsx` (id 12) | ✅ |
| **Vista 13: Model Picker** | Snapshot: `crates/tui/tests/snapshots/views__view_13_model_picker.snap`<br>Test: `crates/tui/tests/views.rs::view_13_model_picker`<br>GUI: `apps/gui/src/views.tsx::ModelPicker` | ✅ |
| **Vista 14: Explain Route** | Snapshot: `crates/tui/tests/snapshots/views__view_14_explain_route.snap`<br>Test: `crates/tui/tests/views.rs::view_14_explain_route`<br>GUI: `apps/gui/src/views.tsx` (id 14) | ✅ |
| **Vista 15: Task List** | Snapshot: `crates/tui/tests/snapshots/views__view_15_task_list.snap`<br>Test: `crates/tui/tests/views.rs::view_15_task_list_and_view_16_task_detail`<br>GUI: `apps/gui/src/views.tsx` (id 15) | ✅ |
| **Vista 16: Task Detail** | Snapshot: `crates/tui/tests/snapshots/views__view_16_task_detail.snap`<br>Test: `crates/tui/tests/views.rs::view_15_task_list_and_view_16_task_detail`<br>GUI: `apps/gui/src/views.tsx::TaskDetail` | ✅ |
| **Vista 17: Milestone Review** | Snapshot: `crates/tui/tests/snapshots/views__view_17_milestone_review.snap`<br>Test: `crates/tui/tests/views.rs::view_17_milestone_review_decides_only_what_the_user_asks`<br>GUI: `apps/gui/src/views.tsx` (id 17) | ✅ |
| **Vista 18: System / Resources** | Snapshot: `crates/tui/tests/snapshots/views__view_18_system.snap`<br>Test: `crates/tui/tests/views.rs::view_18_system_and_view_19_performance_profile`<br>GUI: `apps/gui/src/views.tsx::SystemView` | ✅ |
| **Vista 19: Performance Profile** | Snapshot: `crates/tui/tests/snapshots/views__view_19_performance_profile.snap`<br>Test: `crates/tui/tests/views.rs::view_18_system_and_view_19_performance_profile`<br>GUI: `apps/gui/src/views.tsx::SystemView` | ✅ |
| **Vista 20: Context Overview** | Snapshot: `crates/tui/tests/snapshots/views__view_20_context_overview.snap`<br>Test: `crates/tui/tests/views.rs::view_20_context_overview`<br>GUI: `apps/gui/src/views.tsx` (id 20) | ✅ |
| **Vista 21: Context Object** | Snapshot: `crates/tui/tests/snapshots/views__view_21_context_object_opens_from_an_item.snap`<br>Test: `crates/tui/tests/views.rs::view_21_context_object_opens_from_an_item`<br>GUI: `apps/gui/src/views.tsx::ContextSearch` | ✅ |
| **Vista 22: Providers** | Snapshot: `crates/tui/tests/snapshots/views__view_22_providers.snap`<br>Test: `crates/tui/tests/views.rs::view_22_providers`<br>GUI: `apps/gui/src/views.tsx::Providers` | ✅ |
| **Vista 23: Provider Detail** | Snapshot: `crates/tui/tests/snapshots/views__view_22_provider_detail_with_a_temporary_limit_and_estimated_quota.snap`<br>Test: `crates/tui/tests/views.rs::view_22_provider_detail_with_a_temporary_limit_and_estimated_quota`<br>GUI: `apps/gui/src/views.tsx::Providers(detail)` | ✅ |
| **Vista 24: Failover Event** | Snapshot: `crates/tui/tests/snapshots/views__view_12_24_agent_history_and_failover_event.snap`<br>Test: `crates/tui/tests/views.rs::view_12_24_agent_history_and_failover_event`<br>GUI: `apps/gui/src/views.tsx` (id 24) | ✅ |
| **Vista 25: Changes / Diff** | Test: `crates/tui/tests/views.rs::view_09_agent_changes`<br>Comando CLI: `symphony diff`<br>GUI: `apps/gui/src/views.tsx` (id 25) | ✅ |
| **Vista 26: Merge Preflight** | Snapshot: `crates/tui/tests/snapshots/views__view_26_merge_preflight.snap`<br>Test: `crates/tui/tests/views.rs::view_26_merge_preflight_and_its_actions`<br>GUI: `apps/gui/src/views.tsx` (id 26) | ✅ |
| **Vista 27: Conflict Review** | Snapshot: `crates/tui/tests/snapshots/views__view_27_conflict_review.snap`<br>Test: `crates/tui/tests/views.rs::view_27_conflict_review_and_view_29_review_result`<br>GUI: `apps/gui/src/views.tsx` (id 27) | ✅ |
| **Vista 28: Validation** | Snapshot: `crates/tui/tests/snapshots/views__view_28_validation.snap`<br>Test: `crates/tui/tests/views.rs::view_28_validation_shows_each_tier_and_the_failure`<br>GUI: `apps/gui/src/views.tsx` (id 28) | ✅ |
| **Vista 29: Review Agent Result** | Snapshot: `crates/tui/tests/snapshots/views__view_29_review_result.snap`<br>Test: `crates/tui/tests/views.rs::view_27_conflict_review_and_view_29_review_result`<br>GUI: `apps/gui/src/views.tsx` (id 29) | ✅ |
| **Vista 30: Recovery Center** | Snapshot: `crates/tui/tests/snapshots/views__view_30_recovery_center.snap`<br>Test: `crates/tui/tests/views.rs::view_30_recovery_center`<br>GUI: `apps/gui/src/views.tsx::Recovery` | ✅ |
| **Vista 31: Settings** | GUI: `apps/gui/src/views.tsx` (id 31: `Generic title="Proyecto y configuración" method="project.status"`)<br>TUI: pantalla `FirstRun` y archivos `config.toml`/`project.toml`. Falta pantalla interactiva independiente de Settings en TUI. | 🟡 |
| **Vista 32: Shared Skills** | Snapshot: `crates/tui/tests/snapshots/views__view_32_shared_skills_toggle.snap`<br>Test: `crates/tui/tests/views.rs::view_32_shared_skills_toggle`<br>GUI: `apps/gui/src/views.tsx` (id 32) | ✅ |
| **Vista 33: Shared MCP / Tools** | Snapshot: `crates/tui/tests/snapshots/views__view_33_shared_mcp_with_warnings.snap`<br>Test: `crates/tui/tests/views.rs::view_33_shared_mcp_with_warnings`<br>GUI: `apps/gui/src/views.tsx` (id 33) | ✅ |
| **Vista 34: Event / Audit Log** | TUI: `Tab::Activity` (`crates/tui/tests/views.rs::view_08_agent_activity`) y comando `symphony logs`<br>GUI: `apps/gui/src/views.tsx` (id 34: `agentView("Eventos", "agent.logs")`) | ✅ |
| **Vista 35: Usage / Context Stats** | Snapshot: `crates/tui/tests/snapshots/views__view_35_usage_and_context_stats.snap`<br>Test: `crates/tui/tests/views.rs::view_35_usage_and_context_stats`<br>GUI: `apps/gui/src/views.tsx::Usage` | ✅ |

---

## 2. Journeys de FLOW §20 (5 journeys)

Fuente: `docs/spec/Symphony_CLI_User_Flow_and_Views.html` § 20.

| Elemento | Evidencia | Resultado |
|---|---|---|
| **Journey A — Un solo agente, happy path** | Test L2: `crates/daemon/tests/runtime.rs::exact_model_creates_everything_and_runs_the_executor`<br>Test live: `crates/tui/tests/live.rs::live_journey_a_with_claude_code` (L3 con CLI real) | ✅ (L2)<br>⛔ (Live real requiere suscripción/Leo) |
| **Journey B — Tres agentes sin ralentizar la máquina** | Test L2: `crates/daemon/tests/benchmark.rs::main_benchmark_three_agents_and_one_heavy_test_at_a_time`<br>Test L2: `crates/daemon/tests/runtime.rs::a_batch_creates_agents_with_dependencies_and_integrates_what_finished` | ✅ |
| **Journey C — Se agota el proveedor a media tarea** | Test L2: `crates/daemon/tests/runtime.rs::quota_exhausted_hands_the_same_agent_to_the_next_provider`<br>Test L2: `crates/daemon/tests/runtime.rs::failover_any_goes_to_the_best_other_provider_and_records_why` | ✅ |
| **Journey D — Conflicto entre agentes** | Test L2: `crates/daemon/tests/runtime.rs::two_agents_on_the_same_file_conflict_and_the_user_can_abort_or_resolve`<br>Test L2: `crates/daemon/tests/runtime.rs::aborting_leaves_the_branch_untouched_and_closes_the_conflicts` | ✅ |
| **Journey E — Recovery tras cierre/crash** | Test L2: `crates/cli/tests/e2e.rs::cli_restarts_a_daemon_that_died_without_cleanup`<br>Test L2: `crates/daemon/tests/runtime.rs::crashed_executor_can_restart_from_its_checkpoint`<br>Test L2: `crates/daemon/tests/soak.rs::soak_with_two_restarts` | ✅ |

---

## 3. Reglas UX de FLOW §21 (10 reglas)

Fuente: `docs/spec/Symphony_CLI_User_Flow_and_Views.html` § 21.

| Elemento | Evidencia | Resultado |
|---|---|---|
| **Regla 1. Estado siempre explicable** | `crates/tui/src/app.rs::state_phrase` (describe el motivo de espera o bloqueo)<br>Test: `crates/tui/tests/views.rs::view_06_agent_waiting_for_an_executor`<br>Esquema DB: columna `state_reason` obligatoria en estados de espera en `agents` | ✅ |
| **Regla 2. Usuario manda sobre selección explícita** | Test: `crates/daemon/tests/runtime.rs::an_exact_model_is_obeyed_even_with_low_quota_and_the_choice_is_recorded`<br>Test: `crates/tui/tests/views.rs::an_unavailable_exact_model_is_never_substituted` | ✅ |
| **Regla 3. Failover no borra identidad** | Test: `crates/daemon/tests/runtime.rs::quota_exhausted_hands_the_same_agent_to_the_next_provider`<br>Test: `crates/daemon/tests/runtime.rs::manual_switch_replaces_only_the_executor_and_remembers_the_model` | ✅ |
| **Regla 4. Los errores no destruyen trabajo** | Test: `crates/daemon/tests/runtime.rs::executor_crash_fails_the_agent_and_opens_recovery`<br>Test: `crates/daemon/tests/runtime.rs::hung_executor_is_failed_and_reclaim_keeps_its_workspace` | ✅ |
| **Regla 5. Defaults simples** | Comando CLI: `symphony init` (asigna defaults sensatos sin necesidad de flags)<br>Test: `crates/tui/tests/views.rs::open_without_setup_goes_home_without_listing_other_projects` | ✅ |
| **Regla 6. Avanzado bajo demanda** | Snapshots TUI Home: `crates/tui/tests/snapshots/views__view_04_home_empty.snap`, `views__view_04_home_with_agents_and_a_problem.snap`<br>Scores, eventos y chunks se exponen en subpantallas (ExplainRoute, ContextObject, System) | ✅ |
| **Regla 7. Performance visible sólo cuando importa** | Test: `crates/tui/tests/views.rs::home_shows_the_real_system_strip` (strip resumido en Home; vista 18 solo bajo demanda) | ✅ |
| **Regla 8. No falsa precisión** | Tests: `crates/daemon/tests/runtime.rs::a_known_quota_report_below_the_reserve_marks_quota_low_with_a_real_percentage`, `crates/daemon/tests/runtime.rs::a_token_budget_in_the_config_gives_an_estimated_quota_never_a_percentage`<br>Snapshot: `views__view_22_provider_detail_with_a_temporary_limit_and_estimated_quota.snap` | ✅ |
| **Regla 9. Una misma lógica en CLI/TUI y GUI futura** | `apps/gui/src/views.tsx::VIEWS` consume exactamente las mismas llamadas IPC del daemon que `crates/tui/src/app.rs::update` (`agent.inspect`, `merge.get`, `context.stats`, etc.) | ✅ |
| **Regla 10. Recovery es un flujo de primera clase** | Tests: `crates/tui/tests/views.rs::view_30_recovery_center`, `inconsistent_state_opens_recovery_before_home`<br>Test: `crates/daemon/tests/runtime.rs::executor_crash_fails_the_agent_and_opens_recovery` | ✅ |

---

## 4. Principios de IDEA §2 (11 principios)

Fuente: `docs/spec/idea.md` § 2.

| Elemento | Evidencia | Resultado |
|---|---|---|
| **AGENT ≠ MODEL** | El agente persiste en `agents`; el modelo pertenece al ejecutor activo en `agent_runs`.<br>Test: `crates/daemon/tests/runtime.rs::quota_exhausted_hands_the_same_agent_to_the_next_provider`<br>Test: `crates/daemon/tests/runtime.rs::manual_switch_replaces_only_the_executor_and_remembers_the_model` | ✅ |
| **STATE ≠ MODEL MEMORY** | La verdad se persiste en SQLite y Git, no en la memoria del modelo.<br>Tests: `crates/daemon/tests/runtime.rs::session_leaves_an_up_to_date_checkpoint`, `handoff_combines_the_checkpoint_with_live_git` | ✅ |
| **CHECKPOINT BEFORE FAILURE** | Los checkpoints se generan incrementalmente durante la ejecución, nunca al fallar.<br>Tests: `crates/daemon/tests/runtime.rs::session_leaves_an_up_to_date_checkpoint`, `checkpoints_stay_monotonic_and_consistent`, `crashed_executor_can_restart_from_its_checkpoint` | ✅ |
| **WORKSPACES MUST BE ISOLATED** | Un worktree Git aislado por cada agente.<br>Tests: `crates/daemon/tests/runtime.rs::the_worktree_is_prepared_before_the_agent_starts_and_a_failure_creates_nothing`, `stopping_an_agent_frees_its_clean_worktree_and_keeps_the_branch` | ✅ |
| **HEAVY WORK IS SCHEDULED** | Concurrencia libre en red; serialización y límite estricto en tests/builds.<br>Test: `crates/daemon/tests/benchmark.rs::main_benchmark_three_agents_and_one_heavy_test_at_a_time` (verifica `assert!(!r.heavy_overlap)`) | ✅ |
| **SMART FEATURES ARE OPTIONAL** | El camino crítico no depende de modelos auxiliares ni features lentas.<br>Test: `crates/daemon/tests/runtime.rs::exact_model_creates_everything_and_runs_the_executor` (ejecución sin paso de enrutamiento) | ✅ |
| **RULES BEFORE ML** | Código determinístico prioritario sobre modelos de aprendizaje.<br>Implementación: `crates/daemon/src/routing.rs` basado en reglas explícitas; formalizado por ADR-0017 (ML rechazado en favor de heurísticas y `learned_performance`) | ✅ |
| **CLI FIRST** | CLI interactivo Clap y TUI Ratatui implementados en `crates/cli/src/main.rs` y `crates/tui/src/lib.rs` | ✅ |
| **LOCAL FIRST** | Todo el estado se almacena localmente en SQLite, Git y object store, sin nube propia.<br>Implementación: `crates/core/src/config.rs::SymphonyHome`, `crates/store/src/lib.rs`, `crates/object-store/src/lib.rs` | ✅ |
| **NO TELEMETRY** | Sin telemetría ni llamadas de red externas en el core de Symphony.<br>Verificación: CONSTRAINTS §1; crates del core libres de analytics o rastreo; auditoría documentada en `docs/research/auditoria/` | ✅ |
| **PERFORMANCE BEFORE FEATURES** | Techos de rendimiento verificados (RAM del daemon < 100 MB, startup < 2 s).<br>Test: `crates/daemon/tests/benchmark.rs::main_benchmark_three_agents_and_one_heavy_test_at_a_time` | ✅ |

---

## 5. Tablas de Base de Datos (42 tablas + 2 extensiones)

Fuente: `docs/spec/symphony_database.md` § 2 y `migrations/*.sql`.

| Tabla | Migración | Escritura (Writer) | Lectura (Reader) | Resultado |
|---|---|---|---|---|
| `projects` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_project` | `crates/store/src/repo.rs::get_project`, `crates/store/src/repo.rs::project_by_root` | ✅ |
| `sessions` | `migrations/001_core.sql` | `crates/store/src/repo.rs::start_session`, `crates/store/src/repo.rs::end_session` | `crates/store/src/repo.rs::unclosed_sessions` | ✅ |
| `tasks` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_task_kind`, `crates/store/src/repo.rs::set_task_status` | `crates/store/src/repo.rs::get_task`, `crates/daemon/src/views.rs::tasks` | ✅ |
| `task_dependencies` | `migrations/004_multiagent.sql` | `crates/store/src/deps.rs::add_dependency` | `crates/store/src/deps.rs::dependencies_of`, `crates/store/src/deps.rs::dependents_of` | ✅ |
| `milestones` | `migrations/004_multiagent.sql` | `crates/store/src/deps.rs::create_milestone`, `crates/store/src/deps.rs::assign_task` | `crates/store/src/deps.rs::get_milestone`, `crates/store/src/deps.rs::milestones_of` | ✅ |
| `agents` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_agent`, `crates/store/src/repo.rs::set_agent_state` | `crates/store/src/repo.rs::get_agent`, `crates/store/src/repo.rs::list_agents_rows` | ✅ |
| `worktrees` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_worktree`, `crates/store/src/repo.rs::set_worktree_status` | `crates/store/src/repo.rs::get_worktree`, `crates/store/src/repo.rs::reclaimable_worktrees` | ✅ |
| `agent_runs` | `migrations/001_core.sql` | `crates/store/src/repo.rs::open_run`, `crates/store/src/repo.rs::close_run` | `crates/store/src/repo.rs::current_run`, `crates/store/src/repo.rs::get_run` | ✅ |
| `executor_changes` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_executor_change` | `crates/daemon/src/views.rs::history` | ✅ |
| `messages` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_message` | `crates/store/src/repo.rs::agent_messages` | ✅ |
| `providers` | `migrations/001_core.sql` | `crates/store/src/repo.rs::upsert_provider` | `crates/store/src/repo.rs::ready_providers`, `crates/daemon/src/providers.rs::list` | ✅ |
| `provider_accounts` | `migrations/002_health.sql` | `crates/store/src/health.rs::ensure_account` | `crates/store/src/health.rs::get_health` (JOIN `account_id`) | ✅ |
| `models` | `migrations/001_core.sql` | `crates/store/src/repo.rs::upsert_model` | `crates/store/src/repo.rs::models_of`, `crates/daemon/src/views.rs::models` | ✅ |
| `provider_health` | `migrations/002_health.sql` | `crates/store/src/health.rs::put_health` | `crates/store/src/health.rs::get_health`, `crates/store/src/health.rs::list_health` | ✅ |
| `provider_failures` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_provider_failure` | `crates/store/src/health.rs::recent_failure_count`, `crates/store/src/repo.rs::recent_run_failure` | ✅ |
| `usage_records` | `migrations/002_health.sql` | `crates/store/src/health.rs::insert_usage`, `crates/store/src/health.rs::upsert_reported_usage` | `crates/store/src/health.rs::usage_since`, `crates/store/src/health.rs::provider_tokens_since` | ✅ |
| `profiles` | `migrations/002_health.sql` | `migrations/002_health.sql` (seed inicial en migración de profiles builtin) | `crates/daemon/src/routing.rs::profile_weights`, `crates/daemon/src/routing.rs::list_profiles` | ✅ |
| `profile_models` | `migrations/002_health.sql` | `crates/store/src/suggestions.rs::record_outcome`, `crates/daemon/src/routing.rs::bootstrap` | `crates/daemon/src/routing.rs::candidates` | ✅ |
| `routing_decisions` | `migrations/002_health.sql` | `crates/daemon/src/routing.rs::persist` | `crates/daemon/src/routing.rs::explain` | ✅ |
| `routing_candidates` | `migrations/002_health.sql` | `crates/daemon/src/routing.rs::persist` | `crates/daemon/src/routing.rs::explain` | ✅ |
| `model_suggestions` | `migrations/007_suggestions.sql` | `crates/store/src/suggestions.rs::insert`, `crates/store/src/suggestions.rs::resolve` | `crates/store/src/suggestions.rs::pending_of_project`, `crates/store/src/suggestions.rs::was_kept` | ✅ |
| `events` | `migrations/001_core.sql` | `crates/store/src/writer.rs::insert_event` | `crates/daemon/src/views.rs::activity`, `crates/store/src/repo.rs::turns_finished` | ✅ |
| `tool_calls` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_tool_call`, `crates/store/src/repo.rs::finish_tool_call` | `crates/daemon/src/views.rs::activity`, `crates/store/src/processes.rs::sync_processes` | ✅ |
| `processes` | `migrations/004_multiagent.sql` | `crates/store/src/processes.rs::sync_processes`, `crates/store/src/processes.rs::close_processes` | `crates/store/src/processes.rs::processes_of` | ✅ |
| `resource_samples` | `migrations/004_multiagent.sql` | `crates/store/src/resources.rs::insert_sample` | `crates/store/src/resources.rs::samples_since`, `crates/daemon/src/server.rs::system_get` | ✅ |
| `checkpoints` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_checkpoint`, `crates/store/src/repo.rs::invalidate_checkpoint` | `crates/store/src/repo.rs::latest_checkpoint`, `crates/store/src/repo.rs::checkpoint_base` | ✅ |
| `checkpoint_refs` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_checkpoint` | `crates/store/src/repo.rs::prune_checkpoints` | ✅ |
| `blobs` | `migrations/001_core.sql` | `crates/object-store/src/lib.rs::put`, `crates/object-store/src/lib.rs::add_ref` | `crates/object-store/src/lib.rs::get`, `crates/object-store/src/lib.rs::gc` | ✅ |
| `context_objects` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_context_object`, `crates/store/src/context.rs::upsert_file` | `crates/store/src/context.rs::object_by_uri`, `crates/store/src/context.rs::object_by_id` | ✅ |
| `context_chunks` | `migrations/003_context.sql` | `crates/store/src/context.rs::replace_chunks`, `crates/store/src/context.rs::delete_file` | `crates/store/src/context.rs::search`, `crates/daemon/src/context_views.rs::stats` | ✅ |
| `context_fts` | `migrations/003_context.sql` (FTS5) | Triggers automáticos SQLite (`context_chunks_ai/ad`) | `crates/store/src/context.rs::search` | ✅ |
| `handoffs` | `migrations/001_core.sql` | `crates/store/src/repo.rs::insert_handoff`, `crates/store/src/repo.rs::set_handoff_outcome` | `crates/daemon/src/context_views.rs::inspect`, `crates/daemon/src/context_views.rs::stats` | ✅ |
| `handoff_items` | `migrations/003_context.sql` | `crates/store/src/repo.rs::insert_handoff` | `crates/daemon/src/context_views.rs::inspect` | ✅ |
| `context_retrievals` | `migrations/003_context.sql` | `crates/store/src/context.rs::record_retrieval` | `crates/store/src/context.rs::retrieval_stats`, `crates/daemon/src/context_views.rs::stats` | ✅ |
| `project_facts` | `migrations/003_context.sql` | `crates/store/src/context.rs::set_fact`, `crates/store/src/context.rs::set_fact_conflict` | `crates/store/src/context.rs::list_facts`, `crates/store/src/context.rs::get_current_facts` | ✅ |
| `validation_runs` | `migrations/005_validation.sql` | `crates/store/src/validation.rs::insert_run`, `crates/store/src/validation.rs::finish_run` | `crates/store/src/validation.rs::runs_of`, `crates/store/src/validation.rs::failed_runs` | ✅ |
| `validation_checks` | `migrations/005_validation.sql` | `crates/store/src/validation.rs::insert_check` | `crates/store/src/validation.rs::runs_of` | ✅ |
| `merge_requests` | `migrations/004_multiagent.sql` | `crates/store/src/merge.rs::open_or_reuse`, `crates/store/src/merge.rs::set_status` | `crates/store/src/merge.rs::get_mr`, `crates/store/src/merge.rs::latest_mr` | ✅ |
| `merge_conflicts` | `migrations/004_multiagent.sql` | `crates/store/src/merge.rs::replace_conflicts`, `crates/store/src/merge.rs::resolve_conflicts` | `crates/store/src/merge.rs::conflicts_of` | ✅ |
| `reviews` | `migrations/004_multiagent.sql` | `crates/store/src/merge.rs::insert_review`, `crates/store/src/merge.rs::finish_review` | `crates/store/src/merge.rs::review_by_reviewer`, `crates/store/src/merge.rs::reviews_of_target` | ✅ |
| `recovery_items` | `migrations/001_core.sql` | `crates/store/src/repo.rs::open_recovery_item`, `crates/store/src/repo.rs::resolve_recovery_items` | `crates/daemon/src/views.rs::recovery`, `crates/store/src/repo.rs::open_recovery_count` | ✅ |
| `skills` | `migrations/003_context.sql` | `crates/store/src/shared.rs::sync_skills`, `crates/store/src/shared.rs::set_skill_enabled` | `crates/store/src/shared.rs::list_skills` | ✅ |
| `mcp_servers` | `migrations/003_context.sql` | `crates/store/src/shared.rs::sync_mcp`, `crates/store/src/shared.rs::set_mcp_enabled` | `crates/store/src/shared.rs::list_mcp` | ✅ |
| `learning_samples` | `migrations/007_suggestions.sql` | `crates/store/src/suggestions.rs::record_outcome` | Comprobación de existencia atómica vía `INSERT OR IGNORE` en `crates/store/src/suggestions.rs::record_outcome` | ✅ |

---

## 6. Consultas frecuentes de DB §5 (10 consultas)

Fuente: `docs/spec/symphony_database.md` § 5.

| Consulta especificada | Evidencia en el código | Resultado |
|---|---|---|
| **1. Home**<br>Agentes activos + run abierto (`ended_at IS NULL`) + modelo + task, `provider_health` por proveedor y último `resource_samples` | `crates/store/src/repo.rs::home_rows`<br>`crates/daemon/src/server.rs::agent_list` | ✅ |
| **2. Agent Overview**<br>Agente + run actual + último checkpoint (`MAX(seq)`) + stats del diff (`diff_object_id`) + última `validation_runs` | `crates/daemon/src/server.rs::agent_inspect`<br>`crates/store/src/repo.rs::latest_checkpoint`<br>`crates/store/src/validation.rs::runs_of` | ✅ |
| **3. Agent History**<br>`executor_changes` + `agent_runs` del agente ordenados por `seq` | `crates/daemon/src/views.rs::history`<br>`crates/store/src/repo.rs::runs_of` | ✅ |
| **4. WAITING_RESOURCE**<br>`tool_calls` con `status='QUEUED'` + JOIN a `blocked_by_tool_call_id` → agente que ocupa el slot | `crates/daemon/src/scheduling.rs::hold_for`<br>`crates/daemon/src/server.rs::gate_heavy_tool` | ✅ |
| **5. Task List**<br>Tasks por status + `task_dependencies` pendientes | `crates/daemon/src/views.rs::tasks`<br>`crates/store/src/deps.rs::dependencies_of` | ✅ |
| **6. Explain Route**<br>`routing_decisions` + `routing_candidates` de la última decisión | `crates/daemon/src/routing.rs::explain`<br>método IPC `route.explain` (`crates/daemon/src/server.rs`) | ✅ |
| **7. Failover**<br>Último checkpoint válido + candidatos filtrados por `provider_health` + `failover_policy` | `crates/daemon/src/executor.rs::failover`<br>`crates/daemon/src/executor.rs::pick_replacement`<br>`crates/daemon/src/routing.rs::candidates` | ✅ |
| **8. Recovery Center**<br>`recovery_items WHERE status='OPEN'` | `crates/daemon/src/views.rs::recovery`<br>`crates/daemon/src/server.rs::recovery_list` | ✅ |
| **9. Context Stats**<br>`SUM(tokens_raw_estimate)`, `SUM(tokens_sent)` de handoffs + `context_retrievals WHERE found=0` | `crates/daemon/src/context_views.rs::stats`<br>`crates/store/src/context.rs::retrieval_stats` | ✅ |
| **10. Usage**<br>`usage_records` agrupado por proveedor, separando `REPORTED` y `ESTIMATED` | `crates/store/src/health.rs::usage_since`<br>`crates/daemon/src/server.rs::usage_get` | ✅ |

---

## 7. Comandos de la especificación

Fuente: `docs/spec/Symphony_CLI_Ideal_Technology_Stack.md` § 5.1, `docs/spec/idea.md` § 8 y `--help` del CLI.

| Comando especificado / subcomando | Evidencia (`crates/cli/src/main.rs`) | Estado / Ayuda CLI | Resultado |
|---|---|---|---|
| `symphony` | `Cli::command: Option<Cmd>` | Abre la TUI interactiva de Symphony | ✅ |
| `symphony status` | `Cmd::Status` | Estado del daemon (lo arranca si no está vivo) | ✅ |
| `symphony migrate split` | `Cmd::Migrate { action: MigrateCmd::Split }` | Reparte la base global en `<proyecto>/.symphony/` (P15.5) | ✅ |
| `symphony daemon start\|stop\|status` | `Cmd::Daemon { action: DaemonCmd }` | Controla el daemon `symphonyd` | ✅ |
| `symphony providers [refresh]` | `Cmd::Providers { action: Option<ProvidersCmd> }` | CLIs de IA detectados (Claude Code, Codex, …) | ✅ |
| `symphony usage [--days N]` | `Cmd::Usage { days }` | Uso de tokens por proveedor y modelo (informado vs estimado) | ✅ |
| `symphony init [--kind] [--name] [--git-init]` | `Cmd::Init { kind, name, git_init }` | Prepara la carpeta actual como proyecto Symphony | ✅ |
| `symphony spawn <task> [-m] [--profile] …` | `Cmd::Spawn { task, model, profile, failover, context_mode, priority, description, cwd }` | Crea un agente nuevo con su tarea y workspace aislado | ✅ |
| `symphony agents [-a/--all]` | `Cmd::Agents { all }` | Lista los agentes del proyecto | ✅ |
| `symphony send <agent> <message>` | `Cmd::Send { agent, message }` | Envía mensaje o instrucción al executor vivo del agente | ✅ |
| `symphony pause <agent>` | `Cmd::Pause { agent }` | Pausa el executor de un agente | ✅ |
| `symphony resume <agent>` | `Cmd::Resume { agent }` | Reanuda un agente pausado | ✅ |
| `symphony stop <agent>` | `Cmd::Stop { agent }` | Detiene un agente y su executor | ✅ |
| `symphony kill <agent>` | `Cmd::Kill { agent }` | Termina forzosamente el executor y cancela el agente | ✅ |
| `symphony switch <agent> [-m] [-p]` | `Cmd::Switch { agent, model, target_model, profile }` | Cambia el modelo del agente a uno exacto o profile | ✅ |
| `symphony explain-route <agent>` | `Cmd::ExplainRoute { agent }` | Explica decisiones de routing y candidatos evaluados | ✅ |
| `symphony context inspect\|stats\|raw` | `Cmd::Context { action: ContextCmd }` | Inspecciona handoffs, estadísticas y objetos `ctx://` | ✅ |
| `symphony fact set\|list` | `Cmd::Fact { action: FactCmd }` | Hechos consolidados del proyecto (`CURRENT`, etc.) | ✅ |
| `symphony suggestion list\|switch\|keep\|mute` | `Cmd::Suggestion { action: SuggestionCmd }` | Gestión de sugerencias de cambio de modelo | ✅ |
| `symphony plugin list\|check` | `Cmd::Plugin { action: PluginCmd }` | Plugins instalados (validators y context transformers) | ✅ |
| `symphony review <agent> [--model] [--profile]` | `Cmd::Review { agent, model, profile }` | Pide revisión a un review agent | ✅ |
| `symphony chat merge [--review] [--yes] …` | `Cmd::Chat { action: ChatCmd::Merge }` | Integra el chat general en `main` | ✅ |
| `symphony diff <agent>` | `Cmd::Diff { agent }` | Git diff del worktree del agente contra su base | ✅ |
| `symphony logs <agent> [-l]` | `Cmd::Logs { agent, limit }` | Historial de mensajes y conversación del agente | ✅ |
| `symphony attach <agent> [--print]` | `Cmd::Attach { agent, print }` | Abre la sesión del agente en el CLI oficial | ✅ |
| `symphony inspect <agent>` | `Cmd::Inspect { agent }` | Inspección detallada del agente | ✅ |
| `symphony skills [list\|enable\|disable]` | `Cmd::Skills { action: Option<ToggleCmd> }` | Skills compartidas entre proveedores | ✅ |
| `symphony mcp serve\|list\|enable\|disable` | `Cmd::Mcp { action: McpCmd }` | Servidores MCP compartidos y broker stdio | ✅ |
| `symphony hook emit` | `Cmd::Hook { action: HookCmd::Emit }` | Entrada de eventos de hooks por stdin al daemon | ✅ |
| `symphony plan` | Previsto en IDEA §8 como idea inicial; implementado como orquestación por fases mediante archivos `.symphony/plan/*.md` ejecutados por el runtime (`crates/daemon/tests/runtime.rs::a_plan_runs_one_phase_per_session_and_is_deleted_when_done`), sin subcomando CLI dedicado. | 🟡 |

---

## 8. Resumen global

### Conteo por bloque

| Bloque | Total | ✅ | 🟡 | ❌ | ⛔ |
|---|---|---|---|---|---|
| **1. Vistas de FLOW §18** | 36 | 35 | 1 | 0 | 0 |
| **2. Journeys de FLOW §20** | 5 | 5 | 0 | 0 | 1* |
| **3. Reglas UX de FLOW §21** | 10 | 10 | 0 | 0 | 0 |
| **4. Principios de IDEA §2** | 11 | 11 | 0 | 0 | 0 |
| **5. Tablas de Base de Datos** | 44 | 44 | 0 | 0 | 0 |
| **6. Consultas de DB §5** | 10 | 10 | 0 | 0 | 0 |
| **7. Comandos de la especificación** | 29 | 28 | 1 | 0 | 0 |
| **Total elementos evaluados** | **145** | **143** | **2** | **0** | **1** |

*\*Nota en Journeys:* Los 5 journeys cuentan con suite de tests L2 con fake-agents en verde. La ejecución live con proveedores reales en Journey A está implementada (`live_journey_a_with_claude_code`), pero catalogada como ⛔ al requerir suscripción real y autorización expresa de Leo.

### Lista de elementos parciales (🟡) y bloqueados (⛔)

- **Vista 31: Settings (🟡):**
  - *Estado:* En la GUI existe la vista 31 (`apps/gui/src/views.tsx:387`) conectada a `project.status`; en el backend y CLI la configuración se gestiona mediante archivos `config.toml` y `project.toml`.
  - *Qué falta:* Una pantalla interactiva dedicada a Settings dentro de la TUI de Ratatui (actualmente se gestiona por fichero o mediante `FirstRun`).
- **Comando `symphony plan` (🟡):**
  - *Estado:* La funcionalidad de plan por fases e iteración incremental está implementada y cubierta por tests a través de los archivos de plan en `.symphony/plan/*.md` leídos por el runtime del daemon.
  - *Qué falta:* El subcomando CLI `symphony plan` que contemplaba IDEA §8 no se construyó como CLI standalone al integrarse la orquestación directamente en el daemon y la TUI.
- **Journeys en modo Live (⛔):**
  - *Estado:* La suite L2 (fake-agents) cubre el 100% de los flujos de los 5 journeys de forma determinística en CI.
  - *Qué falta:* Corridas live con cuotas de pago reales de Claude, Codex, Antigravity, Kimi y Copilot reservadas para P16.S2 bajo autorización explícita de Leo.
