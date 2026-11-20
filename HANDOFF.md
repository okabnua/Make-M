# HANDOFF — session continuity

## Current state (last updated: Wave 0–1 done, API auth slice + app binary)

### Completed
- Environment: Go 1.27.3, Node 24, Docker (Postgres 16 up as `artex-pg`, port 55432, restart unless-stopped), rust-anything 2.1.0, Rust 1.100.0.
- ARTEX cloned read-only at `ARTEX/` @ `5cb44fb` (791 files). Never modify.
- rust-anything `analyze --whole-app` → `research/rust-anything-analyze.md` (17 units, waves 0–6).
- Discovery docs: INVENTORY, ARCHITECTURE, DATABASE-INVENTORY, API-INVENTORY, FEATURE-PARITY, SOURCE-DISPOSITION, ROADMAP, BASELINE, DECISIONS, GLOSSARY, MAP.
- Original test baseline: all packages pass except `server` → 1 flaky pre-existing test (BASELINE.md).
- Wave 0: `crates/config` (full port, 6 tests + Go differential byte-identical), `crates/core` (constants.go enums, 1 test).
- Wave 1: `crates/migrations` (schema.sql + 0001/0002 verbatim + loader/validator, 7 tests), `crates/database` (open/ensure_database/apply_schema/apply_migrations, advisory lock, **full builtin seed**: 6 agents, 8 prompt vars, 20 intercept rules, 4 asset intercept rules, browser MCP, skill visibility, target-gate exemptions; settings helpers), `crates/auth` (JWT HS256, bcrypt, password validation, per-IP login limiter, trusted proxies, setup token; 8 tests + **Go↔Rust JWT interop verified**), `crates/api` (axum router: health + auth status/init/login/change-password + JWT middleware; **e2e HTTP test + Go verifies Rust token**), `app` (tokio main binary, serve + graceful shutdown).
- **Parity proofs**: Rust `open()` migrates+seeds clean DB → Go `go test ./db/` passes 35s; Go verifies Rust-issued JWTs and vice versa.
- 29 Rust tests pass, clippy 0 warnings, fmt OK.

### Remaining failures
- None in Rust workspace. (Go baseline has 1 flaky server test, pre-existing.)

### Exact next steps
1. `crates/database` typed repos: assets, companies, tasks, explorations (Wave 1 completion).
2. `crates/api`: add remaining routes from API-INVENTORY.md in dependency order (tasks, assets, companies, exploration, findings, traffic, settings, agents, llm, etc.).
3. Wave 2 security infra: guard, scope, approval, interception (rules/argv/judge), budget, egress.
4. Keep `artex-pg` running (port 55432) for differential tests.

## Key reference paths
- Original: `ARTEX/` (read-only)
- Rust: `crates/{config,core,migrations,database,auth,api}`, `app/`, `tests/`
- Research: `research/rust-anything-analyze.md`, `research/go.mod`, `research/go.sum`
- Docs: see discovery list above