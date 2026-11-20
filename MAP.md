# MAP — module migration map

Per-module live view (PHASE-01 §9). Updates as migration proceeds. Original paths are relative to `ARTEX/`.

| Module | Original path | Rust path | Dependencies | Migration status | Test status | Gate status | Known mismatches | Blockers | Next action |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| config | config/config.go | crates/config | serde, url, thiserror | IMPLEMENTED | TESTED (6 unit + diff vs Go) | — | none | none | run rust-anything gate on crate |
| core (enums) | db/constants.go | crates/core | — | IMPLEMENTED | TESTED (1) | — | none | none | expand with more enums |
| database | db/ | crates/database | sqlx | IMPLEMENTED (schema+migrations+seed) | TESTED (integration + parity) | gate pending | none | none | typed repos for assets/companies/tasks/explorations |
| migrations | db/schema.sql | crates/migrations | sha2 | IMPLEMENTED | TESTED (7 unit) | gate pending | none | none | (done) |
| auth | server/auth.go | crates/auth | jsonwebtoken, bcrypt | IMPLEMENTED | TESTED (8 unit + Go JWT interop) | gate pending | none | none | (done) |
| api | server/ (slice) | crates/api | axum | IMPLEMENTED (health+auth) | TESTED (e2e + Go interop) | gate pending | ~140 routes remain | — | add remaining routes from API-INVENTORY |
| app | cmd/artex | app/ | tokio, axum | IMPLEMENTED (serve+shutdown) | smoke-tested | — | only health+auth wired | — | wire more subsystems |
| auth | server/auth.go | crates/auth | jsonwebtoken, bcrypt | NOT_ANALYZED | — | — | — | — | Wave 1 |
| api | server/ | crates/api | axum | NOT_ANALYZED | — | — | — | ~140 routes | Wave 8 |
| agent | agent/ | crates/agent | tokio, llm | NOT_ANALYZED | — | — | — | norma SDK no equiv | Wave 4 |
| planner | agent/planner.go | crates/planner | agent | NOT_ANALYZED | — | — | — | 43KB | Wave 4 |
| worker | agent/worker.go | crates/worker | agent | NOT_ANALYZED | — | — | — | 40KB | Wave 4 |
| llm | agent/provider.go | crates/llm | reqwest | NOT_ANALYZED | — | — | — | 4 provider formats | Wave 3 |
| llm-providers | agent/provider.go | crates/llm-providers | llm | NOT_ANALYZED | — | — | — | — | Wave 3 |
| llmpool | llmpool/ | crates/llm (pool) | llm | NOT_ANALYZED | — | — | — | circuit breaker | Wave 3 |
| llmrec | llmrec/ | crates/llm (record) | llm | NOT_ANALYZED | — | — | — | — | Wave 3 |
| guard | guard/ | crates/guard | — | NOT_ANALYZED | — | — | — | — | Wave 2 |
| intercept | intercept/ | crates/interception | fancy-regex | NOT_ANALYZED | — | — | — | argv parser, LLM judge | Wave 2 |
| scope | intercept/target_* | crates/scope | — | NOT_ANALYZED | — | — | — | — | Wave 2 |
| approval | intercept (ask) | crates/approval | — | NOT_ANALYZED | — | — | — | — | Wave 2 |
| budget | agent/budget.go | crates/budget | — | NOT_ANALYZED | — | — | — | — | Wave 2 |
| assets/companies | db/assets.go, db/companies.go | crates/assets | sqlx, serde | IMPLEMENTED (models + counts + query + company CRUD) | TESTED (integration) | gate pending | query_by_company/task by-type not yet wired | — | wire QueryByCompany/QueryByTask into /api/assets; add upsert endpoints |
| guard | guard/ | crates/guard | async-trait | IMPLEMENTED (hooks + audit + traits, async) | TESTED (11 tests) | gate pending | applyIntercept orchestration delegated to interceptor | — | wire into engine |
| interception | intercept/ | crates/interception | sqlx, regex, guard | IMPLEMENTED (rules string/regex/argv, target gate, pending) | TESTED (21 tests) | gate pending | full bash grammar, LLM judge not ported | — | add REST routes for rules/pending |
| egress | egress/ | crates/egress | tokio | IMPLEMENTED (governor: rate/burst/inflight/jitter/coalesce/retry) | TESTED (14 tests) | gate pending | HTTP transport (reqwest) not wired | — | wire into WebFetch/http transport |
| budget | agent/budget.go | crates/budget | — | IMPLEMENTED (task/intent budgets, wrapup reserve) | TESTED (13 tests) | gate pending | interior mutability, effective budget | — | wire into agent runtime |
| traffic | traffic/ (store) | crates/traffic | rusqlite | IMPLEMENTED (blobs + FTS index + search + ingest) | TESTED (7 tests) | gate pending | MITM proxy/TLS not ported | — | build proxy crate on rustls+hyper+rcgen |
| mcp | mcphttp/ | crates/mcp | reqwest, sqlx | IMPLEMENTED (server/tool store + JSON-RPC + Streamable HTTP client) | TESTED (5 tests) | gate pending | legacy SSE transport not ported | — | add /api/mcp routes |
| notifications | notify/ | crates/notifications | reqwest, sqlx | IMPLEMENTED (channels store + filter + render all kinds + HTTP send) | TESTED (8 tests) | gate pending | email SMTP send is a stub; digest scheduler not wired | — | add /api/notify routes + SMTP |
| proxy | traffic/ (MITM) | crates/proxy | rcgen, reqwest | IMPLEMENTED (CA gen/load + leaf certs + capturing HTTP forwarder) | TESTED (3 tests) | gate pending | TLS CONNECT interception, HTTP/2, compression not ported | — | wire into app; full rustls MITM |
| engine | server/engine.go | crates/engine | tokio, exploration | IMPLEMENTED (Planner/Worker traits + step loop + intent dispatch/claim/complete + recover) | TESTED (2 integration) | gate pending | LLM-backed planner/worker (norma SDK) not ported; deadlines/budgets/debounce not wired | — | implement concrete Planner/Worker; wire into app |
| exploration (lifecycle) | db/exploration.go intents | crates/exploration | sqlx | IMPLEMENTED (added claim/set_state/frontier/has_active/has_open_goal/reset/reopen/soft_delete) | TESTED | gate pending | — | — | (done) |
| tasks | db/tasks.go, task_categories, task_templates | crates/tasks | sqlx, serde | IMPLEMENTED (Task + categories + templates + counts) | TESTED (13 tests) | gate pending | task_context hydration, archive, cascade delete not ported | — | wire POST/PATCH/DELETE routes; add archive |
| findings | db/findings.go, finding_retests | crates/findings | sqlx, serde | IMPLEMENTED (Finding + retests + stats + groups) | TESTED (7 tests) | gate pending | archived cold-storage merge, conversation wiring not ported | — | add create/update/delete routes |
| exploration | db/exploration.go (read side) | crates/exploration | sqlx, serde | IMPLEMENTED (graph/nodes/frontier/intents/activity/tokens) | TESTED (5 tests) | gate pending | write/claim/agent path out of scope | — | add more read routes (nodes, frontier, intents) |
| llm | agent/provider.go, llmpool, llmrec, db/config.go | crates/llm | sqlx, serde | IMPLEMENTED (profiles + health + records + usage) | TESTED (9 tests) | gate pending | HTTP streaming, secret sealing not ported | — | add provider CRUD routes |
| settings | db/config.go, db/settings.go | crates/settings | sqlx, serde | IMPLEMENTED (typed getters/setters + stats) | TESTED (5 tests) | gate pending | secret encryption, full stats engine fields | — | add PUT /api/settings |
| exploration | db exploration layer | crates/exploration | database | NOT_ANALYZED | — | — | — | — | Wave 1 |
| traffic | traffic/ | crates/traffic | rusqlite | NOT_ANALYZED | — | — | — | SQLite FTS5 | Wave 6 |
| proxy | traffic/ (MITM) | crates/proxy | rustls, hyper, rcgen | NOT_ANALYZED | — | — | — | go-mitmproxy no equiv | Wave 6 |
| evidence | evidence/ | crates/evidence | sha2 | NOT_ANALYZED | — | — | — | — | Wave 6 |
| findings | db/findings | crates/findings | database | NOT_ANALYZED | — | — | — | — | Wave 6 |
| retester | agent/retester.go | crates/retester | agent | NOT_ANALYZED | — | — | — | — | Wave 6 |
| enrichment | enrich/ | crates/enrichment | hickory-dns, reqwest | NOT_ANALYZED | — | — | — | dnsx no equiv | Wave 6 |
| reporting | report/ | crates/reporting | — | NOT_ANALYZED | — | — | — | — | Wave 6 |
| notifications | notify/ | crates/notifications | reqwest | NOT_ANALYZED | — | — | — | 7 channels | Wave 7 |
| mcp | mcphttp/ | crates/mcp | rmcp/custom | NOT_ANALYZED | — | — | — | Streamable HTTP + SSE | Wave 7 |
| egress | egress/ | crates/egress | governor | NOT_ANALYZED | — | — | — | — | Wave 2 |
| search | settings | crates/search | reqwest | NOT_ANALYZED | — | — | — | brave/tavily | Wave 7 |
| sidequestion | sidequestion/ | crates/agent (sq) | agent | NOT_ANALYZED | — | — | — | — | Wave 4 |
| scheduler | server/scheduler.go | crates/scheduler | tokio | NOT_ANALYZED | — | — | — | — | Wave 8 |
| app | cmd/artex | app/ | all | NOT_ANALYZED | — | — | — | — | Wave 9 |