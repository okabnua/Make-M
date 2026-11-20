# HANDOFF — session continuity

## Current state (last updated: all data-layer crates built via subagents + integrated into API)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 `artex-pg` :55432, restart unless-stopped), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX read-only at `ARTEX/` @ `5cb44fb`. Discovery docs + BASELINE + ROADMAP + DECISIONS + GLOSSARY + MAP + HANDOFF.
- Crates DONE (each with cargo test + clippy + fmt green):
  - config, core, migrations, database (+seed), auth (Go↔Rust JWT interop), assets, api (partial), app
  - **tasks** (13 tests): Task model + categories + templates + count_by_status
  - **findings** (7 tests): Finding + retests + stats + groups
  - **exploration** (5 tests): graph/nodes/frontier/intents/activity/token read side
  - **llm** (9 tests): profiles + health (circuit breaker) + records + usage
  - **settings** (5 tests): typed getters/setters + stats; secret keys excluded from list_all
- API routes wired (all verified e2e vs Postgres): /api/health, /api/auth/*, /api/assets, /api/assets/counts, /api/companies, /api/tasks, /api/tasks/{id}, /api/task-categories, /api/task-templates, /api/exploration/findings(+/stats,/{id}), /api/exploration/graph, /api/exploration/activity, /api/llm/profiles, /api/llm/records, /api/stats, /api/settings.
- **70 Rust tests pass, clippy 0 warnings, fmt OK.**
- Parity: Rust migrates+seeds DB → Go db tests pass; JWT interop; asset/company/task/finding queries match Go semantics.

### Remaining failures
- None in Rust workspace. (Go baseline has 1 flaky server test, pre-existing.)

### Exact next steps
1. Add write routes: POST/PATCH/DELETE /api/tasks, POST /api/companies + scope, POST /api/exploration/findings, PUT /api/settings, /api/llm/profiles CRUD.
2. Wave 2 security infra crate(s): guard, interception (rules/argv/judge), scope/target-gate, budget, egress.
3. traffic crate (MITM proxy + store) + /api/traffic routes.
4. agent runtime (planner/worker) — largest piece; port norma SDK behavior.
5. Keep `artex-pg` running (:55432) for differential tests. Build app with `cargo build -p app`; run `./target/debug/artex -addr 127.0.0.1:PORT`.

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api,assets,tasks,findings,exploration,llm,settings}`, `app/`, `tests/`
- Docs: discovery set + MAP/HANDOFF/DECISIONS/GLOSSARY