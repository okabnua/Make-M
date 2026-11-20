# SOURCE-DISPOSITION — every tracked file of ARTEX

Source: `ARTEX/` @ 5cb44fb — 791 tracked files. This is the complete accounting required by PHASE-02 §F.

Legend for **Final runtime**: does the file (or its Rust equivalent) remain in the running product?

## Category summary

| Category | Count | Rust equivalent | In final repo | In final runtime | Notes |
| --- | ---: | --- | --- | --- | --- |
| Test | 265 | Rust tests + differential cases | Test-only; not shipped | Ported assertions to Rust tests + recorded cases |
| Runtime source (Go) | 217 | Rust crate under crates/ | No — replaced by Rust at cutover | Ported to Rust; behavior verified via rust-anything gate |
| Frontend source (Next.js) | 209 | Preserved as presentation layer | Yes (static export embedded) | Documented as preserved UI, not a Rust backend dependency |
| Runtime asset (skill) | 28 | Preserved (copied to Rust data dir) | Yes | Skill files are data, copied unchanged |
| Build/ops tooling | 19 | Rust build scripts / Dockerfile | Partial | Replaced where Rust build differs |
| Documentation (screenshot) | 14 | Preserved (docs) | Docs only | Reference material |
| Documentation | 9 | Preserved (docs) | Docs only | Reference material |
| Startup/install script | 7 | Rust start.sh / install.sh | Yes (rewritten for Rust) | Rewritten to launch Rust binary |
| Configuration | 5 | Rust config + example | Yes | config.json/env preserved |
| Deployment (Docker) | 4 | Rust Dockerfile/compose | Yes | Rebuilt for Rust binary + postgres |
| CI/CD | 4 | Rust CI | Partial | Rebuilt for Rust toolchain |
| Browser extension | 4 | Preserved as-is | Yes | Unchanged; /api/traffic/ingest contract preserved |
| Other | 3 | Review | TBD | Classified individually |
| Database schema/migration | 3 | crates/db/migrations | Yes (embedded) | Ported verbatim, embedded via sqlx migrate |

## Full file disposition

| # | Original path | Category | Rust equivalent / disposition |
| ---: | --- | --- | --- |
| 1 | `.dockerignore` | Deployment (Docker) | Rust Dockerfile/compose |
| 2 | `.env.example` | Configuration | Rust config + example |
| 3 | `.gitattributes` | Other | Review |
| 4 | `.github/scripts/bot.sh` | CI/CD | Rust CI |
| 5 | `.github/workflows/bot.yml` | CI/CD | Rust CI |
| 6 | `.github/workflows/ci.yml` | CI/CD | Rust CI |
| 7 | `.github/workflows/release.yml` | CI/CD | Rust CI |
| 8 | `.gitignore` | Other | Review |
| 9 | `CHANGELOG.md` | Documentation | Preserved (docs) |
| 10 | `Dockerfile` | Deployment (Docker) | Rust Dockerfile/compose |
| 11 | `LICENSE` | Other | Review |
| 12 | `README.md` | Documentation | Preserved (docs) |
| 13 | `RSI.md` | Documentation | Preserved (docs) |
| 14 | `agent/adversarial_gate_seed_test.go` | Test | Rust tests + differential cases |
| 15 | `agent/agentic_guards_test.go` | Test | Rust tests + differential cases |
| 16 | `agent/assembly.go` | Runtime source (Go) | Rust crate under crates/ |
| 17 | `agent/auto_scope_hygiene_test.go` | Test | Rust tests + differential cases |
| 18 | `agent/bbot_ingest.go` | Runtime source (Go) | Rust crate under crates/ |
| 19 | `agent/bbot_ingest_test.go` | Test | Rust tests + differential cases |
| 20 | `agent/bbot_ingest_tool.go` | Runtime source (Go) | Rust crate under crates/ |
| 21 | `agent/bbot_ingest_tool_pg_test.go` | Test | Rust tests + differential cases |
| 22 | `agent/bbot_ingest_tool_test.go` | Test | Rust tests + differential cases |
| 23 | `agent/blackboard_inheritance_test.go` | Test | Rust tests + differential cases |
| 24 | `agent/budget.go` | Runtime source (Go) | Rust crate under crates/ |
| 25 | `agent/budget_test.go` | Test | Rust tests + differential cases |
| 26 | `agent/cancelcause.go` | Runtime source (Go) | Rust crate under crates/ |
| 27 | `agent/capture.go` | Runtime source (Go) | Rust crate under crates/ |
| 28 | `agent/capture_approval_test.go` | Test | Rust tests + differential cases |
| 29 | `agent/capture_usage_test.go` | Test | Rust tests + differential cases |
| 30 | `agent/chat.go` | Runtime source (Go) | Rust crate under crates/ |
| 31 | `agent/coldgraph.go` | Runtime source (Go) | Rust crate under crates/ |
| 32 | `agent/coldgraph_test.go` | Test | Rust tests + differential cases |
| 33 | `agent/compaction.go` | Runtime source (Go) | Rust crate under crates/ |
| 34 | `agent/constraints.go` | Runtime source (Go) | Rust crate under crates/ |
| 35 | `agent/coverage_ledger.go` | Runtime source (Go) | Rust crate under crates/ |
| 36 | `agent/coverage_ledger_test.go` | Test | Rust tests + differential cases |
| 37 | `agent/data_envelope.go` | Runtime source (Go) | Rust crate under crates/ |
| 38 | `agent/data_envelope_test.go` | Test | Rust tests + differential cases |
| 39 | `agent/deadline_contract_seed_test.go` | Test | Rust tests + differential cases |
| 40 | `agent/deferred.go` | Runtime source (Go) | Rust crate under crates/ |
| 41 | `agent/deferred_test.go` | Test | Rust tests + differential cases |
| 42 | `agent/fact_provenance_test.go` | Test | Rust tests + differential cases |
| 43 | `agent/finding_dedup_seed_test.go` | Test | Rust tests + differential cases |
| 44 | `agent/finding_quality_seed_test.go` | Test | Rust tests + differential cases |
| 45 | `agent/finding_reasoning_test.go` | Test | Rust tests + differential cases |
| 46 | `agent/finding_recorder.go` | Runtime source (Go) | Rust crate under crates/ |
| 47 | `agent/finding_recorder_test.go` | Test | Rust tests + differential cases |
| 48 | `agent/finding_workflow.go` | Runtime source (Go) | Rust crate under crates/ |
| 49 | `agent/finding_workflow_test.go` | Test | Rust tests + differential cases |
| 50 | `agent/goals.go` | Runtime source (Go) | Rust crate under crates/ |
| 51 | `agent/hooks_reach_test.go` | Test | Rust tests + differential cases |
| 52 | `agent/insert_assets_test.go` | Test | Rust tests + differential cases |
| 53 | `agent/mainagent.go` | Runtime source (Go) | Rust crate under crates/ |
| 54 | `agent/noa.go` | Runtime source (Go) | Rust crate under crates/ |
| 55 | `agent/noise_profile.go` | Runtime source (Go) | Rust crate under crates/ |
| 56 | `agent/noise_profile_test.go` | Test | Rust tests + differential cases |
| 57 | `agent/operator_handoff_seed_test.go` | Test | Rust tests + differential cases |
| 58 | `agent/planner.go` | Runtime source (Go) | Rust crate under crates/ |
| 59 | `agent/prompt.go` | Runtime source (Go) | Rust crate under crates/ |
| 60 | `agent/prompt_injection_seed_test.go` | Test | Rust tests + differential cases |
| 61 | `agent/prompt_now_test.go` | Test | Rust tests + differential cases |
| 62 | `agent/prompt_test.go` | Test | Rust tests + differential cases |
| 63 | `agent/promptcatalog.go` | Runtime source (Go) | Rust crate under crates/ |
| 64 | `agent/provider.go` | Runtime source (Go) | Rust crate under crates/ |
| 65 | `agent/provider_capture_e2e_test.go` | Test | Rust tests + differential cases |
| 66 | `agent/provider_capture_test.go` | Test | Rust tests + differential cases |
| 67 | `agent/provider_env_test.go` | Test | Rust tests + differential cases |
| 68 | `agent/provider_quota_test.go` | Test | Rust tests + differential cases |
| 69 | `agent/provider_responses_test.go` | Test | Rust tests + differential cases |
| 70 | `agent/proxyenv_test.go` | Test | Rust tests + differential cases |
| 71 | `agent/retester.go` | Runtime source (Go) | Rust crate under crates/ |
| 72 | `agent/review_context_test.go` | Test | Rust tests + differential cases |
| 73 | `agent/runinfo.go` | Runtime source (Go) | Rust crate under crates/ |
| 74 | `agent/scan_ingest.go` | Runtime source (Go) | Rust crate under crates/ |
| 75 | `agent/scan_ingest_test.go` | Test | Rust tests + differential cases |
| 76 | `agent/scan_ingest_tool.go` | Runtime source (Go) | Rust crate under crates/ |
| 77 | `agent/scan_ingest_tool_pg_test.go` | Test | Rust tests + differential cases |
| 78 | `agent/session_header_test.go` | Test | Rust tests + differential cases |
| 79 | `agent/side_questions.go` | Runtime source (Go) | Rust crate under crates/ |
| 80 | `agent/side_questions_test.go` | Test | Rust tests + differential cases |
| 81 | `agent/stagnation_test.go` | Test | Rust tests + differential cases |
| 82 | `agent/target_gate_tools_test.go` | Test | Rust tests + differential cases |
| 83 | `agent/taskclock.go` | Runtime source (Go) | Rust crate under crates/ |
| 84 | `agent/terminalreason.go` | Runtime source (Go) | Rust crate under crates/ |
| 85 | `agent/terminalreason_test.go` | Test | Rust tests + differential cases |
| 86 | `agent/testmain_test.go` | Test | Rust tests + differential cases |
| 87 | `agent/toolcatalog.go` | Runtime source (Go) | Rust crate under crates/ |
| 88 | `agent/toolcatalog_test.go` | Test | Rust tests + differential cases |
| 89 | `agent/toolrag.go` | Runtime source (Go) | Rust crate under crates/ |
| 90 | `agent/toolrag_test.go` | Test | Rust tests + differential cases |
| 91 | `agent/tools.go` | Runtime source (Go) | Rust crate under crates/ |
| 92 | `agent/tools_creds.go` | Runtime source (Go) | Rust crate under crates/ |
| 93 | `agent/tools_digest.go` | Runtime source (Go) | Rust crate under crates/ |
| 94 | `agent/tools_insert.go` | Runtime source (Go) | Rust crate under crates/ |
| 95 | `agent/tools_nil_store_test.go` | Test | Rust tests + differential cases |
| 96 | `agent/tools_overview_test.go` | Test | Rust tests + differential cases |
| 97 | `agent/untrusted_boundary.go` | Runtime source (Go) | Rust crate under crates/ |
| 98 | `agent/untrusted_boundary_test.go` | Test | Rust tests + differential cases |
| 99 | `agent/untrusted_prompt.go` | Runtime source (Go) | Rust crate under crates/ |
| 100 | `agent/user_agent_test.go` | Test | Rust tests + differential cases |
| 101 | `agent/webfetch.go` | Runtime source (Go) | Rust crate under crates/ |
| 102 | `agent/webfetch_dialer_test.go` | Test | Rust tests + differential cases |
| 103 | `agent/webfetch_egress_test.go` | Test | Rust tests + differential cases |
| 104 | `agent/webfetch_redirect_test.go` | Test | Rust tests + differential cases |
| 105 | `agent/worker.go` | Runtime source (Go) | Rust crate under crates/ |
| 106 | `agent/worker_intervention_test.go` | Test | Rust tests + differential cases |
| 107 | `agent/wrapup.go` | Runtime source (Go) | Rust crate under crates/ |
| 108 | `build.sh` | Startup/install script | Rust start.sh / install.sh |
| 109 | `cmd/artex/main.go` | Runtime source (Go) | Rust crate under crates/ |
| 110 | `cmd/artex/main_test.go` | Test | Rust tests + differential cases |
| 111 | `config.example.json` | Configuration | Rust config + example |
| 112 | `config/config.go` | Runtime source (Go) | Rust crate under crates/ |
| 113 | `config/config_test.go` | Test | Rust tests + differential cases |
| 114 | `db/activity_page_test.go` | Test | Rust tests + differential cases |
| 115 | `db/asset_dsl.go` | Runtime source (Go) | Rust crate under crates/ |
| 116 | `db/asset_intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 117 | `db/asset_intercept_match.go` | Runtime source (Go) | Rust crate under crates/ |
| 118 | `db/asset_intercept_match_test.go` | Test | Rust tests + differential cases |
| 119 | `db/assets.go` | Runtime source (Go) | Rust crate under crates/ |
| 120 | `db/assets_test.go` | Test | Rust tests + differential cases |
| 121 | `db/chat_mentions.go` | Runtime source (Go) | Rust crate under crates/ |
| 122 | `db/cleanup_helpers_test.go` | Test | Rust tests + differential cases |
| 123 | `db/commands.go` | Runtime source (Go) | Rust crate under crates/ |
| 124 | `db/companies.go` | Runtime source (Go) | Rust crate under crates/ |
| 125 | `db/companies_test.go` | Test | Rust tests + differential cases |
| 126 | `db/company_lock_test.go` | Test | Rust tests + differential cases |
| 127 | `db/company_scope.go` | Runtime source (Go) | Rust crate under crates/ |
| 128 | `db/company_scope_consistency_test.go` | Test | Rust tests + differential cases |
| 129 | `db/company_scope_test.go` | Test | Rust tests + differential cases |
| 130 | `db/config.go` | Runtime source (Go) | Rust crate under crates/ |
| 131 | `db/config_max_tokens_test.go` | Test | Rust tests + differential cases |
| 132 | `db/config_test.go` | Test | Rust tests + differential cases |
| 133 | `db/constants.go` | Runtime source (Go) | Rust crate under crates/ |
| 134 | `db/constraints.go` | Runtime source (Go) | Rust crate under crates/ |
| 135 | `db/conversation.go` | Runtime source (Go) | Rust crate under crates/ |
| 136 | `db/conversation_pin_test.go` | Test | Rust tests + differential cases |
| 137 | `db/coverage_ledger.go` | Runtime source (Go) | Rust crate under crates/ |
| 138 | `db/coverage_ledger_test.go` | Test | Rust tests + differential cases |
| 139 | `db/customtool_test.go` | Test | Rust tests + differential cases |
| 140 | `db/db.go` | Runtime source (Go) | Rust crate under crates/ |
| 141 | `db/db_test.go` | Test | Rust tests + differential cases |
| 142 | `db/digest.go` | Runtime source (Go) | Rust crate under crates/ |
| 143 | `db/evidence_secrets_test.go` | Test | Rust tests + differential cases |
| 144 | `db/exploration.go` | Runtime source (Go) | Rust crate under crates/ |
| 145 | `db/exploration_sources.go` | Runtime source (Go) | Rust crate under crates/ |
| 146 | `db/exploration_sources_test.go` | Test | Rust tests + differential cases |
| 147 | `db/exploration_test.go` | Test | Rust tests + differential cases |
| 148 | `db/exploration_tokens_test.go` | Test | Rust tests + differential cases |
| 149 | `db/export_test.go` | Test | Rust tests + differential cases |
| 150 | `db/finding_assets.go` | Runtime source (Go) | Rust crate under crates/ |
| 151 | `db/finding_assets_test.go` | Test | Rust tests + differential cases |
| 152 | `db/finding_reasoning_test.go` | Test | Rust tests + differential cases |
| 153 | `db/finding_retests.go` | Runtime source (Go) | Rust crate under crates/ |
| 154 | `db/finding_retests_test.go` | Test | Rust tests + differential cases |
| 155 | `db/finding_retests_verification_test.go` | Test | Rust tests + differential cases |
| 156 | `db/finding_traffic.go` | Runtime source (Go) | Rust crate under crates/ |
| 157 | `db/finding_traffic_archive.go` | Runtime source (Go) | Rust crate under crates/ |
| 158 | `db/finding_traffic_archive_test.go` | Test | Rust tests + differential cases |
| 159 | `db/finding_verification.go` | Runtime source (Go) | Rust crate under crates/ |
| 160 | `db/findings.go` | Runtime source (Go) | Rust crate under crates/ |
| 161 | `db/findings_test.go` | Test | Rust tests + differential cases |
| 162 | `db/intent_admission.go` | Runtime source (Go) | Rust crate under crates/ |
| 163 | `db/intent_claim_test.go` | Test | Rust tests + differential cases |
| 164 | `db/intent_control_test.go` | Test | Rust tests + differential cases |
| 165 | `db/intent_delete_test.go` | Test | Rust tests + differential cases |
| 166 | `db/intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 167 | `db/intercept_argv_test.go` | Test | Rust tests + differential cases |
| 168 | `db/intercept_decided_by_test.go` | Test | Rust tests + differential cases |
| 169 | `db/intercept_detail.go` | Runtime source (Go) | Rust crate under crates/ |
| 170 | `db/intercept_detail_test.go` | Test | Rust tests + differential cases |
| 171 | `db/intercept_execution.go` | Runtime source (Go) | Rust crate under crates/ |
| 172 | `db/intercept_execution_test.go` | Test | Rust tests + differential cases |
| 173 | `db/intercept_filter_test.go` | Test | Rust tests + differential cases |
| 174 | `db/intercept_seed_test.go` | Test | Rust tests + differential cases |
| 175 | `db/intercept_source_test.go` | Test | Rust tests + differential cases |
| 176 | `db/jsonb_clean_test.go` | Test | Rust tests + differential cases |
| 177 | `db/lessons.go` | Runtime source (Go) | Rust crate under crates/ |
| 178 | `db/lessons_test.go` | Test | Rust tests + differential cases |
| 179 | `db/llm_records_migrate_test.go` | Test | Rust tests + differential cases |
| 180 | `db/llm_usage.go` | Runtime source (Go) | Rust crate under crates/ |
| 181 | `db/llmhealth.go` | Runtime source (Go) | Rust crate under crates/ |
| 182 | `db/llmretry.go` | Runtime source (Go) | Rust crate under crates/ |
| 183 | `db/llmretry_test.go` | Test | Rust tests + differential cases |
| 184 | `db/logs.go` | Runtime source (Go) | Rust crate under crates/ |
| 185 | `db/migrate.go` | Runtime source (Go) | Rust crate under crates/ |
| 186 | `db/migrate_test.go` | Test | Rust tests + differential cases |
| 187 | `db/migrations/0001_llm_records.sql` | Database schema/migration | crates/db/migrations |
| 188 | `db/migrations/0002_llm_usage.sql` | Database schema/migration | crates/db/migrations |
| 189 | `db/nkey.go` | Runtime source (Go) | Rust crate under crates/ |
| 190 | `db/notification.go` | Runtime source (Go) | Rust crate under crates/ |
| 191 | `db/notification_delivery.go` | Runtime source (Go) | Rust crate under crates/ |
| 192 | `db/notification_test.go` | Test | Rust tests + differential cases |
| 193 | `db/schema.sql` | Database schema/migration | crates/db/migrations |
| 194 | `db/secrets.go` | Runtime source (Go) | Rust crate under crates/ |
| 195 | `db/secrets_test.go` | Test | Rust tests + differential cases |
| 196 | `db/settings.go` | Runtime source (Go) | Rust crate under crates/ |
| 197 | `db/settings_insert_test.go` | Test | Rust tests + differential cases |
| 198 | `db/side_questions.go` | Runtime source (Go) | Rust crate under crates/ |
| 199 | `db/side_questions_test.go` | Test | Rust tests + differential cases |
| 200 | `db/skill_gaps.go` | Runtime source (Go) | Rust crate under crates/ |
| 201 | `db/skill_usage.go` | Runtime source (Go) | Rust crate under crates/ |
| 202 | `db/skill_usage_test.go` | Test | Rust tests + differential cases |
| 203 | `db/target_pins.go` | Runtime source (Go) | Rust crate under crates/ |
| 204 | `db/target_pins_test.go` | Test | Rust tests + differential cases |
| 205 | `db/task_archive_aggregate_stats.go` | Runtime source (Go) | Rust crate under crates/ |
| 206 | `db/task_archives.go` | Runtime source (Go) | Rust crate under crates/ |
| 207 | `db/task_archives_restore.go` | Runtime source (Go) | Rust crate under crates/ |
| 208 | `db/task_archives_restore_url_test.go` | Test | Rust tests + differential cases |
| 209 | `db/task_archives_test.go` | Test | Rust tests + differential cases |
| 210 | `db/task_assets.go` | Runtime source (Go) | Rust crate under crates/ |
| 211 | `db/task_assets_context.go` | Runtime source (Go) | Rust crate under crates/ |
| 212 | `db/task_assets_test.go` | Test | Rust tests + differential cases |
| 213 | `db/task_categories.go` | Runtime source (Go) | Rust crate under crates/ |
| 214 | `db/task_categories_test.go` | Test | Rust tests + differential cases |
| 215 | `db/task_context.go` | Runtime source (Go) | Rust crate under crates/ |
| 216 | `db/task_context_lock_test.go` | Test | Rust tests + differential cases |
| 217 | `db/task_context_unit_test.go` | Test | Rust tests + differential cases |
| 218 | `db/task_credentials.go` | Runtime source (Go) | Rust crate under crates/ |
| 219 | `db/task_delete_concurrency_test.go` | Test | Rust tests + differential cases |
| 220 | `db/task_intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 221 | `db/task_list_performance_test.go` | Test | Rust tests + differential cases |
| 222 | `db/task_metadata_test.go` | Test | Rust tests + differential cases |
| 223 | `db/task_outcome.go` | Runtime source (Go) | Rust crate under crates/ |
| 224 | `db/task_queue_test.go` | Test | Rust tests + differential cases |
| 225 | `db/task_scope.go` | Runtime source (Go) | Rust crate under crates/ |
| 226 | `db/task_scope_test.go` | Test | Rust tests + differential cases |
| 227 | `db/task_source_limit_test.go` | Test | Rust tests + differential cases |
| 228 | `db/task_templates.go` | Runtime source (Go) | Rust crate under crates/ |
| 229 | `db/task_templates_test.go` | Test | Rust tests + differential cases |
| 230 | `db/tasks.go` | Runtime source (Go) | Rust crate under crates/ |
| 231 | `db/tasks_test.go` | Test | Rust tests + differential cases |
| 232 | `db/testmain_test.go` | Test | Rust tests + differential cases |
| 233 | `db/tool_usage.go` | Runtime source (Go) | Rust crate under crates/ |
| 234 | `db/tool_usage_test.go` | Test | Rust tests + differential cases |
| 235 | `db/tools.go` | Runtime source (Go) | Rust crate under crates/ |
| 236 | `db/tools_secrets_test.go` | Test | Rust tests + differential cases |
| 237 | `db/triggers.go` | Runtime source (Go) | Rust crate under crates/ |
| 238 | `dev.sh` | Startup/install script | Rust start.sh / install.sh |
| 239 | `docker-compose.bench.yml` | Deployment (Docker) | Rust Dockerfile/compose |
| 240 | `docker-compose.yml` | Deployment (Docker) | Rust Dockerfile/compose |
| 241 | `docs/ai-pr-review-technology.md` | Documentation | Preserved (docs) |
| 242 | `docs/rsi-lessons.md` | Documentation | Preserved (docs) |
| 243 | `docs/vulnerability-traffic-evidence.md` | Documentation | Preserved (docs) |
| 244 | `egress/egress.go` | Runtime source (Go) | Rust crate under crates/ |
| 245 | `egress/egress_test.go` | Test | Rust tests + differential cases |
| 246 | `enrich/enrich.go` | Runtime source (Go) | Rust crate under crates/ |
| 247 | `evidence/store.go` | Runtime source (Go) | Rust crate under crates/ |
| 248 | `evidence/store_test.go` | Test | Rust tests + differential cases |
| 249 | `evidence/testmain_test.go` | Test | Rust tests + differential cases |
| 250 | `extension/background.js` | Browser extension | Preserved as-is |
| 251 | `extension/manifest.json` | Browser extension | Preserved as-is |
| 252 | `extension/popup.html` | Browser extension | Preserved as-is |
| 253 | `extension/popup.js` | Browser extension | Preserved as-is |
| 254 | `go.mod` | Configuration | Rust config + example |
| 255 | `go.sum` | Configuration | Rust config + example |
| 256 | `guard/argv_test.go` | Test | Rust tests + differential cases |
| 257 | `guard/guard.go` | Runtime source (Go) | Rust crate under crates/ |
| 258 | `guard/guard_test.go` | Test | Rust tests + differential cases |
| 259 | `guard/judge_precedence_test.go` | Test | Rust tests + differential cases |
| 260 | `guard/target_gate_test.go` | Test | Rust tests + differential cases |
| 261 | `guard/testmain_test.go` | Test | Rust tests + differential cases |
| 262 | `install.sh` | Startup/install script | Rust start.sh / install.sh |
| 263 | `intercept/alwaysallow_denylist.go` | Runtime source (Go) | Rust crate under crates/ |
| 264 | `intercept/alwaysallow_denylist_test.go` | Test | Rust tests + differential cases |
| 265 | `intercept/argv.go` | Runtime source (Go) | Rust crate under crates/ |
| 266 | `intercept/argv_test.go` | Test | Rust tests + differential cases |
| 267 | `intercept/closed_judge.go` | Runtime source (Go) | Rust crate under crates/ |
| 268 | `intercept/closed_judge_pg_test.go` | Test | Rust tests + differential cases |
| 269 | `intercept/closed_judge_test.go` | Test | Rust tests + differential cases |
| 270 | `intercept/enrich_probe_test.go` | Test | Rust tests + differential cases |
| 271 | `intercept/export_test.go` | Test | Rust tests + differential cases |
| 272 | `intercept/intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 273 | `intercept/prompt.go` | Runtime source (Go) | Rust crate under crates/ |
| 274 | `intercept/prompt_test.go` | Test | Rust tests + differential cases |
| 275 | `intercept/review_context.go` | Runtime source (Go) | Rust crate under crates/ |
| 276 | `intercept/review_context_test.go` | Test | Rust tests + differential cases |
| 277 | `intercept/target.go` | Runtime source (Go) | Rust crate under crates/ |
| 278 | `intercept/target_declared_test.go` | Test | Rust tests + differential cases |
| 279 | `intercept/target_dial.go` | Runtime source (Go) | Rust crate under crates/ |
| 280 | `intercept/target_dial_pg_test.go` | Test | Rust tests + differential cases |
| 281 | `intercept/target_dial_test.go` | Test | Rust tests + differential cases |
| 282 | `intercept/target_exempt.go` | Runtime source (Go) | Rust crate under crates/ |
| 283 | `intercept/target_exempt_test.go` | Test | Rust tests + differential cases |
| 284 | `intercept/target_extract.go` | Runtime source (Go) | Rust crate under crates/ |
| 285 | `intercept/target_gate.go` | Runtime source (Go) | Rust crate under crates/ |
| 286 | `intercept/target_gate_pg_test.go` | Test | Rust tests + differential cases |
| 287 | `intercept/target_gate_test.go` | Test | Rust tests + differential cases |
| 288 | `intercept/target_redirect.go` | Runtime source (Go) | Rust crate under crates/ |
| 289 | `intercept/target_redirect_pg_test.go` | Test | Rust tests + differential cases |
| 290 | `intercept/target_redirect_test.go` | Test | Rust tests + differential cases |
| 291 | `intercept/target_rendered.go` | Runtime source (Go) | Rust crate under crates/ |
| 292 | `intercept/target_rendered_test.go` | Test | Rust tests + differential cases |
| 293 | `intercept/target_request.go` | Runtime source (Go) | Rust crate under crates/ |
| 294 | `intercept/target_request_test.go` | Test | Rust tests + differential cases |
| 295 | `intercept/target_resolve.go` | Runtime source (Go) | Rust crate under crates/ |
| 296 | `intercept/target_test.go` | Test | Rust tests + differential cases |
| 297 | `intercept/testmain_test.go` | Test | Rust tests + differential cases |
| 298 | `intercept/trace.go` | Runtime source (Go) | Rust crate under crates/ |
| 299 | `intercept/trace_test.go` | Test | Rust tests + differential cases |
| 300 | `llmpool/health.go` | Runtime source (Go) | Rust crate under crates/ |
| 301 | `llmpool/health_policy_test.go` | Test | Rust tests + differential cases |
| 302 | `llmpool/pool.go` | Runtime source (Go) | Rust crate under crates/ |
| 303 | `llmpool/pool_test.go` | Test | Rust tests + differential cases |
| 304 | `llmrec/capture.go` | Runtime source (Go) | Rust crate under crates/ |
| 305 | `llmrec/capture_test.go` | Test | Rust tests + differential cases |
| 306 | `llmrec/llmrec.go` | Runtime source (Go) | Rust crate under crates/ |
| 307 | `llmrec/llmrec_test.go` | Test | Rust tests + differential cases |
| 308 | `llmrec/testmain_test.go` | Test | Rust tests + differential cases |
| 309 | `mcphttp/client.go` | Runtime source (Go) | Rust crate under crates/ |
| 310 | `mcphttp/client_sse_test.go` | Test | Rust tests + differential cases |
| 311 | `mcphttp/client_test.go` | Test | Rust tests + differential cases |
| 312 | `notify/channel.go` | Runtime source (Go) | Rust crate under crates/ |
| 313 | `notify/channels_test.go` | Test | Rust tests + differential cases |
| 314 | `notify/dingtalk.go` | Runtime source (Go) | Rust crate under crates/ |
| 315 | `notify/email.go` | Runtime source (Go) | Rust crate under crates/ |
| 316 | `notify/email_test.go` | Test | Rust tests + differential cases |
| 317 | `notify/event.go` | Runtime source (Go) | Rust crate under crates/ |
| 318 | `notify/feishu.go` | Runtime source (Go) | Rust crate under crates/ |
| 319 | `notify/filter.go` | Runtime source (Go) | Rust crate under crates/ |
| 320 | `notify/filter_test.go` | Test | Rust tests + differential cases |
| 321 | `notify/html.go` | Runtime source (Go) | Rust crate under crates/ |
| 322 | `notify/http.go` | Runtime source (Go) | Rust crate under crates/ |
| 323 | `notify/http_test.go` | Test | Rust tests + differential cases |
| 324 | `notify/limits_test.go` | Test | Rust tests + differential cases |
| 325 | `notify/markdown.go` | Runtime source (Go) | Rust crate under crates/ |
| 326 | `notify/mask.go` | Runtime source (Go) | Rust crate under crates/ |
| 327 | `notify/mask_test.go` | Test | Rust tests + differential cases |
| 328 | `notify/notify.go` | Runtime source (Go) | Rust crate under crates/ |
| 329 | `notify/pack_test.go` | Test | Rust tests + differential cases |
| 330 | `notify/redact_test.go` | Test | Rust tests + differential cases |
| 331 | `notify/render.go` | Runtime source (Go) | Rust crate under crates/ |
| 332 | `notify/render_test.go` | Test | Rust tests + differential cases |
| 333 | `notify/sign_test.go` | Test | Rust tests + differential cases |
| 334 | `notify/ssrf_test.go` | Test | Rust tests + differential cases |
| 335 | `notify/telegram.go` | Runtime source (Go) | Rust crate under crates/ |
| 336 | `notify/webhook.go` | Runtime source (Go) | Rust crate under crates/ |
| 337 | `notify/webhook_template_test.go` | Test | Rust tests + differential cases |
| 338 | `notify/wecom.go` | Runtime source (Go) | Rust crate under crates/ |
| 339 | `report/findings.go` | Runtime source (Go) | Rust crate under crates/ |
| 340 | `report/report.go` | Runtime source (Go) | Rust crate under crates/ |
| 341 | `report/report_test.go` | Test | Rust tests + differential cases |
| 342 | `reset-password.sh` | Startup/install script | Rust start.sh / install.sh |
| 343 | `screenshots/.gitkeep` | Documentation (screenshot) | Preserved (docs) |
| 344 | `screenshots/agents.png` | Documentation (screenshot) | Preserved (docs) |
| 345 | `screenshots/assets.png` | Documentation (screenshot) | Preserved (docs) |
| 346 | `screenshots/assets_test.png` | Documentation (screenshot) | Preserved (docs) |
| 347 | `screenshots/chat.png` | Documentation (screenshot) | Preserved (docs) |
| 348 | `screenshots/dashboard.png` | Documentation (screenshot) | Preserved (docs) |
| 349 | `screenshots/findings.png` | Documentation (screenshot) | Preserved (docs) |
| 350 | `screenshots/graph.png` | Documentation (screenshot) | Preserved (docs) |
| 351 | `screenshots/intercept.png` | Documentation (screenshot) | Preserved (docs) |
| 352 | `screenshots/llm.png` | Documentation (screenshot) | Preserved (docs) |
| 353 | `screenshots/logs.png` | Documentation (screenshot) | Preserved (docs) |
| 354 | `screenshots/sessions.png` | Documentation (screenshot) | Preserved (docs) |
| 355 | `screenshots/tasks.png` | Documentation (screenshot) | Preserved (docs) |
| 356 | `screenshots/traffic.png` | Documentation (screenshot) | Preserved (docs) |
| 357 | `scripts/backup.sh` | Build/ops tooling | Rust build scripts / Dockerfile |
| 358 | `scripts/benchmark/README.md` | Build/ops tooling | Rust build scripts / Dockerfile |
| 359 | `scripts/benchmark/drost.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 360 | `scripts/benchmark/ground_truth/juice-shop.json` | Build/ops tooling | Rust build scripts / Dockerfile |
| 361 | `scripts/benchmark/run_bench.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 362 | `scripts/benchmark/test_drost.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 363 | `scripts/extract-changelog-section.sh` | Build/ops tooling | Rust build scripts / Dockerfile |
| 364 | `scripts/poc/php2shell/LICENSE` | Build/ops tooling | Rust build scripts / Dockerfile |
| 365 | `scripts/poc/php2shell/README.upstream.md` | Build/ops tooling | Rust build scripts / Dockerfile |
| 366 | `scripts/poc/php2shell/docs/ANALYSIS.md` | Build/ops tooling | Rust build scripts / Dockerfile |
| 367 | `scripts/poc/php2shell/lab/setup.sh` | Build/ops tooling | Rust build scripts / Dockerfile |
| 368 | `scripts/poc/php2shell/poc/verify_handler_mismatch.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 369 | `scripts/poc/php2shell/poc/verify_session_injection.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 370 | `scripts/poc/php2shell/poc/verify_uaf_asan.php` | Build/ops tooling | Rust build scripts / Dockerfile |
| 371 | `scripts/posture_check.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 372 | `scripts/submission_review/README.md` | Build/ops tooling | Rust build scripts / Dockerfile |
| 373 | `scripts/submission_review/review.py` | Build/ops tooling | Rust build scripts / Dockerfile |
| 374 | `scripts/submission_review/rules/strict-vdp.json` | Build/ops tooling | Rust build scripts / Dockerfile |
| 375 | `scripts/test-extract-changelog-section.sh` | Build/ops tooling | Rust build scripts / Dockerfile |
| 376 | `server/ask_operator.go` | Runtime source (Go) | Rust crate under crates/ |
| 377 | `server/ask_operator_test.go` | Test | Rust tests + differential cases |
| 378 | `server/assembly.go` | Runtime source (Go) | Rust crate under crates/ |
| 379 | `server/assembly_test.go` | Test | Rust tests + differential cases |
| 380 | `server/asset_intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 381 | `server/assets.go` | Runtime source (Go) | Rust crate under crates/ |
| 382 | `server/assets_scope_test.go` | Test | Rust tests + differential cases |
| 383 | `server/auth.go` | Runtime source (Go) | Rust crate under crates/ |
| 384 | `server/auth_test.go` | Test | Rust tests + differential cases |
| 385 | `server/broadcast.go` | Runtime source (Go) | Rust crate under crates/ |
| 386 | `server/chat_llm_resolve_test.go` | Test | Rust tests + differential cases |
| 387 | `server/chat_mentions.go` | Runtime source (Go) | Rust crate under crates/ |
| 388 | `server/chat_mentions_test.go` | Test | Rust tests + differential cases |
| 389 | `server/chatupload.go` | Runtime source (Go) | Rust crate under crates/ |
| 390 | `server/claim_heartbeat_test.go` | Test | Rust tests + differential cases |
| 391 | `server/commands.go` | Runtime source (Go) | Rust crate under crates/ |
| 392 | `server/constraints_api.go` | Runtime source (Go) | Rust crate under crates/ |
| 393 | `server/conversation_status_test.go` | Test | Rust tests + differential cases |
| 394 | `server/conversations.go` | Runtime source (Go) | Rust crate under crates/ |
| 395 | `server/core_test.go` | Test | Rust tests + differential cases |
| 396 | `server/cost_ledger_test.go` | Test | Rust tests + differential cases |
| 397 | `server/customtool.go` | Runtime source (Go) | Rust crate under crates/ |
| 398 | `server/customtool_dial_test.go` | Test | Rust tests + differential cases |
| 399 | `server/customtool_target_gate_test.go` | Test | Rust tests + differential cases |
| 400 | `server/customtool_test.go` | Test | Rust tests + differential cases |
| 401 | `server/dto.go` | Runtime source (Go) | Rust crate under crates/ |
| 402 | `server/engine.go` | Runtime source (Go) | Rust crate under crates/ |
| 403 | `server/engine_budget.go` | Runtime source (Go) | Rust crate under crates/ |
| 404 | `server/engine_budget_test.go` | Test | Rust tests + differential cases |
| 405 | `server/engine_cancelcause_test.go` | Test | Rust tests + differential cases |
| 406 | `server/engine_emptyturn_test.go` | Test | Rust tests + differential cases |
| 407 | `server/engine_llm_calls_test.go` | Test | Rust tests + differential cases |
| 408 | `server/engine_timeout.go` | Runtime source (Go) | Rust crate under crates/ |
| 409 | `server/env_scrub.go` | Runtime source (Go) | Rust crate under crates/ |
| 410 | `server/env_scrub_test.go` | Test | Rust tests + differential cases |
| 411 | `server/fact_source_schema_test.go` | Test | Rust tests + differential cases |
| 412 | `server/finding_reasoning_test.go` | Test | Rust tests + differential cases |
| 413 | `server/finding_retests.go` | Runtime source (Go) | Rust crate under crates/ |
| 414 | `server/finding_retests_test.go` | Test | Rust tests + differential cases |
| 415 | `server/finding_traffic.go` | Runtime source (Go) | Rust crate under crates/ |
| 416 | `server/finding_traffic_export.go` | Runtime source (Go) | Rust crate under crates/ |
| 417 | `server/finding_traffic_test.go` | Test | Rust tests + differential cases |
| 418 | `server/finding_verification.go` | Runtime source (Go) | Rust crate under crates/ |
| 419 | `server/finding_verification_decode_test.go` | Test | Rust tests + differential cases |
| 420 | `server/finding_verification_test.go` | Test | Rust tests + differential cases |
| 421 | `server/finding_workflow.go` | Runtime source (Go) | Rust crate under crates/ |
| 422 | `server/finding_workflow_test.go` | Test | Rust tests + differential cases |
| 423 | `server/findings_groups.go` | Runtime source (Go) | Rust crate under crates/ |
| 424 | `server/findings_groups_test.go` | Test | Rust tests + differential cases |
| 425 | `server/goal_gate.go` | Runtime source (Go) | Rust crate under crates/ |
| 426 | `server/goal_gate_test.go` | Test | Rust tests + differential cases |
| 427 | `server/goals.go` | Runtime source (Go) | Rust crate under crates/ |
| 428 | `server/goals_api.go` | Runtime source (Go) | Rust crate under crates/ |
| 429 | `server/goals_test.go` | Test | Rust tests + differential cases |
| 430 | `server/inheritance_api_test.go` | Test | Rust tests + differential cases |
| 431 | `server/inheritance_dto_test.go` | Test | Rust tests + differential cases |
| 432 | `server/intent_intervention.go` | Runtime source (Go) | Rust crate under crates/ |
| 433 | `server/intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 434 | `server/intercept_always_allow_pg_test.go` | Test | Rust tests + differential cases |
| 435 | `server/intercept_argv_test.go` | Test | Rust tests + differential cases |
| 436 | `server/intercept_decided_by_test.go` | Test | Rust tests + differential cases |
| 437 | `server/intercept_detail_test.go` | Test | Rust tests + differential cases |
| 438 | `server/intercept_filter_test.go` | Test | Rust tests + differential cases |
| 439 | `server/intercept_live_test.go` | Test | Rust tests + differential cases |
| 440 | `server/intercept_review_test.go` | Test | Rust tests + differential cases |
| 441 | `server/lessons.go` | Runtime source (Go) | Rust crate under crates/ |
| 442 | `server/lessons_test.go` | Test | Rust tests + differential cases |
| 443 | `server/llm_key_endpoint_test.go` | Test | Rust tests + differential cases |
| 444 | `server/llm_ollama_test.go` | Test | Rust tests + differential cases |
| 445 | `server/llm_test_fallback_test.go` | Test | Rust tests + differential cases |
| 446 | `server/llmpool.go` | Runtime source (Go) | Rust crate under crates/ |
| 447 | `server/llmpool_test.go` | Test | Rust tests + differential cases |
| 448 | `server/llmrec_raw_test.go` | Test | Rust tests + differential cases |
| 449 | `server/llmretry.go` | Runtime source (Go) | Rust crate under crates/ |
| 450 | `server/llmretry_test.go` | Test | Rust tests + differential cases |
| 451 | `server/login_limit.go` | Runtime source (Go) | Rust crate under crates/ |
| 452 | `server/login_limit_test.go` | Test | Rust tests + differential cases |
| 453 | `server/logsink.go` | Runtime source (Go) | Rust crate under crates/ |
| 454 | `server/manager.go` | Runtime source (Go) | Rust crate under crates/ |
| 455 | `server/manager_delete_files_test.go` | Test | Rust tests + differential cases |
| 456 | `server/manager_lifecycle_test.go` | Test | Rust tests + differential cases |
| 457 | `server/mcp_health_test.go` | Test | Rust tests + differential cases |
| 458 | `server/mcpdiscover.go` | Runtime source (Go) | Rust crate under crates/ |
| 459 | `server/mgmt_test.go` | Test | Rust tests + differential cases |
| 460 | `server/notifier.go` | Runtime source (Go) | Rust crate under crates/ |
| 461 | `server/notify_api.go` | Runtime source (Go) | Rust crate under crates/ |
| 462 | `server/notify_api_test.go` | Test | Rust tests + differential cases |
| 463 | `server/orchestration.go` | Runtime source (Go) | Rust crate under crates/ |
| 464 | `server/planner_quiet_test.go` | Test | Rust tests + differential cases |
| 465 | `server/platform_tools.go` | Runtime source (Go) | Rust crate under crates/ |
| 466 | `server/platform_tools_test.go` | Test | Rust tests + differential cases |
| 467 | `server/prompt_vars_test.go` | Test | Rust tests + differential cases |
| 468 | `server/scheduler.go` | Runtime source (Go) | Rust crate under crates/ |
| 469 | `server/secrets_api_test.go` | Test | Rust tests + differential cases |
| 470 | `server/server.go` | Runtime source (Go) | Rust crate under crates/ |
| 471 | `server/server_mgmt.go` | Runtime source (Go) | Rust crate under crates/ |
| 472 | `server/shipped_skills_test.go` | Test | Rust tests + differential cases |
| 473 | `server/side_questions.go` | Runtime source (Go) | Rust crate under crates/ |
| 474 | `server/side_questions_test.go` | Test | Rust tests + differential cases |
| 475 | `server/skill_frontmatter_test.go` | Test | Rust tests + differential cases |
| 476 | `server/skill_gaps.go` | Runtime source (Go) | Rust crate under crates/ |
| 477 | `server/skill_gaps_test.go` | Test | Rust tests + differential cases |
| 478 | `server/skill_upload_test.go` | Test | Rust tests + differential cases |
| 479 | `server/skill_usage.go` | Runtime source (Go) | Rust crate under crates/ |
| 480 | `server/skill_zip.go` | Runtime source (Go) | Rust crate under crates/ |
| 481 | `server/sync_scopesentry.go` | Runtime source (Go) | Rust crate under crates/ |
| 482 | `server/target_gate_flag_test.go` | Test | Rust tests + differential cases |
| 483 | `server/target_gate_scope_test.go` | Test | Rust tests + differential cases |
| 484 | `server/target_gate_wiring_test.go` | Test | Rust tests + differential cases |
| 485 | `server/target_redirect_test.go` | Test | Rust tests + differential cases |
| 486 | `server/task_admission_test.go` | Test | Rust tests + differential cases |
| 487 | `server/task_archive_package.go` | Runtime source (Go) | Rust crate under crates/ |
| 488 | `server/task_archive_package_test.go` | Test | Rust tests + differential cases |
| 489 | `server/task_archives.go` | Runtime source (Go) | Rust crate under crates/ |
| 490 | `server/task_archives_test.go` | Test | Rust tests + differential cases |
| 491 | `server/task_assets.go` | Runtime source (Go) | Rust crate under crates/ |
| 492 | `server/task_categories.go` | Runtime source (Go) | Rust crate under crates/ |
| 493 | `server/task_categories_test.go` | Test | Rust tests + differential cases |
| 494 | `server/task_control.go` | Runtime source (Go) | Rust crate under crates/ |
| 495 | `server/task_control_routes_test.go` | Test | Rust tests + differential cases |
| 496 | `server/task_credentials.go` | Runtime source (Go) | Rust crate under crates/ |
| 497 | `server/task_credentials_test.go` | Test | Rust tests + differential cases |
| 498 | `server/task_delete_barrier_test.go` | Test | Rust tests + differential cases |
| 499 | `server/task_detail_metrics_test.go` | Test | Rust tests + differential cases |
| 500 | `server/task_end_reason_test.go` | Test | Rust tests + differential cases |
| 501 | `server/task_intercept.go` | Runtime source (Go) | Rust crate under crates/ |
| 502 | `server/task_llm.go` | Runtime source (Go) | Rust crate under crates/ |
| 503 | `server/task_llm_escalation_test.go` | Test | Rust tests + differential cases |
| 504 | `server/task_llm_rotation_test.go` | Test | Rust tests + differential cases |
| 505 | `server/task_llm_test.go` | Test | Rust tests + differential cases |
| 506 | `server/task_metadata.go` | Runtime source (Go) | Rust crate under crates/ |
| 507 | `server/task_metadata_test.go` | Test | Rust tests + differential cases |
| 508 | `server/task_outcome.go` | Runtime source (Go) | Rust crate under crates/ |
| 509 | `server/task_outcome_test.go` | Test | Rust tests + differential cases |
| 510 | `server/task_pause_deadline_test.go` | Test | Rust tests + differential cases |
| 511 | `server/task_resolution.go` | Runtime source (Go) | Rust crate under crates/ |
| 512 | `server/task_scope_description_test.go` | Test | Rust tests + differential cases |
| 513 | `server/task_templates.go` | Runtime source (Go) | Rust crate under crates/ |
| 514 | `server/task_templates_test.go` | Test | Rust tests + differential cases |
| 515 | `server/testmain_test.go` | Test | Rust tests + differential cases |
| 516 | `server/tool_usage.go` | Runtime source (Go) | Rust crate under crates/ |
| 517 | `server/tool_usage_test.go` | Test | Rust tests + differential cases |
| 518 | `server/tools_secrets_api_test.go` | Test | Rust tests + differential cases |
| 519 | `server/tools_wire_test.go` | Test | Rust tests + differential cases |
| 520 | `server/traffic_ingest.go` | Runtime source (Go) | Rust crate under crates/ |
| 521 | `server/traffic_ingest_test.go` | Test | Rust tests + differential cases |
| 522 | `server/trigger_merge_test.go` | Test | Rust tests + differential cases |
| 523 | `server/triggers.go` | Runtime source (Go) | Rust crate under crates/ |
| 524 | `server/update_check.go` | Runtime source (Go) | Rust crate under crates/ |
| 525 | `server/update_check_test.go` | Test | Rust tests + differential cases |
| 526 | `server/webui_embed.go` | Runtime source (Go) | Rust crate under crates/ |
| 527 | `server/webui_stub.go` | Runtime source (Go) | Rust crate under crates/ |
| 528 | `server/worker_message_test.go` | Test | Rust tests + differential cases |
| 529 | `server/workspace.go` | Runtime source (Go) | Rust crate under crates/ |
| 530 | `sidequestion/CONTEXT_BUDGET.md` | Documentation | Preserved (docs) |
| 531 | `sidequestion/README.md` | Documentation | Preserved (docs) |
| 532 | `sidequestion/VALIDATION.md` | Documentation | Preserved (docs) |
| 533 | `sidequestion/capture.go` | Runtime source (Go) | Rust crate under crates/ |
| 534 | `sidequestion/context.go` | Runtime source (Go) | Rust crate under crates/ |
| 535 | `sidequestion/context_test.go` | Test | Rust tests + differential cases |
| 536 | `sidequestion/request.go` | Runtime source (Go) | Rust crate under crates/ |
| 537 | `sidequestion/service.go` | Runtime source (Go) | Rust crate under crates/ |
| 538 | `sidequestion/sidequestion_test.go` | Test | Rust tests + differential cases |
| 539 | `sidequestion/validation-2026-09-10.json` | Configuration | Rust config + example |
| 540 | `skills/api-recon/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 541 | `skills/api-recon/reference.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 542 | `skills/api-recon/scripts/build_perm_tree.py` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 543 | `skills/api-recon/scripts/extract_route_map.py` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 544 | `skills/api-recon/scripts/harvest_static.py` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 545 | `skills/api-recon/scripts/package-lock.json` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 546 | `skills/api-recon/scripts/package.json` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 547 | `skills/api-recon/scripts/preload.js` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 548 | `skills/api-recon/scripts/runtime_harvest.js` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 549 | `skills/api-recon/scripts/spider_mpa.py` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 550 | `skills/api-security/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 551 | `skills/bbot-recon/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 552 | `skills/codeaudit/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 553 | `skills/edr-bypass-re/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 554 | `skills/patch-diff/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 555 | `skills/php2shell/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 556 | `skills/playwright-cli/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 557 | `skills/playwright-cli/references/element-attributes.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 558 | `skills/playwright-cli/references/playwright-tests.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 559 | `skills/playwright-cli/references/request-mocking.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 560 | `skills/playwright-cli/references/running-code.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 561 | `skills/playwright-cli/references/session-management.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 562 | `skills/playwright-cli/references/storage-state.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 563 | `skills/playwright-cli/references/test-generation.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 564 | `skills/playwright-cli/references/tracing.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 565 | `skills/playwright-cli/references/video-recording.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 566 | `skills/rea/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 567 | `skills/scopesentry/SKILL.md` | Runtime asset (skill) | Preserved (copied to Rust data dir) |
| 568 | `start.bat` | Startup/install script | Rust start.sh / install.sh |
| 569 | `start.sh` | Startup/install script | Rust start.sh / install.sh |
| 570 | `traffic/archive.go` | Runtime source (Go) | Rust crate under crates/ |
| 571 | `traffic/archive_test.go` | Test | Rust tests + differential cases |
| 572 | `traffic/evidence.go` | Runtime source (Go) | Rust crate under crates/ |
| 573 | `traffic/external.go` | Runtime source (Go) | Rust crate under crates/ |
| 574 | `traffic/external_test.go` | Test | Rust tests + differential cases |
| 575 | `traffic/passthrough_test.go` | Test | Rust tests + differential cases |
| 576 | `traffic/proxy_test.go` | Test | Rust tests + differential cases |
| 577 | `traffic/reclaim_test.go` | Test | Rust tests + differential cases |
| 578 | `traffic/traffic.go` | Runtime source (Go) | Rust crate under crates/ |
| 579 | `traffic/traffic_store_test.go` | Test | Rust tests + differential cases |
| 580 | `traffic/traffic_test.go` | Test | Rust tests + differential cases |
| 581 | `traffic/upgrade_test.go` | Test | Rust tests + differential cases |
| 582 | `update.sh` | Startup/install script | Rust start.sh / install.sh |
| 583 | `web/.gitignore` | Frontend source (Next.js) | Preserved as presentation layer |
| 584 | `web/.husky/pre-commit` | Frontend source (Next.js) | Preserved as presentation layer |
| 585 | `web/LICENSE` | Frontend source (Next.js) | Preserved as presentation layer |
| 586 | `web/biome.json` | Frontend source (Next.js) | Preserved as presentation layer |
| 587 | `web/components.json` | Frontend source (Next.js) | Preserved as presentation layer |
| 588 | `web/media/dashboard.png` | Frontend source (Next.js) | Preserved as presentation layer |
| 589 | `web/next.config.mjs` | Frontend source (Next.js) | Preserved as presentation layer |
| 590 | `web/package-lock.json` | Frontend source (Next.js) | Preserved as presentation layer |
| 591 | `web/package.json` | Frontend source (Next.js) | Preserved as presentation layer |
| 592 | `web/postcss.config.mjs` | Frontend source (Next.js) | Preserved as presentation layer |
| 593 | `web/public/logo.png` | Frontend source (Next.js) | Preserved as presentation layer |
| 594 | `web/public/logo.svg` | Frontend source (Next.js) | Preserved as presentation layer |
| 595 | `web/src/app/(auth)/layout.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 596 | `web/src/app/(auth)/login/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 597 | `web/src/app/(auth)/setup/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 598 | `web/src/app/(external)/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 599 | `web/src/app/(main)/_components/main-content.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 600 | `web/src/app/(main)/_components/sidebar/account-switcher.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 601 | `web/src/app/(main)/_components/sidebar/app-sidebar.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 602 | `web/src/app/(main)/_components/sidebar/change-password-dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 603 | `web/src/app/(main)/_components/sidebar/layout-controls.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 604 | `web/src/app/(main)/_components/sidebar/nav-documents.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 605 | `web/src/app/(main)/_components/sidebar/nav-main.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 606 | `web/src/app/(main)/_components/sidebar/nav-secondary.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 607 | `web/src/app/(main)/_components/sidebar/nav-user.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 608 | `web/src/app/(main)/_components/sidebar/search-dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 609 | `web/src/app/(main)/_components/sidebar/sidebar-support-card.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 610 | `web/src/app/(main)/_components/sidebar/theme-switcher.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 611 | `web/src/app/(main)/chat/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 612 | `web/src/app/(main)/dashboard/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 613 | `web/src/app/(main)/function/assets/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 614 | `web/src/app/(main)/function/commands/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 615 | `web/src/app/(main)/function/findings/_components/asset-tree.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 616 | `web/src/app/(main)/function/findings/_components/findings-table.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 617 | `web/src/app/(main)/function/findings/detail/lineage.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 618 | `web/src/app/(main)/function/findings/detail/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 619 | `web/src/app/(main)/function/findings/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 620 | `web/src/app/(main)/function/llm-records/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 621 | `web/src/app/(main)/function/sync/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 622 | `web/src/app/(main)/function/tasks/detail/_tabs/assets-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 623 | `web/src/app/(main)/function/tasks/detail/_tabs/broadcast-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 624 | `web/src/app/(main)/function/tasks/detail/_tabs/coverage-graph-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 625 | `web/src/app/(main)/function/tasks/detail/_tabs/credentials-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 626 | `web/src/app/(main)/function/tasks/detail/_tabs/findings-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 627 | `web/src/app/(main)/function/tasks/detail/_tabs/graph-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 628 | `web/src/app/(main)/function/tasks/detail/_tabs/intercept-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 629 | `web/src/app/(main)/function/tasks/detail/_tabs/overview-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 630 | `web/src/app/(main)/function/tasks/detail/_tabs/report-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 631 | `web/src/app/(main)/function/tasks/detail/_tabs/retests-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 632 | `web/src/app/(main)/function/tasks/detail/_tabs/sessions-tab.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 633 | `web/src/app/(main)/function/tasks/detail/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 634 | `web/src/app/(main)/function/tasks/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 635 | `web/src/app/(main)/function/traffic/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 636 | `web/src/app/(main)/function/workspace/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 637 | `web/src/app/(main)/layout.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 638 | `web/src/app/(main)/system/agents/detail/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 639 | `web/src/app/(main)/system/agents/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 640 | `web/src/app/(main)/system/intercept/approvals/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 641 | `web/src/app/(main)/system/intercept/assets/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 642 | `web/src/app/(main)/system/intercept/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 643 | `web/src/app/(main)/system/llm/_components/retry.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 644 | `web/src/app/(main)/system/llm/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 645 | `web/src/app/(main)/system/logs/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 646 | `web/src/app/(main)/system/mcp/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 647 | `web/src/app/(main)/system/notify/_components/channel-fields.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 648 | `web/src/app/(main)/system/notify/_components/channel-form.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 649 | `web/src/app/(main)/system/notify/_components/delivery-list.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 650 | `web/src/app/(main)/system/notify/_components/stat-tile.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 651 | `web/src/app/(main)/system/notify/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 652 | `web/src/app/(main)/system/settings/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 653 | `web/src/app/(main)/system/skills/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 654 | `web/src/app/(main)/system/tools/page.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 655 | `web/src/app/favicon.ico` | Frontend source (Next.js) | Preserved as presentation layer |
| 656 | `web/src/app/globals.css` | Frontend source (Next.js) | Preserved as presentation layer |
| 657 | `web/src/app/icon.png` | Frontend source (Next.js) | Preserved as presentation layer |
| 658 | `web/src/app/layout.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 659 | `web/src/app/not-found.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 660 | `web/src/components/agent-editor.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 661 | `web/src/components/approval-execution-focus.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 662 | `web/src/components/approval-records.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 663 | `web/src/components/asset-dsl-search.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 664 | `web/src/components/asset-intercept-rules-editor.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 665 | `web/src/components/calendar/event-calendar-views.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 666 | `web/src/components/copy-button.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 667 | `web/src/components/date-range-picker.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 668 | `web/src/components/exploration-graph.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 669 | `web/src/components/finding-retest-dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 670 | `web/src/components/finding-retest-panel.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 671 | `web/src/components/finding-traffic-panel.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 672 | `web/src/components/http-code-block.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 673 | `web/src/components/link-traffic-dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 674 | `web/src/components/markdown.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 675 | `web/src/components/mention-textarea.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 676 | `web/src/components/scope-text-editor.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 677 | `web/src/components/side-question-workspace.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 678 | `web/src/components/simple-icon.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 679 | `web/src/components/status-badge.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 680 | `web/src/components/table-pagination.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 681 | `web/src/components/task-llm-profile-chain.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 682 | `web/src/components/task-template-controls.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 683 | `web/src/components/todo-popover.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 684 | `web/src/components/traffic-evidence-viewer.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 685 | `web/src/components/traffic-picker-dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 686 | `web/src/components/transcript.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 687 | `web/src/components/ui/accordion.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 688 | `web/src/components/ui/alert-dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 689 | `web/src/components/ui/alert.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 690 | `web/src/components/ui/aspect-ratio.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 691 | `web/src/components/ui/attachment.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 692 | `web/src/components/ui/avatar.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 693 | `web/src/components/ui/badge.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 694 | `web/src/components/ui/breadcrumb.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 695 | `web/src/components/ui/bubble.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 696 | `web/src/components/ui/button-group.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 697 | `web/src/components/ui/button.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 698 | `web/src/components/ui/calendar.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 699 | `web/src/components/ui/card.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 700 | `web/src/components/ui/carousel.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 701 | `web/src/components/ui/chart.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 702 | `web/src/components/ui/checkbox.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 703 | `web/src/components/ui/collapsible.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 704 | `web/src/components/ui/combobox.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 705 | `web/src/components/ui/command.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 706 | `web/src/components/ui/context-menu.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 707 | `web/src/components/ui/dialog.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 708 | `web/src/components/ui/direction.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 709 | `web/src/components/ui/drawer.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 710 | `web/src/components/ui/dropdown-menu.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 711 | `web/src/components/ui/empty.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 712 | `web/src/components/ui/field.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 713 | `web/src/components/ui/hover-card.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 714 | `web/src/components/ui/input-group.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 715 | `web/src/components/ui/input-otp.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 716 | `web/src/components/ui/input.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 717 | `web/src/components/ui/item.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 718 | `web/src/components/ui/kbd.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 719 | `web/src/components/ui/label.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 720 | `web/src/components/ui/marker.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 721 | `web/src/components/ui/menubar.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 722 | `web/src/components/ui/message-scroller.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 723 | `web/src/components/ui/message.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 724 | `web/src/components/ui/native-select.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 725 | `web/src/components/ui/navigation-menu.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 726 | `web/src/components/ui/pagination.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 727 | `web/src/components/ui/popover.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 728 | `web/src/components/ui/progress.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 729 | `web/src/components/ui/radio-group.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 730 | `web/src/components/ui/resizable.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 731 | `web/src/components/ui/scroll-area.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 732 | `web/src/components/ui/select.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 733 | `web/src/components/ui/separator.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 734 | `web/src/components/ui/sheet.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 735 | `web/src/components/ui/sidebar.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 736 | `web/src/components/ui/skeleton.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 737 | `web/src/components/ui/slider.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 738 | `web/src/components/ui/sonner.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 739 | `web/src/components/ui/sortable-head.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 740 | `web/src/components/ui/spinner.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 741 | `web/src/components/ui/switch.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 742 | `web/src/components/ui/table.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 743 | `web/src/components/ui/tabs.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 744 | `web/src/components/ui/textarea.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 745 | `web/src/components/ui/toggle-group.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 746 | `web/src/components/ui/toggle.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 747 | `web/src/components/ui/tooltip.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 748 | `web/src/config/app-config.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 749 | `web/src/hooks/use-current-user.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 750 | `web/src/hooks/use-lg.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 751 | `web/src/hooks/use-mobile.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 752 | `web/src/hooks/use-side-questions.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 753 | `web/src/lib/activity-merge.test.mjs` | Frontend source (Next.js) | Preserved as presentation layer |
| 754 | `web/src/lib/activity-merge.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 755 | `web/src/lib/api.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 756 | `web/src/lib/auth.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 757 | `web/src/lib/chat-mentions.test.mjs` | Frontend source (Next.js) | Preserved as presentation layer |
| 758 | `web/src/lib/chat-mentions.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 759 | `web/src/lib/chat-send-mode.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 760 | `web/src/lib/company-scope.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 761 | `web/src/lib/cookie.client.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 762 | `web/src/lib/fonts/registry.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 763 | `web/src/lib/local-storage.client.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 764 | `web/src/lib/mock/data.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 765 | `web/src/lib/mock/enabled.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 766 | `web/src/lib/mock/handler.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 767 | `web/src/lib/preferences/layout-utils.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 768 | `web/src/lib/preferences/layout.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 769 | `web/src/lib/preferences/preferences-config.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 770 | `web/src/lib/preferences/preferences-storage.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 771 | `web/src/lib/preferences/theme-utils.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 772 | `web/src/lib/preferences/theme.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 773 | `web/src/lib/side-questions.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 774 | `web/src/lib/sort-preference.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 775 | `web/src/lib/status.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 776 | `web/src/lib/task-assets.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 777 | `web/src/lib/types.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 778 | `web/src/lib/utils.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 779 | `web/src/navigation/sidebar/sidebar-items.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 780 | `web/src/proxy.disabled.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 781 | `web/src/proxy.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 782 | `web/src/scripts/generate-theme-presets.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 783 | `web/src/scripts/theme-boot.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 784 | `web/src/stores/preferences/preferences-provider.tsx` | Frontend source (Next.js) | Preserved as presentation layer |
| 785 | `web/src/stores/preferences/preferences-store.ts` | Frontend source (Next.js) | Preserved as presentation layer |
| 786 | `web/src/styles/flag-icons/flags.css` | Frontend source (Next.js) | Preserved as presentation layer |
| 787 | `web/src/styles/presets/brutalist.css` | Frontend source (Next.js) | Preserved as presentation layer |
| 788 | `web/src/styles/presets/soft-pop.css` | Frontend source (Next.js) | Preserved as presentation layer |
| 789 | `web/src/styles/presets/tangerine.css` | Frontend source (Next.js) | Preserved as presentation layer |
| 790 | `web/tsconfig.json` | Frontend source (Next.js) | Preserved as presentation layer |
| 791 | `web/tsconfig.scripts.json` | Frontend source (Next.js) | Preserved as presentation layer |
