# DECISIONS — architecture decision records

## D-001: Preserve the original ARTEX source read-only
**Status:** Accepted. **Context:** PHASE-0 rule 1. The Rust rebuild lives at the `Make-M` root; `ARTEX/` is never modified. The original is the behavioral reference until cutover.

## D-002: Modular Cargo workspace, not a single crate
**Status:** Accepted. **Context:** ARTEX has 17 Go packages with distinct responsibilities and a real dependency graph (analyze waves). A workspace (`crates/*` + `app/`) mirrors this; crates are split only where the dependency graph justifies it. `core` holds shared domain enums.

## D-003: `config` port uses `url` crate + serde, no FFI
**Status:** Accepted. **Context:** `config/` is a leaf with readiness 81 and no exported FFI surface; porting it as a native crate + unit/differential tests is simpler and more faithful than a cgo bridge. DSN assembly reproduces Go's `net/url` encoding byte-for-byte (verified).

## D-004: Postgres via sqlx, schema embedded with sqlx::migrate!
**Status:** Proposed (Wave 1). **Context:** Replaces `db/` (pgx). Keeping table/column names, types, constraints, and indexes identical preserves DB compatibility during transition. Original `schema.sql` + versioned `migrations/` ported verbatim.

## D-005: async with tokio, HTTP with axum
**Status:** Proposed (Wave 8). **Context:** Standard, well-supported; matches the long-lived SSE streams and many concurrent workers/goroutines.

## D-006: norma SDK has no Rust equivalent — biggest design decision
**Status:** Open. **Context:** The agent runtime (agentcore/harness/tool/llm/permission/transcript/compaction) is the `norma` Go SDK. No Rust port exists. We must design our own agent runtime that reproduces observable behavior (tool dispatch, harness hooks, transcript, compaction, permission decisions). This gates Waves 4–5. Research file: `research/github.com-Autumn-27-norma.md` (to write).

## D-007: MITM proxy rebuilt on rustls + hyper + rcgen
**Status:** Proposed (Wave 6). **Context:** Replaces `go-mitmproxy`. CA generation via `rcgen`; TLS interception via `rustls`; HTTP via `hyper`. Traffic store index via `rusqlite` (FTS5) matching the original SQLite schema.

## D-008: Preserved presentation layer
**Status:** Accepted. **Context:** `web/` (Next.js static export) and `extension/` are preserved, not rewritten. They depend only on `/api/*` + SSE. Documented as presentation layer, not a Rust backend dependency. Browser extension talks to `POST /api/traffic/ingest`.

## D-009: Regex via fancy-regex where Go patterns use lookaround/backrefs
**Status:** Proposed (Wave 2). **Context:** `intercept/` uses 2 regex features the `regex` crate lacks. Use `fancy-regex` for those patterns and test on non-ASCII (case-insensitive `ſ`/`K` differences).