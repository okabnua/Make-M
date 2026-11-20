# ROADMAP — ARTEX Rust migration (dependency-wave order)

Derived from `research/rust-anything-analyze.md` (wave column) and the real import graph. Each unit moves ANALYZED → PLANNED → IMPLEMENTED → TESTED → VERIFIED through the rust-anything loop (`analyze → bridge → record → build → diff → bench → shadow → gate`), or, for the HTTP backend, the whole-app loop (`app record → app replay → app front-door → wire → lift`).

## Wave ordering (from analyze)

| Wave | Unit | Lines | Readiness | Rust crate(s) | Bridge |
| ---: | --- | ---: | ---: | --- | --- |
| 0 | `config/` | 212 | 81 | config | cgo |
| 0 | `llmpool/` | 444 | 30 | llm (pool) | cgo |
| 0 | `mcphttp/` | 585 | 16 | mcp | cgo |
| 0 | `sidequestion/` | 704 | 7 | agent (sidequestion) | cgo |
| 0 | `notify/` | 2733 | 0 | notifications | cgo |
| 1 | `db/` | 21160 | 0 | database, migrations | cgo |
| 2 | `report/` | 290 | 51 | reporting | cgo |
| 2 | `llmrec/` | 523 | 25 | llm (record) | cgo |
| 2 | `intercept/` | 3863 | 0 | interception, approval, scope | cgo |
| 2 | `traffic/` | 2480 | 0 | traffic, proxy | cgo |
| 3 | `guard/` | 330 | 50 | guard | cgo |
| 3 | `evidence/` | 399 | 48 | evidence | cgo |
| 3 | `egress/` | 414 | 33 | egress | cgo |
| 4 | `enrich/` | 280 | 37 | enrichment | cgo |
| 4 | `agent/` | 9561 | 0 | agent, planner, worker, tools, llm | cgo |
| 5 | `server/` | 26397 | 0 | api, scheduler, auth, graph, assets, findings | cgo |
| 6 | `cmd/artex/` | 131 | 81 | app (tokio main) | cgo |

## Migration waves (grouped by PHASE-01 §6)

### Wave 0 — Foundation
- Cargo workspace skeleton (`Cargo.toml` members, `app/`).
- `config` crate (config.json + env). Start here (highest readiness, leaf, no deps).
- Domain types (`core`): node kinds/states, enums from `db/constants.go`.
- Logging (`tracing`), error types (`thiserror`).
- Runtime init: tokio main skeleton in `app/` (no server yet).

### Wave 1 — Data & core services
- `migrations` crate: embed `schema.sql` + versioned migrations via sqlx.
- `database` crate: Postgres pool, typed repos for assets/companies/tasks/explorations.
- `auth`: JWT (jsonwebtoken), bcrypt, setup token, login limiter, trusted proxies.
- Shared domain models: assets, findings, tasks.

### Wave 2 — Security infrastructure
- `guard` (PreToolUse/PostToolUse), `scope` (target gate), `approval` (intercept ask/decide).
- `interception` rules: string/regex/argv (`fancy-regex` for lookaround/backrefs), argv parser.
- `budget` (token/turn/intent budgets).
- `egress` governor (rate/burst/concurrency/cache/coalesce).
- Secret encryption (`aes-gcm`).

### Wave 3 — LLM infrastructure
- `llm` provider abstraction + `llm-providers` (anthropic/openai/openai-responses/ollama).
- `llmpool` failover + circuit breaker (health persistence).
- `llmrec` recording decorator + usage accounting.

### Wave 4 — Autonomous engine
- `agent` core (goals/mainagent/chat/sidequestion).
- `planner` loop + shared todolist.
- `worker` loop + intent queue (engine).
- Task lifecycle, resumption, graph-triggered events.

### Wave 5 — Tool execution
- `tools` registry + implementations (graph_overview, record_fact, report_finding, insert_assets, etc.).
- Bash/HTTP/browser execution, output capture, tool approvals.

### Wave 6 — Evidence & traffic
- `evidence` content-addressed blobs + GC.
- `proxy` (MITM: rustls + hyper + rcgen CA) + `traffic` store (rusqlite FTS5 + blobs).
- Traffic search/replay, browser-extension ingest.
- Findings + retests + verification.

### Wave 7 — Platform integrations
- `mcp` (rmcp/custom JSON-RPC), skills, memory, workspace, `search`, `egress` done, `enrichment`, ScopeSentry sync, `notifications`, `reporting`.

### Wave 8 — API & UI integration
- `api` (axum) — full route parity (API-INVENTORY.md), JWT, SSE.
- Static UI embedding (`rust-embed` serving `web/out`).
- Preserve all frontend workflows; verify browser-extension `/api/traffic/ingest`.

### Wave 9 — Deployment & cutover
- `app` binary (tokio main), Dockerfile, compose, install/start scripts.
- Cross-platform builds, schema upgrade, backup/restore, health checks.
- Final whole-app replay; reduce Go fallback to zero; `app lift` on a copy.

## Verification gates per wave
- Unit waves: `rust-anything diff` (recorded cases) + `gate` (fmt/clippy/test/behavior/api/size) + `bench`.
- Whole-app: `app record` → `app replay` after each integration wave.
- Cutover: all gates pass, replay passes, no unresolved critical mismatch.

## Dependency decisions needed (research files)
`rust-anything` flagged D1–D33 dependency replacements (norma SDK, pgx, go-mitmproxy, miekg/dns, dnsx, bcrypt, modernc sqlite, mvdan/sh, etc.). Each needs a researched Rust equivalent under `research/` before porting its users. Critical: **norma SDK** (agentcore/harness/tool/llm/permission/transcript/compaction) has no Rust equivalent — this is the largest design decision (see `research/github.com-Autumn-27-norma.md`).