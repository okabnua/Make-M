# HANDOFF — session continuity

## Current state (last updated: Wave 3/4 — LLM live client + concrete agent)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 `artex-pg` :55432), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX read-only at `ARTEX/` @ `5cb44fb`. Discovery docs + BASELINE + ROADMAP + DECISIONS + GLOSSARY + MAP + HANDOFF.
- Crates DONE (each: cargo test + clippy 0 + fmt green):
  - Wave 0/1: config, core, migrations, database (+seed), auth, assets, api (partial), app
  - Data: tasks, findings, exploration (+intent lifecycle), llm (+live client), settings
  - Wave 2: guard, interception, egress, budget
  - Wave 6: traffic, evidence, proxy
  - Wave 7: reporting, enrichment, mcp, notifications
  - Wave 4: engine (Planner/Worker traits + loop + recover), agent (LlmPlanner/LlmWorker concrete)
  - Wave 3: llm LlmClient (openai/anthropic/openai-responses/ollama, non-streaming, 28 tests)
- **231 Rust tests pass, clippy 0 warnings, fmt OK.**
- Parity: Rust migrates+seeds DB → Go db tests pass; JWT interop.

### Key notes
- `guard` traits async (`#[async_trait]`).
- `engine` = DB-driven orchestration; `agent::LlmPlanner`/`LlmWorker` = concrete LLM-backed impls (call LlmClient, parse JSON). Tool execution / compaction / transcript / harness NOT ported (norma SDK).
- `llm::LlmClient` non-streaming; streaming + tool-use loop + retries NOT ported.
- `proxy` captures plain HTTP; TLS CONNECT gap.
- `notifications` email SMTP stub.
- Disk: `cargo clean` was needed once (target hit 15G); monitor.

### Exact next steps
1. Add tool execution to agent (Bash/HTTP/browser via a `tools` crate) + tool-use loop in LlmWorker; streaming in LlmClient.
2. Wire engine + agent + guard/interception/egress/budget/mcp/notifications/proxy into app + /api routes (tasks/findings write, mcp, notify, traffic, engine control, SSE).
3. Wave 8: full API parity (~140 routes) + static UI embedding (rust-embed) + SSE streams.
4. Wave 9: Dockerfile, compose, install scripts, cutover.
5. Keep `artex-pg` running (:55432).

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api,assets,tasks,findings,exploration,llm,settings,guard,interception,egress,budget,traffic,evidence,proxy,reporting,enrichment,mcp,notifications,engine,agent}`, `app/`
- Docs: discovery set + MAP/HANDOFF/DECISIONS/GLOSSARY