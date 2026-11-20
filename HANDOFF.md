# HANDOFF — session continuity

## Current state (last updated: Wave 6/7 done — mcp, notifications, proxy)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 `artex-pg` :55432), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX read-only at `ARTEX/` @ `5cb44fb`. Discovery docs + BASELINE + ROADMAP + DECISIONS + GLOSSARY + MAP + HANDOFF.
- Crates DONE (each: cargo test + clippy 0 + fmt green):
  - Wave 0/1: config, core, migrations, database (+seed), auth, assets, api (partial), app
  - Data layer: tasks (13), findings (7), exploration (5), llm (9), settings (5)
  - Wave 2 security: guard (11), interception (21), egress (14), budget (13)
  - Wave 6: traffic (7), evidence (8), proxy (3, CA+leaf+capturing forwarder)
  - Wave 7: reporting (26), enrichment (16), mcp (5), notifications (8)
- **202 Rust tests pass, clippy 0 warnings, fmt OK.**
- Parity: Rust migrates+seeds DB → Go db tests pass; JWT interop; data queries match Go.

### Key notes
- `guard` traits are async (`#[async_trait]`).
- `interception` argv: simplified shell tokenizer matching Go `argvRuleMatches`.
- `proxy` CA: rcgen 0.13 (key pair preserved across reload; cert re-derived). Captures plain HTTP; TLS CONNECT interception is a documented gap.
- `notifications` email: rendered, SMTP send is a documented stub.
- Disk note: `cargo clean` was needed once (target hit 15G). Current target ~ reasonable.

### Exact next steps
1. Wave 4 agent runtime (planner/worker/engine) — largest remaining piece; port norma SDK behavior.
2. Wire guard/interception/egress/budget/mcp/notifications/proxy into app + add /api routes (tasks write, findings write, mcp, notify, traffic).
3. Wave 8: full API parity + static UI embedding (rust-embed) + SSE streams.
4. Wave 9: Dockerfile, compose, install scripts, cutover.
5. Keep `artex-pg` running (:55432).

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api,assets,tasks,findings,exploration,llm,settings,guard,interception,egress,budget,traffic,evidence,proxy,reporting,enrichment,mcp,notifications}`, `app/`
- Docs: discovery set + MAP/HANDOFF/DECISIONS/GLOSSARY