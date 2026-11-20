# BASELINE — original ARTEX build & test results

Environment: Go 1.27.3, Postgres 16 (Docker `artex-pg`, DSN `postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable`), Node 24, rust-anything 2.1.0.

## Build
- `go build ./config/...` OK. Full module fetches from proxy (norma SDK `github.com/BaaRai/Norma v0.4.8`, pgx, etc.).
- Full binary build not attempted yet (needs `web/out` embedded via `-tags embedui`).

## Test suite (`go test ./...` with Postgres)
1122 test functions across packages. Result:

| Package | Result |
| --- | --- |
| agent, cmd/artex, config, db, egress, evidence, guard, intercept, llmpool, llmrec, mcphttp, notify, report, sidequestion, traffic | ok |
| server | **FAIL** |

### Known baseline failure (pre-existing, in original source)
`TestDecodeEvidenceBodyGzipQuoteFound` in `server/finding_verification_decode_test.go`.

- Cause: the test's precondition `if strings.Contains(string(raw), "admin") { t.Fatal("fixture is not compressed") }`. Go 1.27's `compress/gzip` emits a *stored* (uncompressed) deflate block for the short fixture, so the gzip bytes still contain the literal `admin`. The assertion that gzip always compresses a tiny input is false.
- This is a **flaky test in the original repo**, unrelated to the migration. Reproduces 3/3 runs.
- **We must not modify ARTEX.** For the Rust port, `decodeEvidenceBody` must decompress gzip correctly regardless; we will add our own recorded cases rather than depend on this test's precondition.

## Baseline established
- Original behavior reference captured: `ARTEX/` @ `5cb44fb` (read-only).
- Postgres available for differential testing.
- This failure is the single deviation from green; everything else passes.