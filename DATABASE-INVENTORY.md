# DATABASE-INVENTORY — ARTEX PostgreSQL schema

Source: `ARTEX/db/schema.sql` (~1610 lines) + `ARTEX/db/migrations/0001_llm_records.sql`, `0002_llm_usage.sql`. Applied idempotently at startup (`CREATE ... IF NOT EXISTS`); versioned migrations checksummed in `schema_migrations`. Primary store is **PostgreSQL**; only `traffic/` uses a sidecar **SQLite** FTS index.

Rust target: `crates/migrations` embeds the schema via `sqlx::migrate!`; `crates/database` uses sqlx with a Postgres pool. Migration strategy: keep table/column names, types, constraints, indexes identical so the original DB remains compatible during transition.

## Tables (by domain)

### A. Asset layer
- **companies** (id, name, nkey, logo)
- **assets** (id, type ∈ root_domain/ip/subdomain/app/service/endpoint, company_id, company_source, task_ids[], domain, ip, c_segment, port, technologies[], open_ports[], url, service_*, page_title, auth/params JSONB[], extra …)
- **company_scope** (company_id, kind ∈ domain/ip/cidr/icp/keyword, domain/net/value)

### B. Reasoning exploration layer
- **explorations** (id, description, goal, status, round_no)
- **exploration_nodes** (id, exploration_id, kind ∈ begin/goal/intent/fact/finding/hint/digest, payload JSONB, priority, state, origin, owner, blocked_reason, delete_reason, content_version, cold_since_round, last_heartbeat_at, unclaim_token)
- **exploration_edges** (exploration_id, src_id, dst_id, rel ∈ spawns/derived_from/yields/proves/covers)
- **exploration_anchors** (node_id, asset_id)
- **task_constraints** (exploration_id, kind ∈ allow/deny, text, origin)
- **activity** (exploration_id, node_id, worker, kind, tool, tool_use_id, is_error, summary, detail, metadata JSONB, tokens, main_seg)
- **main_sessions** (exploration_id, seq)

### C. LLM profiles / settings
- **settings** (key, value) — generic KV (auth hash, feature flags, API keys)
- **llm_profiles** (name, format, base_url, proxy, model, api_key, rate limits, context_window_k, thinking_type, reasoning_effort, is_default, priority, pool_exclude, streaming, max_tokens, session_header_key, user_agent, retry_* overrides, price_* per-M)
- **llm_profile_health** (profile_id, fails, trips, open_until, last_error) — circuit-breaker persistence
- **task_categories** (name, nkey)

### D. Task layer
- **tasks** (name, category_id, description, goal, exploration_id, status ∈ created/running/paused/done/failed/timeout, paused, queued(+queued_at, queue_mode), llm_profile_id, active_llm_profile_id, llm_chain_revision, llm_rotate, timeout_seconds, plan_heartbeat_seconds, plan_quiet_mode, coverage_enabled, target_gate, pinned_at, first_run_at, deadline_at, paused_at, archived_at, deleted_at, completed_at, end_reason)
- **task_archives** (task_id, state, phase, progress, archive_path, sha256 …)
- **task_templates** (name, nkey, description, goal, category_id, intercept_rules)
- **task_relations** (task_id, source_task_id)
- **task_asset_links** (task_id, asset_id, source, source_summary, source_node_id)
- **task_llm_profiles** (task_id, profile_id, position, status, last_error, exhausted_at)
- **task_scope** (task_id, kind, company_id, domain, net, value, source ∈ auto/agent/manual, reason)

### E. Agents / prompts
- **agents** (key, name, role ∈ goals/main/planner/worker/assistant, builtin, enabled, llm_profile_id, current_prompt_id, max_turns, run_seconds, web_search, interactive_shell, wrapup_prompt/max_turns, task_timeout_wrapup_*, trigger_run_mode/merge_mode/max_parallel)
- **agent_prompts** (agent_id, version, template_text, note, updated_by)
- **agent_prompt_vars** (agent_id, var_name, description, example, source)

### F. MCP
- **mcp_servers** (name, transport ∈ stdio/http/sse, command, args, env, url, enabled, insecure, timeout_seconds, last_connect_*)
- **mcp_tools_cache** (server_id, tool_name, description, schema)

### G. Visibility / usage ledgers
- **agent_visibility** (agent_id, resource_kind, resource_id, mcp_tool_name, enabled)
- **agent_skill_visibility** (agent_id, skill_name, enabled)
- **skill_usage** (skill, agent_key, task_id, exploration_id, intent_id, session_id, args_len, found)
- **tool_usage** (tool_key, agent_key, task_id, exploration_id, intent_id, session_id)

### H. Tool catalog
- **tools** (key, system, description, schema, agents[], enabled, kind, exec, deferred)

### I. Sessions (chat)
- **conversations** (agent_key, title, llm_profile_id, pinned_at, target_gate)
- **conversation_activities** (conversation_id, worker, kind, tool, summary, detail, tokens)

### J. Triggers
- **agent_triggers** (agent_key, enabled, interval_sec, on_finding/goal_met/task_timeout/tool_call/task_create, *_message, tool_names, last_fire)
- **scheduler_state** (key, value)

### K. Interception
- **intercept_rules** (name, enabled, priority, match_target ∈ tool_name/tool_input, match_type ∈ string/regex/argv, pattern, action ∈ allow/deny/ask, message, timeout_*)
- **intercept_pending** (rule_id, conversation_id, task_id, agent_name, tool_name, tool_input, status, reason, audit, decision_source, decided_by, target_key)
- **target_pins** (task_id, conversation_id, host, port, version, pinned_ips, status, decided_by, decided_from, name_only_effective, revoked_*)
- **target_gate_exemption_log** (changed_at, changed_by, before, after, digest)

### L. Findings
- **findings** (task_id, node_id, vulnclass, name, severity, summary, evidence, worker, asset_ids, status, report, evidence_version, report_evidence_version, poe_level, verification_status/note, counterevidence, confidence_rationale)
- **finding_retests** (finding_id, conversation_id, status, verdict ∈ reproduced/fixed/inconclusive, snapshot, summary, evidence …)
- **traffic_evidence_snapshots** (id, source_traffic_id, url, method, status, req_head/resp_head, req_hash/resp_hash, unreferenced_at)
- **finding_traffic_bindings** (finding_id, snapshot_id, role, note, position)

### M. Backend logs
- **server_logs** (level, tag, text)

### N. Side questions
- **side_question_sessions** (session_key, conversation_id/task_id/exploration_id/intent_id, run_id, version, snapshot, generation, memory)
- **side_question_requests** (id, session_key, generation, client_id, question, answer, status, model, sequence, usage, context_info)

### O. Asset interception (global + per-task)
- **asset_intercept_rules** (enabled, kind ∈ exact_domain/exact_ip/exact_url/fuzzy_*/cidr, pattern, note, builtin)
- **task_intercept_rules** (task_id, enabled, action ∈ block/allow, kind, pattern, note)
- **task_credentials** (task_id, domain, username, password, source ∈ user/agent, note)

### P. Notification
- **notification_channels** (name, kind ∈ dingtalk/feishu/wecom/webhook/telegram/email, enabled, config JSONB, mode ∈ realtime/digest, filter JSONB, rate_per_min)
- **notification_events** (kind, finding_id, snapshot, fanned_out)
- **notification_deliveries** (event_id, channel_id, state, attempts, next_attempt_at, last_error, batch_id)

### Q. RSI lessons / coverage
- **rsi_lessons** (dedupe_key, kind ∈ positive/negative/caution, cue, action, category_id, company_id, status)
- **rsi_lesson_evidence** (lesson_id, source_kind, source_id, detail)
- **rsi_lesson_usage** (lesson_id, task_id, mode ∈ shadow/inject, shown_at)
- **coverage_ledger** (task_id, surface, risk_area, outcome, evidence, reason, worker, intent_id)
- **coverage_ledger_history** (… recorded_at)

### R. Migrations / records
- **schema_migrations** (version, name, checksum, applied_at)
- **llm_records** (migration 0001) — full LLM request/response
- **llm_usage** (migration 0002) — token usage accounting

## Sidecar (traffic SQLite, not Postgres)
`traffic/_index/index.sqlite` (FTS, WAL, auto_vacuum) + `_blobs/` content-addressed store. Rust: `crates/traffic` uses `rusqlite` with the same FTS5 schema.

## Secret encryption
AES-256-GCM at rest for: `brave_search_api_key`, `tavily_search_api_key`, `web_search_proxy`, `global_proxy`, and custom-tool `tools.exec`. Key in `keys/secret.key` (0600) or `ARTEX_SECRET_KEY` env. Rust: `aes-gcm` crate.