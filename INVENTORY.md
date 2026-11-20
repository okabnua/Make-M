# INVENTORY — ARTEX source components

Source: `ARTEX/` @ `5cb44fb` (791 tracked files). Companion docs: `SOURCE-DISPOSITION.md` (per-file), `ARCHITECTURE.md`, `DATABASE-INVENTORY.md`, `API-INVENTORY.md`, `FEATURE-PARITY.md`, `research/rust-anything-analyze.md`.

Original module: `github.com/Autumn-27/artex` · Go 1.26 · Postgres (pgx) · agent SDK `norma`.

## Component inventory

| # | Component (Go pkg) | Path | Files | ~Lines | Responsibility | Rust dest |
| --- | --- | --- | ---: | ---: | --- | --- |
| 1 | entrypoint | `cmd/artex` | 2 | 131 | CLI flags, HTTP+proxy server bootstrap, signal shutdown | `app/` (tokio main) |
| 2 | server | `server` | 154 | 26397 | HTTP API, Manager, Engine (planner/worker loop), scheduler, notifier, archive worker, auth, SSE, static embed | `crates/api`, `crates/scheduler`, `crates/worker`, `crates/planner`, `crates/auth` |
| 3 | agent | `agent` | 94 | 9561 | goals/planner/worker/mainagent/chat agents, tools, prompts, compaction, provider, budgets, RAG, noise | `crates/agent`, `crates/planner`, `crates/worker`, `crates/llm-providers`, `crates/tools` |
| 4 | db | `db` | 124 | 21160 | Postgres data layer, schema.sql (1610 lines), migrations, secrets AES-GCM | `crates/database`, `crates/migrations` |
| 5 | config | `config` | 2 | 212 | config.json + env resolution | `crates/config` |
| 6 | guard | `guard` | 6 | 330 | PreToolUse/PostToolUse safety hooks, target gate | `crates/guard` |
| 7 | intercept | `intercept` | 37 | 3863 | User interception rules (string/regex/argv), target pins, LLM judge, always-allow denylist | `crates/interception` |
| 8 | egress | `egress` | 2 | 414 | HTTP noise governor (rate/burst/concurrency/cache/coalesce) | `crates/egress` |
| 9 | enrich | `enrich` | 1 | 280 | Async DNS resolve + HTTP probe asset enrichment | `crates/enrichment` |
| 10 | evidence | `evidence` | 3 | 399 | Content-addressed evidence blobs, GC, archive portability | `crates/evidence` |
| 11 | llmpool | `llmpool` | 4 | 444 | LLM round-robin pool + circuit-breaker registry | `crates/llm-providers` (pool) |
| 12 | llmrec | `llmrec` | 5 | 523 | LLM recording decorator (requests/responses/usage) | `crates/llm` (record) |
| 13 | mcphttp | `mcphttp` | 3 | 585 | Remote MCP clients (Streamable HTTP + legacy SSE) | `crates/mcp` |
| 14 | notify | `notify` | 27 | 2733 | IM/email push channels (dingtalk/feishu/wecom/webhook/telegram/email) | `crates/notifications` |
| 15 | report | `report` | 3 | 290 | Markdown/CSV finding & coverage reports | `crates/reporting` |
| 16 | sidequestion | `sidequestion` | 10 | 704 | `/btw` side questions with snapshots + SSE | `crates/agent` (sidequestion) |
| 17 | traffic | `traffic` | 12 | 2480 | MITM recording proxy (go-mitmproxy), SQLite FTS index, blobs, search, browser-extension ingest | `crates/traffic`, `crates/proxy` |
| — | web | `web` | 209 | — | Next.js 16 static export (presentation only) | preserved (embedded static) |
| — | extension | `extension` | 4 | — | MV3 Chrome/Edge traffic capture | preserved as-is |
| — | skills | `skills` | 28 | — | Runtime skill packs (SKILL.md + files) | preserved (data) |

## Runtime entry points (verified from source)

- **HTTP server**: `cmd/artex/main.go` → `server.New(...)` → `http.Server{Addr: -addr (127.0.0.1:8787), Handler: srv.Handler()}`.
- **Traffic proxy**: started by `server.NewManager(dataDir, proxyAddr)` where `proxyAddr = -proxy` (127.0.0.1:8788).
- **Background goroutines** (from `server.New`): evidence GC, scheduler (5s tick), notifier (3s tick), LLM pool reconcile, MCP discover-on-startup, task runtime restore, task archive worker.
- **Frontend**: static files embedded via `//go:embed all:webui/dist` under `-tags embedui`, served on same port.

## External runtime dependencies

| Dependency | Used by | Purpose |
| --- | --- | --- |
| PostgreSQL | db, most subsystems | primary datastore (pgx) |
| LLM provider (Anthropic/OpenAI/Ollama) | agent | autonomous reasoning |
| External scanners (nmap, nuclei, httpx, ffuf, etc.) | worker via Bash | security testing |
| MCP servers (stdio/http/sse) | mcphttp | tool extensions |
| ScopeSentry (optional) | sync | asset sync |
| Notification channels | notify | outbound alerts |
| Web search (Brave/Tavily) | settings | search |

## Build / deploy artifacts

- `Dockerfile`, `docker-compose.yml`, `docker-compose.bench.yml`
- `build.sh` (embed UI + build + package), `install.sh`, `start.sh`/`start.bat`, `update.sh`, `dev.sh`, `reset-password.sh`
- `.github/workflows/` CI + release
- `.env.example`, `config.example.json`