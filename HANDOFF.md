# HANDOFF — session continuity

## Current state (last updated: Wave 2 security infra + traffic store done)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 `artex-pg` :55432, restart unless-stopped), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX read-only at `ARTEX/` @ `5cb44fb`. Discovery docs + BASELINE + ROADMAP + DECISIONS + GLOSSARY + MAP + HANDOFF.
- Crates DONE (each: cargo test + clippy 0 + fmt green):
  - Wave 0/1: config, core, migrations, database (+seed), auth (Go↔Rust JWT interop), assets, api (partial), app
  - Data layer (subagents): tasks (13), findings (7), exploration (5), llm (9), settings (5)
  - **Wave 2 security (subagents)**: guard (11, async traits), interception (21, rules string/regex/argv + target gate + pending), egress (14, governor), budget (13)
  - **traffic** (7, blobs + FTS index + search + external ingest)
- API routes wired (e2e vs Postgres): health, auth/*, assets(+/counts), companies, tasks(/{id}), task-categories, task-templates, exploration/findings(+/stats,/{id}), exploration/graph, exploration/activity, llm/profiles, llm/records, stats, settings.
- **136 Rust tests pass, clippy 0 warnings, fmt OK.**
- Parity: Rust migrates+seeds DB → Go db tests pass; JWT interop; data queries match Go semantics.

### Key integration notes
- `guard` traits are now `async` (`#[async_trait]`); `InterceptorImpl` in interception implements them (DB-backed).
- `interception` argv matching uses simplified shell tokenizer (full mvdan.cc/sh grammar not ported); argv rule matches derived prefix (Go `argvRuleMatches` semantics).
- `egress` Governor is async rate/concurrency gate; real HTTP transport (reqwest) to be wired in Wave 3+.
- `traffic` store uses rusqlite (bundled) + SHA-256 blobs; MITM proxy (TLS) is a separate future crate.

### Exact next steps
1. Wire guard/interception/egress/budget into the agent runtime (Wave 4) + API routes for rules/pending.
2. Wave 3 LLM: live provider streaming (reqwest), provider CRUD routes, per-task LLM selection.
3. Wave 6: proxy crate (rustls + hyper + rcgen CA) on top of traffic store; /api/traffic routes.
4. Wave 4 agent runtime (planner/worker/engine) — largest remaining piece.
5. Keep `artex-pg` running (:55432). Build `cargo build -p app`; run `./target/debug/artex -addr 127.0.0.1:PORT`.

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api,assets,tasks,findings,exploration,llm,settings,guard,interception,egress,budget,traffic}`, `app/`
- Docs: discovery set + MAP/HANDOFF/DECISIONS/GLOSSARY