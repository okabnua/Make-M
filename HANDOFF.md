# HANDOFF — session continuity

## Current state (last updated: Wave 1 + assets/companies crate + API asset/company routes)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 up as `artex-pg`, port 55432, restart unless-stopped), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX cloned read-only at `ARTEX/` @ `5cb44fb` (791 files). Never modify.
- Discovery docs: INVENTORY, ARCHITECTURE, DATABASE-INVENTORY, API-INVENTORY, FEATURE-PARITY, SOURCE-DISPOSITION, ROADMAP, BASELINE, DECISIONS, GLOSSARY, MAP.
- Original test baseline: all packages pass except `server` → 1 flaky pre-existing test (BASELINE.md).
- Wave 0: `crates/config`, `crates/core`.
- Wave 1: `crates/migrations` (schema + fail-closed loader), `crates/database` (open/seed/settings helpers), `crates/auth` (JWT/bcrypt/login-limiter; Go↔Rust interop), `crates/api` (health + auth routes + JWT middleware), `app` binary.
- `crates/assets` (NEW): Asset + Company models, `counts_by_type`, `count_by_type/company/task`, `query_by_type`, `CompanyStore` (get/upsert/list/get_scope/delete); integration test (idempotent).
- `crates/api` routes added: `GET /api/assets`, `GET /api/assets/counts`, `GET /api/companies` — verified end-to-end against Postgres (returns `{count,total,assets}` / company list with asset_count+scope).
- **Parity proofs**: Rust migrates+seeds clean DB → Go db tests pass 35s; Go verifies Rust JWTs; asset/company queries match Go semantics.
- 31 Rust tests pass, clippy 0 warnings, fmt OK.

### Remaining failures
- None in Rust workspace. (Go baseline has 1 flaky server test, pre-existing.)

### Exact next steps
1. `crates/assets`: wire `query_by_company`/`query_by_task` (with pagination) into `/api/assets`; add `POST /api/assets` (insert), `POST/DELETE /api/companies`, `POST /api/companies/{id}/scope`.
2. `crates/api`: add `/api/tasks`, `/api/settings`, `/api/stats` (needs asset counts + engine state) — unblocks the preserved frontend dashboard.
3. Wave 2 security infra: guard, scope, approval, interception (rules/argv/judge), budget, egress.
4. Keep `artex-pg` running (port 55432) for differential tests.

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api,assets}`, `app/`, `tests/`
- Research: `research/rust-anything-analyze.md`, `research/go.mod`, `research/go.sum`
- Docs: see discovery list above