# HANDOFF — session continuity

## Current state (last updated: Wave 4 started — engine orchestration + intent lifecycle)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 `artex-pg` :55432), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX read-only at `ARTEX/` @ `5cb44fb`. Discovery docs + BASELINE + ROADMAP + DECISIONS + GLOSSARY + MAP + HANDOFF.
- Crates DONE (each: cargo test + clippy 0 + fmt green):
  - Wave 0/1: config, core, migrations, database (+seed), auth, assets, api (partial), app
  - Data: tasks (13), findings (7), exploration (5), llm (9), settings (5)
  - Wave 2: guard (11), interception (21), egress (14), budget (13)
  - Wave 6: traffic (7), evidence (8), proxy (3)
  - Wave 7: reporting (26), enrichment (16), mcp (5), notifications (8)
  - **Wave 4 (started)**: `engine` (2 integration tests) — Planner/Worker traits + step loop (dispatch/claim/complete) + recover; `exploration` extended with intent lifecycle (claim_intent_token, set_intent_state, has_active_intent, has_open_goal, reset_running, reopen, soft_delete).
- **206 Rust tests pass, clippy 0 warnings, fmt OK.**
- Parity: Rust migrates+seeds DB → Go db tests pass; JWT interop.

### Key notes
- `guard` traits async (`#[async_trait]`).
- `engine` is the DB-driven orchestration; LLM reasoning (norma SDK planner/worker) is behind `Planner`/`Worker` traits — the concrete impl is the next big piece.
- `proxy` captures plain HTTP; TLS CONNECT gap documented.
- `notifications` email SMTP send is a stub.
- Disk: `cargo clean` was needed once (target hit 15G); monitor.

### Exact next steps
1. Implement concrete LLM-backed `Planner`/`Worker` (port `agent/planner.go` + `agent/worker.go` + norma SDK behavior) — the hardest remaining piece.
2. Wire engine + guard/interception/egress/budget/mcp/notifications/proxy into app + /api routes (tasks/findings write, mcp, notify, traffic, engine control).
3. Wave 8: full API parity + static UI embedding (rust-embed) + SSE streams.
4. Wave 9: Dockerfile, compose, install scripts, cutover.
5. Keep `artex-pg` running (:55432).

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api,assets,tasks,findings,exploration,llm,settings,guard,interception,egress,budget,traffic,evidence,proxy,reporting,enrichment,mcp,notifications,engine}`, `app/`
- Docs: discovery set + MAP/HANDOFF/DECISIONS/GLOSSARY