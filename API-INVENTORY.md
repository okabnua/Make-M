# API-INVENTORY — ARTEX HTTP API surface

Source: `ARTEX/server/server.go` (`Handler()` + `registerSideRoutes` + `registerFindingTraffic`) and route files. All routes under `/api`; non-`/api` paths serve the embedded UI. Auth: JWT (HS256, `Authorization: Bearer` | `artex_token` cookie | `?token=`). Exempt: `/api/auth/*`, `/api/health`. CORS `*`; `OPTIONS` → 204.

Rust target: `crates/api` (axum Router) must reproduce every method+path, request/response JSON shape, status codes, and SSE streams. This is the compatibility contract the preserved Next.js frontend and browser extension depend on.

## Auth / system
- `GET /api/auth/status`, `POST /api/auth/init`, `POST /api/auth/login`, `POST /api/auth/change-password`
- `GET /api/health`, `GET /api/stats`
- `GET /api/logs`, `GET /api/logs/history`, `GET /api/logs/stream` (SSE)
- `GET /api/audit`, `POST /api/gc`, `GET /api/settings`, `PUT /api/settings`, `POST /api/active`

## Tasks
- `GET /api/tasks`, `POST /api/tasks`, `GET /api/tasks/{id}`, `PATCH /api/tasks/{id}`, `DELETE /api/tasks/{id}`
- `GET /api/tasks/{id}/outcome`, `/lessons`, `POST /api/tasks/{id}/lessons/extract`
- `PATCH /api/tasks/{id}/category`, `GET /api/task-categories`, `POST /api/task-categories`, `PATCH/DELETE /api/task-categories/{id}`, `POST /api/tasks/category/batch`
- `GET /api/task-templates`, `POST /api/task-templates`, `PATCH/DELETE /api/task-templates/{id}`
- `POST /api/tasks/{id}/control`, `POST /api/tasks/control/batch`
- `PUT /api/tasks/{id}/llm`, `GET /api/tasks/{id}/llm/resolution`
- `POST /api/tasks/{id}/intents/{iid}/control`, `.../messages`, `.../rerun`, `POST /api/tasks/{id}/intents/rerun-blocked`
- `GET /api/tasks/{id}/credentials`, `POST …/credentials`, `DELETE …/credentials/{cid}`
- `GET /api/tasks/{id}/scope`, `POST …/scope`, `DELETE …/scope/{sid}`
- `GET/POST /api/tasks/{id}/goals`, `PATCH/DELETE …/goals/{gid}`
- `GET/POST /api/tasks/{id}/constraints`, `PATCH/DELETE …/constraints/{cid}`
- `GET /api/tasks/{id}/coverage`, `/coverage-graph`, `/asset-refs`, `/intent-assets`
- `POST /api/tasks/{id}/assets`, `DELETE …/assets/{assetID}`
- `GET/POST /api/tasks/{id}/intercept-rules`, `PUT/DELETE …/{rid}`, `POST …/{rid}/toggle`
- `GET /api/tasks/{id}/target-pins`, `POST …/target-pins/{pid}/revoke`
- `GET/POST /api/tasks/{id}/chat/status|stop`
- Archives: `GET /api/task-archives`, `GET /api/task-archives/{id}`, `POST /api/tasks/{id}/archive`, `POST /api/tasks/archive/batch`, `POST /api/task-archives/{id}/restore`, `POST /api/task-archives/restore/batch`, `DELETE /api/task-archives/{id}`, `POST /api/task-archives/delete/batch`

## Exploration / findings
- `GET /api/exploration/frontier`, `/intents`, `/graph`, `/nodes`
- `GET /api/exploration/findings`, `/findings/groups`, `/findings/asset-tree`, `/findings/stats`, `/findings/export`
- `GET /api/exploration/findings/{id}`, `PATCH/DELETE …/{id}`, `.../lineage`, `POST …/deepen`
- `GET/POST /api/exploration/findings/{id}/retests`, `GET /api/exploration/findings/retests/active`
- Finding traffic: `GET/POST …/findings/{id}/traffic`, `PATCH/DELETE …/traffic/{binding_id}`, `PUT …/traffic/order`, `GET …/traffic/{binding_id}`, `GET …/traffic/{binding_id}/body`
- Activity: `GET /api/exploration/activity`, `/activity/history`, `/activity/stream` (SSE), `/activity/{seq}`
- `GET /api/exploration/main-sessions`, `POST /api/exploration/main-session/new`
- Tokens: `GET /api/exploration/tokens`, `/api/tokens/daily`, `/api/tokens/conversations`, `/api/tokens/usage`

## Assets / companies
- `GET /api/assets`, `/assets/counts`, `POST /api/assets`, `DELETE /api/assets`
- `GET/POST /api/companies`, `GET/DELETE /api/companies/{id}`, `POST /api/companies/{id}/scope`, `POST /api/companies/reattribute`

## Traffic
- `GET /api/traffic`, `/traffic/hosts`, `/traffic/exchange`, `/traffic/blob`, `DELETE /api/traffic`, `/traffic/hosts`, `/traffic/all`, `POST /api/traffic/ingest` (browser extension)

## LLM / profiles / pool
- `GET/POST /api/llm`, `POST /api/llm/test`
- `GET/POST /api/llm/profiles`, `DELETE /api/llm/profiles/{id}`, `POST /api/llm/profiles/active`
- `GET/POST /api/llm/retry-policy`, `GET /api/llm/pool`, `POST /api/llm/pool/reset`, `POST /api/llm/models`
- `GET/DELETE /api/llm/records`, `GET /api/llm/records/tasks`, `/records/by-model`, `/records/{id}`
- `GET /api/commands`, `/commands/stats`

## Agents / conversations / chat
- `GET/POST /api/agents`, `GET/PATCH/DELETE /api/agents/{key}`
- `PUT /api/agents/{key}/config`, `/prompt`, `POST …/prompt/reset`, `/prompt/preview`
- `PUT /api/agents/{key}/wrapup`, `POST …/wrapup/reset`, `/wrapup/task-timeout` (+ reset)
- `GET/POST /api/agents/{key}/triggers`, `PATCH/DELETE /api/triggers/{id}`
- `GET /api/agents/{key}/prompts`, `/variables`, `/visibility`, `PUT …/visibility`
- `GET/POST /api/conversations`, `POST /api/conversations/delete/batch`, `PATCH/DELETE /api/conversations/{id}`, `PATCH …/profile`
- `GET/POST /api/conversations/{id}/messages`, `POST …/stop`, `GET …/messages/{seq}`
- `GET /api/conversations/{id}/target-pins`, `POST …/target-pins/{pid}/revoke`
- `GET /api/chat/mentions`, `POST /api/chat`, `POST /api/chat/upload`
- Side questions: `GET/POST/DELETE /api/conversations/{id}/side-questions`, `/api/tasks/{id}/chat/side-questions`, `/api/tasks/{id}/intents/{iid}/side-questions`; `GET /api/side-questions/{requestID}/events` (SSE), `POST …/cancel`

## Tools / MCP / skills / sync
- `GET /api/tools`, `PUT /api/tools/{key}`, `POST /api/tools/{key}/reset`
- `POST /api/tools/custom`, `/custom/test`, `PUT/DELETE /api/tools/custom/{key}`
- `GET/POST /api/mcp`, `DELETE /api/mcp/{id}`, `GET /api/mcp/{id}/tools`, `POST /api/mcp/{id}/refresh`
- `GET/POST /api/skills`, `POST /api/skills/upload`, `DELETE /api/skills/{name}`, `GET /api/skills/missing`, `GET /api/skills/{name}/usage`, `PUT /api/skills/{name}/meta`
- `POST /api/skills/{name}/dirs`, `GET /api/skills/{name}/files`, `GET/PUT/DELETE /api/skills/{name}/files/{file...}`
- `GET /api/visibility/{kind}/{id}`, `POST /api/visibility/toggle`, `GET /api/visibility/skill/{name}`, `POST /api/visibility/skill/toggle`
- `GET /api/skill-gaps`
- ScopeSentry: `GET /api/sync/scopesentry/status`, `POST …/datasource`, `GET …/projects`, `GET …/tasks`, `POST …/sync`
- `POST /api/settings/python/detect`, `POST /api/settings/web-search/test`

## Intercept / target gate
- `GET/POST /api/intercept/rules`, `PUT/DELETE …/{id}`, `POST …/{id}/toggle`
- `GET/POST /api/asset-intercept/rules`, `PUT/DELETE …/{id}`, `POST …/{id}/toggle`
- `GET /api/intercept/pending`, `GET …/pending/{id}`, `POST …/pending/{id}/decide`, `POST …/pending/{id}/always-allow`
- `GET /api/intercept/history`, `/history/{id}`, `/history/{id}/execution`, `/intercept/task/{taskID}`
- `GET/PUT /api/intercept/tool-config`
- `GET/PUT /api/target-gate/exemptions`, `GET /api/target-gate/exemptions/history`
- `GET/PUT /api/intercept/judge`, `GET /api/intercept/judge/usage`

## Notify / report
- `GET /api/notify/meta`, `GET/POST /api/notify/channels`, `PATCH/DELETE …/{id}`, `POST …/{id}/test`
- `GET /api/notify/deliveries`, `POST …/{id}/retry`
- `GET /api/report`

## Workspace
- `GET /api/workspace/list`, `/workspace/read`, `/workspace/download`, `POST /workspace/write`, `/workspace/mkdir`, `/workspace/upload`, `DELETE /workspace/delete`

## SSE streams
- `GET /api/logs/stream` — backend log tail (`?since=`), 20s ping
- `GET /api/exploration/activity/stream` — per-task activity (`?task=&since=&intent=`), Last-Event-ID resume
- `GET /api/side-questions/{requestID}/events` — side-question snapshots

## Contract notes (must preserve)
- Error body shape: `{"error": "<string>"}` on non-2xx.
- 204 No Content for some deletes.
- JWT in `localStorage["artex_token"]` + cookie; `?token=` accepted for SSE.
- Trailing-slash route URLs (static export).
- `GET /api/health` returns `{ok, service, version, latest_version?}` — the only non-auth endpoint besides `/api/auth/*`.