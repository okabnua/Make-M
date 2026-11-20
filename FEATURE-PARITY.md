# FEATURE-PARITY — ARTEX feature matrix

Every original feature with a stable id, source reference, and migration status. Status legend: `NOT_ANALYZED` `ANALYZED` `PLANNED` `IN_PROGRESS` `IMPLEMENTED` `TESTED` `VERIFIED` `BLOCKED`.

This matrix is the master parity checklist (PHASE-01 §3). No feature may silently disappear. Rows move ANALYZED → PLANNED → … → VERIFIED as the migration proceeds; nothing is marked VERIFIED without recorded evidence + passing gate.

## A. Autonomous agent engine
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| A1 | Main agent (human-in-loop orchestrator) | agent/mainagent.go | ANALYZED |
| A2 | Planner (sole intent generator) | agent/planner.go | ANALYZED |
| A3 | Worker agents (claim one intent) | agent/worker.go | ANALYZED |
| A4 | Goal decomposition | agent/goals.go | ANALYZED |
| A5 | Intent create / dispatch / claim | server/engine.go | ANALYZED |
| A6 | Worker lifecycle | server/engine.go | ANALYZED |
| A7 | Task lifecycle | server/manager.go, db/tasks | ANALYZED |
| A8 | Autonomous exploration loop | server/engine.go | ANALYZED |
| A9 | Goal evaluation / completion | agent/planner.go (prove_goal) | ANALYZED |
| A10 | Task resumption / recovery | server/manager.go (restoreTaskRuntimes) | ANALYZED |
| A11 | Cross-worker trace search | agent/tools.go (search_all_worker_traces) | ANALYZED |
| A12 | Shared per-task planner todolist | agent/planner.go (TodoStore) | ANALYZED |
| A13 | Facts / hints / derived info | agent/tools.go (record_fact, add_hint) | ANALYZED |
| A14 | Error recovery / retry | agent/capture.go, budget.go | ANALYZED |
| A15 | Cancellation / timeout | agent/taskclock.go, cancelcause.go | ANALYZED |

## B. Dual graph
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| B1 | Asset graph (shared, global) | db/schema assets, server asset routes | ANALYZED |
| B2 | Exploration graph (per-task) | db/schema exploration_* | ANALYZED |
| B3 | Node/edge/anchor semantics | db/constants.go | ANALYZED |
| B4 | Asset parent-child + dedup | db asset layer | ANALYZED |
| B5 | Asset ownership (companies) | db/schema companies | ANALYZED |
| B6 | Coverage / coverage graph | agent/coverage_ledger.go, server coverage routes | ANALYZED |
| B7 | Graph traversal / queries | server exploration handlers | ANALYZED |

## C. LLM infrastructure
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| C1 | Provider abstraction | agent/provider.go | ANALYZED |
| C2 | Anthropic | agent/provider.go | ANALYZED |
| C3 | OpenAI | agent/provider.go | ANALYZED |
| C4 | OpenAI-responses | agent/provider.go | ANALYZED |
| C5 | Ollama (keyless) | agent/provider.go | ANALYZED |
| C6 | API key mgmt / profiles | db/llm_profiles, server/llm | ANALYZED |
| C7 | Provider rotation / failover | llmpool/pool.go | ANALYZED |
| C8 | Circuit breaker / health | llmpool/health.go, db/llm_profile_health | ANALYZED |
| C9 | Retry / timeout policies | db/llm_profiles retry_* | ANALYZED |
| C10 | Streaming | llm provider iter.Seq2 | ANALYZED |
| C11 | Token accounting / usage records | llmrec/, db/llm_usage, llm_records | ANALYZED |
| C12 | Per-task / per-worker config | db/task_llm_profiles | ANALYZED |

## D. Security tool execution
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| D1 | Tool registry | agent/toolcatalog.go, db/tools | ANALYZED |
| D2 | Bash execution | worker + norma harness | ANALYZED |
| D3 | HTTP tools / WebFetch | agent/webfetch.go | ANALYZED |
| D4 | Scanner integrations | agent/scan_ingest.go, bbot_ingest.go | ANALYZED |
| D5 | Arg validation / output capture | norma tool | ANALYZED |
| D6 | Timeout / cancellation | agent/taskclock.go | ANALYZED |
| D7 | Tool result normalization | agent/capture.go | ANALYZED |
| D8 | Untrusted-output framing | agent/untrusted_boundary.go | ANALYZED |

## E. Security & safety controls
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| E1 | Target scope enforcement | intercept/ (target gate) | ANALYZED |
| E2 | Deny rules | guard/, intercept/ | ANALYZED |
| E3 | Human approval (ask/decide) | intercept/intercept.go, pending | ANALYZED |
| E4 | Interception rules (string/regex/argv) | intercept/ | ANALYZED |
| E5 | Token / turn / intent budgets | agent/budget.go | ANALYZED |
| E6 | Noise governor (pace/burst/cache) | egress/ | ANALYZED |
| E7 | Prompt-injection boundaries | agent/untrusted_prompt.go | ANALYZED |
| E8 | Secret encryption at rest | db/secrets.go (AES-GCM) | ANALYZED |
| E9 | Audit trail | guard/ audit ring, db server_logs | ANALYZED |

## F. Traffic recording / MITM proxy
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| F1 | Proxy listener + CA gen | traffic/ (go-mitmproxy) | ANALYZED |
| F2 | Request/response capture | traffic/ | ANALYZED |
| F3 | Compression decode | traffic/ (klauspost/compress) | ANALYZED |
| F4 | SQLite FTS index + search | traffic/ | ANALYZED |
| F5 | Traffic persistence / blobs | traffic/ | ANALYZED |
| F6 | Traffic retrieval / replay | server/traffic routes | ANALYZED |
| F7 | Browser-extension ingest | traffic/external.go, server/traffic_ingest.go | ANALYZED |
| F8 | Evidence snapshots | evidence/ | ANALYZED |

## G. Browser extension
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| G1 | Manifest / background SW | extension/ | ANALYZED |
| G2 | Popup UI + config | extension/ | ANALYZED |
| G3 | Debugger-driven capture | extension/background.js | ANALYZED |
| G4 | Batch upload to /api/traffic/ingest | extension/background.js | ANALYZED |

## H. Findings & evidence
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| H1 | Finding create / update / classify | db/findings, agent tools | ANALYZED |
| H2 | Severity / confidence / verification | db/findings | ANALYZED |
| H3 | Evidence quotes / traffic assoc | evidence/, finding_traffic_bindings | ANALYZED |
| H4 | Finding history / lineage | exploration graph | ANALYZED |
| H5 | Bulk status / status transitions | server findings handlers | ANALYZED |

## I. Retesting
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| I1 | Retester agent | agent/retester.go | ANALYZED |
| I2 | Retest runs + verdicts | db/finding_retests | ANALYZED |
| I3 | Auto status transition on Fixed | server | ANALYZED |

## J. Asset discovery / enrichment
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| J1 | Root/subdomain/ip/port/service/app/endpoint | db/assets | ANALYZED |
| J2 | Asset dedup / relationships | db | ANALYZED |
| J3 | Enrichment (DNS/HTTP) | enrich/ | ANALYZED |
| J4 | Coverage tracking | coverage_ledger | ANALYZED |
| J5 | ScopeSentry sync | server sync routes | ANALYZED |

## K. MCP / agent extensions
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| K1 | MCP Streamable HTTP | mcphttp/client.go | ANALYZED |
| K2 | MCP legacy SSE | mcphttp/client.go | ANALYZED |
| K3 | JSON-RPC / tool registration | mcphttp/ | ANALYZED |
| K4 | Skills | skills/, server skills routes | ANALYZED |
| K5 | Memory / workspace | sidequestion memory, workspace routes | ANALYZED |

## L. Reporting / export
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| L1 | Finding/evidence reports | report/ | ANALYZED |
| L2 | Task archives | server archive worker | ANALYZED |
| L3 | Export formats (md/csv/json/zip) | report/, server | ANALYZED |

## M. Notifications / supporting
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| M1 | IM/email channels | notify/ | ANALYZED |
| M2 | Event notifications / deliveries | notify/, db notification_* | ANALYZED |
| M3 | Backend logs | server log sink | ANALYZED |
| M4 | Side questions (`/btw`) | sidequestion/ | ANALYZED |
| M5 | Web search (Brave/Tavily) | settings + agent | ANALYZED |
| M6 | Egress proxy | egress/ | ANALYZED |

## N. Web interface (preserved)
| ID | Feature | Status |
| --- | --- | --- |
| N1 | 27 pages (dashboard/tasks/findings/assets/traffic/chat/system) | ANALYZED — preserved |
| N2 | SSE consumers (logs/activity/side-questions) | ANALYZED |
| N3 | API client (web/src/lib/api.ts) | ANALYZED |
| N4 | Auth gate (token/cookie) | ANALYZED |

## O. Deployment / ops
| ID | Feature | Source | Status |
| --- | --- | --- | --- |
| O1 | Docker + Compose | Dockerfile, compose | ANALYZED |
| O2 | Install/start/update scripts | *.sh, start.bat | ANALYZED |
| O3 | Setup wizard / first-run token | server/auth.go | ANALYZED |
| O4 | Config (config.json/env) | config/ | ANALYZED |
| O5 | Backup/restore | scripts/backup.sh | ANALYZED |
| O6 | Schema migration on start | db/migrate.go | ANALYZED |
| O7 | Cross-platform builds / release | build.sh, .github | ANALYZED |
| O8 | Health check | /api/health | ANALYZED |

## Retest / retesting additions (PHASE-02 §B)
| ID | Feature | Status |
| --- | --- | --- |
| P1 | First-run setup token + admin claim | ANALYZED |
| P2 | Login rate limiting + trusted proxies | ANALYZED |
| P3 | Company/user ownership | ANALYZED |
| P4 | Task categories/templates/relations | ANALYZED |
| P5 | Judge + compactor LLM roles | ANALYZED |
| P6 | Provider exclusion/rotation semantics | ANALYZED |
| P7 | Forced wrap-up / resumed tasks | ANALYZED |
| P8 | Scanner low-and-slow guidance | ANALYZED |
| P9 | Generated MITM CA lifecycle | ANALYZED |
| P10 | Encrypted traffic snapshots | ANALYZED |
| P11 | Reporting proof-strength semantics | ANALYZED |
| P12 | Release checksums/SBOM/provenance | ANALYZED |

## Status tracking rule
A feature advances only with evidence. For each feature, when ported: add a `rust/crates/<crate>` reference + a test/recorded-case id + gate result. The `MAP.md` (created during migration) is the per-module live view; this file is the per-feature view.