# GLOSSARY — ARTEX domain terms

| Term | Meaning |
| --- | --- |
| Asset graph | Global, shared source-of-truth graph of assets (root_domain/subdomain/ip/service/app/endpoint) owned by companies. |
| Exploration graph | Per-task graph of the reasoning/advance process (goal/intent/fact/finding/hint/digest) with lineage edges. |
| Anchor | `exploration_anchors(node_id, asset_id)` linking an exploration node to a concrete asset. |
| Intent | A planner-generated exploration direction; a worker claims one intent to execute. |
| Frontier | The queue of pending intents awaiting a worker. |
| Fact | A worker's conclusion (incl. negative result), tied to its intent. |
| Finding | A confirmed vulnerability (`report_finding`), distinct from a fact. |
| Digest | A lossless compressed fold of cold intents/facts; restorable by id. |
| Planner | The sole intent generator; keeps a shared per-task todolist across wake-ups. |
| Worker | Executes one claimed intent with real tools, writes back to the graphs. |
| Main agent | Human-in-the-loop orchestrator; observes and injects hints/intents. |
| Retester | Agent that re-tests findings; verdict reproduced/fixed/inconclusive. |
| Side question (`/btw`) | Independent side query with immutable structured snapshots; does not own the main session. |
| Noise governor | `egress` rate/burst/concurrency/cache limiter for platform-owned target requests. |
| Target gate | Approval of host:port targets (`target_pins`) before a tool may contact them. |
| Interception rule | User rule (string/regex/argv) matching tool calls → allow/deny/ask. |
| argv rule | Prefix rule (JSON token array) for Bash commands, e.g. `["git","push"]`. |
| Budget | Token/turn caps at task/intent/round level; drives forced wrap-up. |
| Compaction | Cold-node digest compaction (maintain/minor/major) to keep context bounded. |
| Untrusted boundary | Nonce-tagged framing so tool results are data, not instructions (prompt-injection defense). |
| LLM profile | A configured provider+model+limits record (`llm_profiles`). |
| LLM pool | Round-robin failover across profiles with a circuit breaker. |
| Evidence | Content-addressed blob (SHA-256) preserving finding proof, independent of disposable traffic. |
| Coverage | Asset test coverage ledger / coverage graph (in-scope assets + tested highlights). |
| MITM proxy | Recording proxy (TLS interception) that captures every target HTTP exchange to a browsable store. |
| Skills | Reusable playbooks (`SKILL.md` + files) available to agents. |
| ScopeSentry | External asset source that syncs targets into the asset graph. |
| RSI lessons | Positive/negative/caution lessons injected (shadow/inject) to guide agents. |
| Setup token | One-time token printed at first start, required to claim the admin password on `/setup`. |
| Front door | rust-anything whole-app mode that puts a Rust handler in front of the original for differential capture. |