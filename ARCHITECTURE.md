# ARCHITECTURE — ARTEX (original) and Rust rebuild target

Original source: `ARTEX/` @ `5cb44fb`. This document describes the original architecture (verified from source) and the Rust target architecture.

## 1. Original architecture (Go)

ARTEX is an **LLM multi-agent autonomous penetration-testing system**: a Go monolithic backend (embedded Next.js UI) + PostgreSQL, built on the `norma` agent SDK (`agentcore`/`tool`/`permission`/`harness`/`memory`/`transcript`).

### Layering

```
Frontend (Next.js static, go:embed)  ──HTTP/SSE──►  server (net/http, JWT, SSE)
                                                        │
                              ┌─────────────────────────┼──────────────────────────┐
                              ▼                          ▼                          ▼
                          Manager                  Engine (scheduler)          supporting subsystems
                     (task/engine/store LC)     plannerLoop + N workerLoop   proxy, guard/intercept,
                                                                     enrich, evidence, mcp, notify
                              │                          │
                              ▼                          ▼
                     PostgreSQL (dual graphs)     agent (norma): goals/planner/worker/mainagent/chat
```

### Dual-graph core
- **Asset graph** (global, shared source of truth): nodes `root_domain/subdomain/ip/service/app/endpoint` owned by companies; parent-child + dedup computed by the program.
- **Exploration graph** (per-task): nodes `goal/intent/fact/finding/hint/digest` connected by edges `spawns/derived_from/yields/proves/covers` — the lineage/advance chain.
- **Anchors** `exploration_anchors(node_id, asset_id)` link the two graphs → powers asset test coverage + coverage graph.

### Engine closed loop
1. Any graph change wakes the planner (debounced).
2. Planner reads situation, dispatches 0..N intents to the frontier (with asset_ids).
3. Workers claim one intent, execute real tools, write back facts/assets/findings + per-step activity.
4. Write-back triggers next wake. Ends when goal proven.

Cross-worker info exchange: `search_all_worker_traces`, `list_worker_traces`, `get_worker_trace`.
Stable attack chains: per-task shared **planner todolist** retained across wake-ups; each round dispatches only next-step intents whose prerequisites are met.

### Agent roles
goals (decompose + constraints), planner (sole intent generator), worker (execute one intent), mainagent (human-in-loop orchestrator), chat (conversational), retester (re-tests findings).

### Supporting subsystems (see INVENTORY.md for one-liners)
guard/intercept (tool approval + target gate), egress (noise governor), enrich (DNS/HTTP), evidence (content-addressed blobs), llmpool (LLM failover + circuit breaker), llmrec (LLM recording), mcphttp (remote MCP), notify (IM/email), report (MD/CSV), sidequestion (`/btw`), traffic (MITM proxy + SQLite FTS).

### Data store
PostgreSQL via pgx; schema embedded and applied idempotently on every start (`schema.sql` + versioned `migrations/*.sql` recorded in `schema_migrations`); AES-256-GCM secret encryption (`keys/secret.key`).

## 2. Rust rebuild target architecture

Modular Cargo workspace at the `Make-M` root (`app/`, `crates/`). Proposed crates (adjust to the real dependency graph; not every component needs its own crate):

| Crate | Replaces | Notes |
| --- | --- | --- |
| `core` | shared domain types | assets, findings, nodes, enums (node kind/state, severity) |
| `config` | `config/` | config.json + env; figment/serde |
| `database` | `db/` (data access) | sqlx + pgx-compatible Postgres |
| `migrations` | `db/schema.sql`, `db/migrations` | embedded via `sqlx::migrate!` |
| `api` | `server/` routing | axum, JWT, SSE, static embed (rust-embed) |
| `auth` | `server/auth.go` | jsonwebtoken + bcrypt, setup token, login limiter |
| `scheduler` | `server/scheduler.go` | triggers, tick loop (tokio) |
| `agent` | `agent/` core | goals/mainagent/chat, sidequestion |
| `planner` | `agent/planner.go` | planner loop + shared todolist |
| `worker` | `agent/worker.go` + `server/engine` | intent claiming + execution loop |
| `llm` | `agent/provider.go`, `llmrec`, `llmpool` | provider abstraction, record, pool |
| `llm-providers` | provider formats | anthropic/openai/openai-responses/ollama |
| `tools` | `agent/tools*.go` | tool registry + implementations |
| `guard` | `guard/` | PreToolUse/PostToolUse hooks |
| `scope` | target gate | host allow/deny |
| `approval` | `intercept/` (ask/decide) | interception + pending decisions |
| `budget` | `agent/budget.go` | token/turn budgets |
| `graph` | asset + exploration graphs | traversal, coverage |
| `assets` | `db` asset layer | |
| `exploration` | `db` exploration layer | |
| `traffic` | `traffic/` (store) | blobs + index (rusqlite) |
| `proxy` | `traffic/` (MITM) | rustls + hyper; CA gen (rcgen) |
| `interception` | `intercept/` rules/argv/judge | |
| `evidence` | `evidence/` | content-addressed blobs |
| `findings` | findings + retests | |
| `retester` | `agent/retester.go` | |
| `enrichment` | `enrich/` | hickory-dns + reqwest |
| `reporting` | `report/` | markdown/csv |
| `notifications` | `notify/` | per-channel |
| `mcp` | `mcphttp/` | rmcp or custom JSON-RPC client |
| `egress` | `egress/` | governor (governor crate) |
| `search` | web search | brave/tavily |
| `logging` | `server` log sink | tracing |

`app/` = tokio main binary (`artex`), wires everything, serves HTTP+proxy, handles signals/shutdown.

### Rust engineering standards (per PHASE-01 §4)
stable Rust · tokio · axum · sqlx (Postgres) · serde · tracing · explicit error types (thiserror) · strongly typed domain models · controlled concurrency · cancellation-safe async · secure secret mgmt · minimal unsafe · no gratuitous FFI in the final binary.

### Kept vs replaced
- **Replaced by Rust**: all backend Go packages.
- **Preserved**: `web/` (Next.js static export, embedded), `extension/` (unchanged), `skills/` (data).
- Frontend is documented as a *presentation layer*, not a Rust backend dependency (PHASE-01 §11).