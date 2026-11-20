# MASTER PROMPT — ARTEX RUST-NATIVE FULL REBUILD

## ROLE

Act as a Principal Rust Systems Architect, Senior Reverse Engineer, Software Migration Engineer, QA Engineer, and Security Platform Engineer.

Your mission is to rebuild the complete ARTEX application using Rust as its primary and final backend implementation, using `rust-anything` as the behavioral-preserving migration and verification framework.

### SOURCE REPOSITORIES

Original application:
https://github.com/BaaRai/ARTEX

Migration framework:
https://github.com/erkamyaman/rust-anything

### PRIMARY OBJECTIVE

Rebuild ARTEX into a production-ready Rust-native application while preserving all original functionality, behavior, features, integrations, API contracts, database semantics, workflows, and user-facing capabilities.

This is a complete system migration, NOT:
- A simplified rewrite.
- A partial port.
- A new application inspired by ARTEX.
- A redesign that removes existing capabilities.
- A wrapper that simply launches the original Go binary.
- A mock implementation that only looks functional.

The final backend must execute natively in Rust.

The original ARTEX repository is the source of truth for behavior and functionality.

---

# PHASE 0 — NON-NEGOTIABLE RULES

1. NEVER modify or destroy the original ARTEX source repository.
2. Create a separate Rust rebuild workspace.
3. Never delete an original component before its Rust replacement is implemented and verified.
4. Never silently omit, simplify, disable, or replace a feature.
5. Never invent functionality that does not exist in the original.
6. Never use mock data as a substitute for real functionality.
7. Never mark an incomplete module as completed.
8. Never use `TODO`, placeholder handlers, fake responses, or empty implementations to satisfy a migration checklist.
9. Preserve original API contracts unless a documented incompatibility is explicitly approved.
10. Preserve database compatibility, relationships, migrations, indexes, constraints, and transaction semantics.
11. Preserve authentication, authorization, security controls, and approval workflows.
12. Preserve concurrency, event handling, asynchronous behavior, and state transitions.
13. Preserve error handling and observable behavior.
14. Every migration unit must have recorded behavioral tests.
15. Every Rust implementation must pass differential verification before replacing its original counterpart.
16. Never claim complete migration while fallback to Go remains.
17. Never fabricate test results, coverage percentages, benchmarks, or completed features.
18. Do not overwrite configuration, credentials, or existing agent settings.
19. Never run recording or replay against production infrastructure or production credentials.
20. Never run active security tests against unauthorized targets.

If an implementation decision is unclear, investigate the source code, tests, documentation, and dependencies first.

Ask the user only when a genuinely blocking architectural decision cannot be established from the source or project requirements.

Do not repeatedly stop and ask questions when you can safely investigate and continue.

---

# PHASE 1 — REPOSITORY DISCOVERY AND SOURCE PRESERVATION

Before writing Rust code:

1. Inspect the complete ARTEX repository.
2. Inspect the complete rust-anything repository and its installed documentation.
3. Identify the exact versions, commits, branches, licenses, dependencies, and runtime requirements.
4. Read every source directory and relevant configuration file.
5. Inspect build scripts, Dockerfiles, CI workflows, startup scripts, migrations, environment variables, and deployment configurations.
6. Identify generated code and embedded assets.
7. Identify all external integrations and runtime dependencies.
8. Identify all background processes, goroutines, event loops, schedulers, and worker lifecycles.
9. Identify every API endpoint, WebSocket, SSE stream, webhook, MCP interface, and internal communication protocol.
10. Identify every frontend-to-backend interaction.
11. Identify every database read, write, transaction, trigger, and migration.
12. Identify every filesystem operation, subprocess, network operation, and external tool invocation.

Do not rely on README descriptions alone.

Read the implementation itself.

### Source integrity

Create an immutable reference copy or Git worktree of the original.

Record:
- Original commit SHA.
- Git status.
- Repository tree.
- Dependency lockfiles.
- Build configuration.
- Test results.
- Existing warnings and known failures.

Create a new destination:

`artex-rust/`

Never perform the rebuild inside the original source directory.

---

# PHASE 2 — COMPLETE SYSTEM DECOMPOSITION

This is the most important phase.

Perform a complete architectural teardown of ARTEX.

Do not start migration until the inventory is sufficiently complete and all discovered components have been assigned a status.

Inspect every directory, package, source file, public interface, and runtime dependency.

### Mandatory source inventory

At minimum, explicitly investigate these known ARTEX components:

- `agent/`
- `cmd/artex/`
- `config/`
- `db/`
- `egress/`
- `enrich/`
- `evidence/`
- `extension/`
- `guard/`
- `intercept/`
- `llmpool/`
- `llmrec/`
- `mcphttp/`
- `notify/`
- `report/`
- `screenshots/`
- `scripts/`
- `server/`
- `sidequestion/`
- `skills/`
- `traffic/`
- `web/`

Also inspect:
- Dockerfile.
- docker-compose files.
- `.env.example`.
- `go.mod` and `go.sum`.
- Frontend package manifests and lockfiles.
- Build and installation scripts.
- Configuration examples.
- Database migrations.
- Documentation.
- Security policies.
- CI/CD workflows.
- Browser extension manifests and source.
- Embedded UI assets.
- Tests, fixtures, and test utilities.

The list above is an initial checklist, NOT an exhaustive declaration of the repository.

Discover additional directories and components automatically.

### Component decomposition requirements

For every discovered component, document:

1. Component name.
2. Original source path.
3. Responsibility.
4. Public interfaces.
5. Internal dependencies.
6. External dependencies.
7. Input and output contracts.
8. Database dependencies.
9. Configuration requirements.
10. Side effects.
11. Concurrency behavior.
12. Error behavior.
13. Security boundaries.
14. Existing tests.
15. Rust replacement strategy.
16. Verification requirements.
17. Migration status.

### Build dependency maps

Create:
- Complete module dependency graph.
- Runtime execution graph.
- Database dependency graph.
- API dependency graph.
- Event and message flow graph.
- External integration graph.
- Frontend/backend interaction graph.
- Security boundary graph.

Identify circular dependencies and modules that must migrate together.

### Required documentation

Create:

`rust/INVENTORY.md`
`rust/ARCHITECTURE.md`
`rust/DEPENDENCY-MAP.md`
`rust/API-INVENTORY.md`
`rust/DATABASE-INVENTORY.md`
`rust/CONFIG-INVENTORY.md`
`rust/INTEGRATION-INVENTORY.md`
`rust/SECURITY-INVENTORY.md`
`rust/TEST-INVENTORY.md`
`rust/FEATURE-PARITY.md`

Every discovered source component must appear in the inventory.

No undocumented component is allowed to disappear from the migration plan.

---

# PHASE 3 — FEATURE PARITY MATRIX

Build a comprehensive feature-to-implementation matrix.

Investigate and account for every existing feature, including:

## A. Autonomous Agent Engine

- Main agent.
- Planner.
- Worker agents.
- Intent creation.
- Intent dispatch.
- Intent claiming.
- Worker execution.
- Worker lifecycle.
- Task lifecycle.
- Autonomous exploration loop.
- Goal evaluation.
- Goal completion.
- Task resumption.
- Worker coordination.
- Shared worker memory.
- Cross-worker trace searching.
- Task todolist.
- Facts and observations.
- Hints and derived information.
- Error recovery.
- Retry behavior.
- Cancellation.
- Timeout handling.

## B. Dual Graph Architecture

- Shared asset graph.
- Per-task exploration graph.
- Graph nodes.
- Graph edges.
- Graph anchors.
- Parent-child asset relationships.
- Asset deduplication.
- Asset ownership.
- Asset coverage.
- Exploration lineage.
- Graph traversal.
- Graph updates.
- Graph queries.
- Coverage calculations.

Preserve the semantics of all node and relationship types.

## C. LLM Infrastructure

- LLM provider management.
- Anthropic integration.
- OpenAI integration.
- OpenAI-compatible providers.
- Ollama integration.
- Custom base URLs.
- API key management.
- Provider profiles.
- Model configuration.
- Provider rotation.
- Provider failover.
- Retry policies.
- Timeout policies.
- Streaming responses.
- Token accounting.
- Prompt construction.
- Context management.
- Usage records.
- Cache accounting.
- Per-task LLM configuration.
- Per-worker model configuration.
- Concurrent model calls.

Verify every provider-specific protocol and error behavior.

## D. Security Tool Execution

- Tool registry.
- Tool discovery.
- Tool invocation.
- Bash execution.
- HTTP tools.
- Browser interaction.
- Scanner integrations.
- Argument validation.
- Process execution.
- Output capture.
- Exit-code handling.
- Timeout enforcement.
- Process cancellation.
- Tool approval.
- Execution logging.
- Tool result normalization.
- Untrusted output framing.

Preserve real integrations rather than replacing them with simulated tools.

## E. Security and Safety Controls

- Target scope enforcement.
- Deny rules.
- Destination restrictions.
- Human approval.
- Interception rules.
- String matching.
- Regex matching.
- Argument-prefix matching.
- Approval decision attribution.
- Token budgets.
- Turn budgets.
- Intent budgets.
- Noise governor.
- Request pacing.
- Burst limits.
- Concurrency limits.
- Jitter.
- Backoff.
- Retry-After handling.
- Prompt-injection boundaries.
- Credential protection.
- Secret encryption.
- Audit trails.

Security controls must never become weaker during migration.

## F. Traffic Recording and MITM Proxy

- Proxy listener.
- Generated certificate authority.
- Certificate generation.
- TLS interception.
- HTTP request capture.
- HTTP response capture.
- Header preservation.
- Body preservation.
- Compression decoding.
- Binary body handling.
- Request-response association.
- Traffic persistence.
- Traffic retrieval.
- Traffic replay.
- Evidence association.
- Proxy configuration.
- Proxy authentication.
- Certificate trust handling.

Preserve the existing proxy's protocol semantics and compatibility.

## G. Browser Extension

- Manifest.
- Background service worker.
- Content scripts.
- Popup interface.
- Configuration.
- Authentication.
- Token management.
- Capture controls.
- Request interception.
- Response capture.
- Batch uploads.
- Binary filtering.
- Body size limits.
- Error handling.
- ARTEX traffic ingestion API.

Preserve the extension's functionality and communication contracts.

## H. Findings and Evidence

- Finding creation.
- Finding updates.
- Finding classification.
- Severity.
- Confidence.
- Verification status.
- Proof-of-exploit level.
- Evidence quotes.
- HTTP request association.
- HTTP response association.
- Evidence integrity.
- Verification logic.
- Bulk status changes.
- Finding history.
- Evidence retrieval.
- Retest association.

Never mark a finding verified without satisfying its actual evidence requirements.

## I. Retesting

- Retester agent.
- Finding selection.
- Retest instructions.
- Fixed-version conditions.
- Reproduction.
- Still reproducible.
- Fixed.
- Cannot confirm.
- Retest history.
- Automatic status transitions.
- Preservation of original evidence.

## J. Asset Discovery and Enrichment

- Root domains.
- Subdomains.
- IP addresses.
- Ports.
- Services.
- Applications.
- Endpoints.
- Asset relationships.
- Asset deduplication.
- Asset enrichment.
- Coverage tracking.
- External asset synchronization.
- ScopeSentry integration.
- Asset ownership.

## K. MCP and Agent Extensions

- MCP server.
- HTTP transport.
- SSE transport.
- Authentication.
- Session handling.
- JSON-RPC.
- Tool registration.
- Tool invocation.
- Remote integrations.
- Skills.
- Agent memory.
- Workspace.
- Reusable playbooks.

## L. Reporting and Data Export

- Finding reports.
- Evidence reports.
- Task archives.
- Export formats.
- Traffic snapshots.
- Report generation.
- Data integrity.
- Encryption at rest.
- Download functionality.

## M. Notifications and Supporting Services

- Notification providers.
- Event notifications.
- System logs.
- Backend logs.
- LLM records.
- Side questions.
- Screenshots.
- Web search.
- Search providers.
- Egress proxy.
- Configuration management.

## N. Web Interface

Preserve the existing Next.js interface initially.

Account for:
- Dashboard.
- Tasks.
- Task creation.
- Task execution views.
- Findings.
- Evidence.
- Assets.
- Asset graph.
- Exploration graph.
- Traffic recording.
- Traffic details.
- Human approval interface.
- Interception approvals.
- Agent management.
- LLM configuration.
- System settings.
- Logs.
- Reports.
- Archives.
- MCP configuration.
- Skills management.
- Workspace.
- Authentication.
- Setup flow.
- All dialogs, forms, tables, filters, pagination, charts, and interactions.

Do not assume the UI is complete just because the main pages render.

Inventory all routes and interactive components.

## O. Deployment and Operations

- Linux deployment.
- Windows deployment.
- Docker.
- Docker Compose.
- PostgreSQL.
- Installation scripts.
- Startup scripts.
- Shutdown behavior.
- Restart behavior.
- Health checks.
- Environment variables.
- Configuration files.
- Persistent volumes.
- Static asset embedding.
- Release packaging.
- Cross-compilation.
- Upgrade procedures.
- Backup and recovery.
- Database migration.
- Logging.
- Operational diagnostics.

For every feature, create a parity entry with:

`NOT_ANALYZED`
`ANALYZED`
`PLANNED`
`IN_PROGRESS`
`IMPLEMENTED`
`TESTED`
`VERIFIED`
`BLOCKED`

Never mark a feature VERIFIED without evidence.

---

# PHASE 4 — RUST ARCHITECTURE DESIGN

Design the Rust architecture only after source analysis.

Use a modular Rust workspace.

Suggested structure:

rust/
  Cargo.toml
  Cargo.lock
  DECISIONS.md
  GLOSSARY.md
  MAP.md
  HANDOFF.md
  INVENTORY.md
  FEATURE-PARITY.md
  app/
  crates/
    core/
    config/
    database/
    migrations/
    api/
    auth/
    agent/
    planner/
    worker/
    scheduler/
    llm/
    llm-providers/
    tools/
    execution/
    guard/
    scope/
    approval/
    budget/
    graph/
    assets/
    exploration/
    traffic/
    proxy/
    interception/
    evidence/
    findings/
    retester/
    enrichment/
    reporting/
    notifications/
    mcp/
    skills/
    memory/
    workspace/
    search/
    egress/
    logging/
    telemetry/
    integrations/
    compatibility/
  app-cases/
  tests/
  research/
  cutover.sh

This is a proposed starting structure, not permission to force every component into an unnecessary separate crate.

Adjust boundaries based on the actual dependency graph.

### Rust engineering standards

- Stable Rust unless a documented reason requires otherwise.
- Tokio for asynchronous execution where appropriate.
- Axum for HTTP services where appropriate.
- SQLx or an equivalent suitable PostgreSQL library.
- Serde for serialization.
- Tracing for structured observability.
- Explicit error types.
- Strongly typed domain models.
- Controlled concurrency.
- Cancellation-safe asynchronous operations.
- Structured configuration.
- Secure secret management.
- Minimal unsafe Rust.
- No unnecessary dependencies.
- No unnecessary FFI in the final application.

Choose crates based on actual feature requirements, not popularity.

Create ADRs for significant architecture decisions.

---

# PHASE 5 — RUST-ANYTHING MIGRATION WORKFLOW

Use the installed rust-anything version and its actual CLI/MCP documentation.

Do not invent commands or assume features exist in the installed version.

Start with:

`rust-anything --help`

`rust-anything analyze ORIGINAL_REPO --whole-app`

For ARTEX's Go backend, use the appropriate original Go command and HTTP server mode.

For example, after verifying the original startup command:

`rust-anything app record ORIGINAL_REPO --http -- <VERIFIED_GO_SERVER_COMMAND>`

Then:

`rust-anything app replay ORIGINAL_REPO`

The actual command must be derived from the repository.

### Important recording safeguards

Recording executes the original application.

Before recording:

- Create an isolated test environment.
- Use a dedicated test PostgreSQL instance.
- Use disposable test credentials.
- Disable access to production systems.
- Prevent unintended external network operations.
- Use controlled test fixtures.
- Snapshot database state.
- Verify filesystem isolation.
- Verify subprocess isolation.
- Record all meaningful side effects.
- Ensure test cases are repeatable.

Do not assume HTTP response replay alone captures database mutations, network effects, process behavior, or external side effects.

Build supplementary integration tests for these behaviors.

### Migration loop

For every eligible migration unit:

1. Analyze.
2. Identify dependencies.
3. Research dependency semantics.
4. Design Rust interfaces.
5. Create bridge where applicable.
6. Record original behavior.
7. Generate semantic edge cases.
8. Implement Rust.
9. Build.
10. Run differential tests.
11. Investigate every mismatch.
12. Fix the implementation.
13. Benchmark.
14. Run shadow comparison where supported.
15. Run gate.
16. Update migration map.
17. Wire the verified unit.
18. Replay the whole application.
19. Update the handoff documentation.

Never skip a failed step.

### Critical limitation

The original ARTEX backend is Go, while the UI and extension are JavaScript/TypeScript.

Use rust-anything only where the installed tool supports the relevant language, interfaces, and runtime.

Do not assume the Go bridge can automatically handle every Go interface, goroutine, callback, database side effect, or operating-system interaction.

Where bridge-based migration is unsuitable, use app-level black-box recording, explicit compatibility adapters, and dedicated integration tests.

The original implementation must remain available as the behavioral reference until migration completion.

---

# PHASE 6 — MIGRATION ORDER

Use dependency-aware migration waves.

Do not migrate randomly.

Suggested order:

### Wave 0 — Foundation

- Project structure.
- Configuration.
- Error handling.
- Logging.
- Runtime initialization.
- Database connection.
- Database migrations.
- Shared domain types.

### Wave 1 — Data and Core Services

- Database repository layer.
- Transactions.
- Authentication.
- Authorization.
- Asset models.
- Finding models.
- Task models.
- Graph persistence.
- Configuration management.

### Wave 2 — Security Infrastructure

- Scope enforcement.
- Deny rules.
- Approval engine.
- Execution policies.
- Budgets.
- Noise governor.
- Secret management.
- Audit trail.

### Wave 3 — LLM Infrastructure

- Provider abstraction.
- Anthropic.
- OpenAI.
- OpenAI-compatible endpoints.
- Ollama.
- Failover.
- Rotation.
- Usage accounting.
- Streaming.
- LLM records.

### Wave 4 — Autonomous Engine

- Agent runtime.
- Planner.
- Intent queue.
- Worker execution.
- Task lifecycle.
- Shared memory.
- Graph-triggered events.
- Autonomous loop.
- Goal completion.
- Recovery and resumption.

### Wave 5 — Tool Execution

- Tool registry.
- Bash execution.
- HTTP execution.
- Browser integration.
- External scanner integrations.
- Output capture.
- Tool approvals.
- Process isolation.

### Wave 6 — Evidence and Traffic

- Recording proxy.
- Certificate handling.
- Traffic storage.
- Traffic replay.
- Evidence management.
- Finding verification.
- Retesting.

### Wave 7 — Platform Integrations

- MCP.
- Skills.
- Memory.
- Workspace.
- Web search.
- Egress.
- Enrichment.
- ScopeSentry.
- Notifications.
- Reporting.

### Wave 8 — API and UI Integration

- Complete API compatibility.
- Authentication flows.
- WebSocket/SSE behavior.
- Static UI embedding.
- All frontend workflows.
- Browser extension compatibility.

### Wave 9 — Deployment and Cutover

- Production build.
- Docker.
- Docker Compose.
- Installation.
- Configuration.
- Cross-platform builds.
- Database upgrade.
- Backup and restore.
- Health checks.
- Release packaging.

The actual dependency analysis takes precedence over this suggested order.

---

# PHASE 7 — FUNCTIONAL AND BEHAVIORAL VERIFICATION

Preserve more than function return values.

Test all observable behavior.

### API parity

Compare:
- HTTP methods.
- Routes.
- Request schemas.
- Response schemas.
- Status codes.
- Headers.
- Authentication.
- Authorization.
- Error responses.
- Pagination.
- Filtering.
- Sorting.
- Streaming.
- WebSocket events.
- SSE events.

### Database parity

Compare:
- Schema.
- Data types.
- Nullability.
- Constraints.
- Indexes.
- Foreign keys.
- Transactions.
- Cascading behavior.
- Migration order.
- Default values.
- Query semantics.
- Data persistence.

### Runtime parity

Compare:
- Startup.
- Shutdown.
- Restart.
- Worker concurrency.
- Event ordering.
- Task state transitions.
- Cancellation.
- Timeout.
- Retry.
- Recovery.
- Background execution.

### Integration parity

Verify actual connectivity with:
- PostgreSQL.
- Every supported LLM provider.
- MCP clients.
- Browser extension.
- External tools.
- Search providers.
- Egress proxies.
- ScopeSentry.
- Notification providers.

### UI parity

Verify:
- Every route.
- Every button.
- Every form.
- Every modal.
- Every interactive component.
- Every API call.
- Every state transition.
- Loading states.
- Error states.
- Empty states.
- Pagination.
- Filtering.
- Realtime updates.

### Security parity

Verify that Rust does not weaken:
- Authentication.
- Authorization.
- Scope restrictions.
- Approval requirements.
- Command validation.
- Network restrictions.
- Secret handling.
- Evidence validation.
- Prompt-injection boundaries.
- Audit logging.

Security tests must run in isolated environments with authorized targets and simulated fixtures.

### Regression testing

Run the original and Rust implementations against the same controlled scenarios.

Any unexplained mismatch blocks migration.

Do not suppress mismatches by weakening tests.

---

# PHASE 8 — PERFORMANCE AND RESOURCE OPTIMIZATION

Only optimize after behavioral parity.

Measure:

- Startup time.
- Memory usage.
- CPU utilization.
- API latency.
- Database query performance.
- LLM orchestration overhead.
- Worker throughput.
- Concurrent task capacity.
- Traffic recording throughput.
- Proxy throughput.
- Graph traversal performance.
- Report generation.
- Large evidence handling.

Use realistic workloads.

Do not sacrifice:
- Correctness.
- Security.
- Evidence integrity.
- API compatibility.
- Task consistency.
- Reliability.

Rust-native does not automatically mean faster.

Benchmark the complete execution path, not isolated functions alone.

---

# PHASE 9 — CONTINUOUS MIGRATION MANAGEMENT

Maintain these files throughout the project:

`rust/MAP.md`
`rust/HANDOFF.md`
`rust/FEATURE-PARITY.md`
`rust/DECISIONS.md`
`rust/GLOSSARY.md`
`rust/INVENTORY.md`

### MAP.md

For every module:

- Original path.
- Rust path.
- Dependencies.
- Migration status.
- Test status.
- Gate status.
- Known mismatches.
- Blockers.
- Next action.

### HANDOFF.md

Record:
- Completed work.
- Current migration unit.
- Files modified.
- Commands executed.
- Test results.
- Remaining failures.
- Exact next steps.

This is mandatory for session continuity.

### Feature parity

Every original feature must have:
- A unique identifier.
- A source reference.
- A Rust destination.
- A test reference.
- A verification result.

No feature can disappear from the matrix.

---

# PHASE 10 — AUTONOMOUS EXECUTION POLICY

Operate as a persistent engineering agent.

Do not stop merely because:
- One module is difficult.
- A test fails.
- A dependency is unavailable.
- An implementation attempt fails.
- A migration approach proves unsuitable.
- A mismatch is discovered.

When blocked:

1. Identify the exact blocker.
2. Investigate its root cause.
3. Inspect the original implementation.
4. Research alternative Rust dependencies.
5. Evaluate another architecture.
6. Try a different implementation strategy.
7. Create additional tests.
8. Record the result.
9. Continue with independent migration units where safe.

Do not repeat failed approaches without learning from previous failures.

Use parallel subagents only for independent, clearly bounded tasks.

Never let two agents modify the same migration unit simultaneously.

Assign one integration owner responsible for reviewing and merging all changes.

Do not fabricate completion to satisfy the user.

If a genuine external dependency or user decision blocks progress, document the blocker precisely and continue other independent work.

---

# PHASE 11 — FINAL CUTOVER REQUIREMENTS

The rebuild is not complete until all of these conditions hold:

1. Every original source component is accounted for.
2. Every original feature has a verified Rust implementation or an explicitly approved retained non-backend asset.
3. All required backend execution paths run in Rust.
4. All original Go backend runtime dependencies have been removed from the final runtime.
5. No hidden Go fallback remains.
6. No original executable is required.
7. No original backend service is launched as a subprocess.
8. No fake implementation remains.
9. All required API contracts pass.
10. All database compatibility tests pass.
11. All integration tests pass.
12. All security regression tests pass.
13. All required UI workflows pass.
14. Browser extension compatibility is verified.
15. All required recorded behavioral cases pass.
16. All Rust migration units pass their gates.
17. Whole-application replay passes.
18. No unresolved critical mismatch remains.
19. Build, formatting, linting, and tests pass.
20. Deployment and rollback procedures are documented.

For rust-anything:

- All units must pass their required gates.
- Whole-application fallback must reach zero.
- App replay must pass.
- `rust-anything app lift` must only run after all its actual prerequisites are satisfied.

Perform the final lift on a copy.

Never modify or delete the original ARTEX repository.

If the frontend remains Next.js, explicitly document it as a preserved presentation layer, not a Rust backend dependency.

Do not falsely claim the entire application is 100% Rust if browser-side JavaScript or TypeScript assets remain.

---

# FINAL DELIVERABLES

Produce:

1. Complete source inventory.
2. Original architecture documentation.
3. Rust architecture documentation.
4. Dependency graph.
5. API compatibility matrix.
6. Database compatibility report.
7. Feature parity matrix.
8. Migration status map.
9. Rust workspace.
10. Complete Rust backend source.
11. Preserved frontend assets.
12. Preserved browser extension.
13. Database migrations.
14. Integration tests.
15. Differential test cases.
16. Security regression tests.
17. Benchmark reports.
18. Dockerfile.
19. Docker Compose configuration.
20. Installation scripts.
21. Configuration examples.
22. Deployment documentation.
23. Backup and rollback instructions.
24. Final migration verification report.

---

# FIRST EXECUTION TASK

Do not immediately start coding.

Start by performing these actions in order:

1. Verify both repositories exist.
2. Verify the installed rust-anything version.
3. Verify the original ARTEX commit and working tree.
4. Read the complete rust-anything workflow documentation.
5. Run repository-wide source discovery.
6. Inspect the original Go entry point and HTTP server.
7. Inspect all frontend and extension entry points.
8. Inspect database schema and migrations.
9. Build the complete component inventory.
10. Build the feature parity matrix.
11. Run the original project's existing tests.
12. Establish a clean baseline.
13. Execute `rust-anything analyze --whole-app` with the verified repository path and correct CLI syntax.
14. Determine which components can use unit-level recording and which require app-level behavioral verification.
15. Produce the complete migration roadmap.
16. Begin implementation only after the discovery deliverables are sufficiently complete.

After the discovery phase, continue implementing the migration without waiting for another prompt, provided no genuinely blocking user decision is required.

# DEFINITION OF SUCCESS

The final result must be a fully functioning Rust-native backend rebuild of ARTEX, preserving the original system's complete verified capabilities.

The original ARTEX is the behavioral reference.

The Rust implementation is the final product.

No component is allowed to disappear silently.

No feature is allowed to be considered complete without implementation and evidence.

**DISCOVER EVERYTHING → DOCUMENT EVERYTHING → RECORD EVERYTHING → MIGRATE EVERYTHING → VERIFY EVERYTHING → LIFT TO RUST.**

Correctness and completeness take priority over migration speed.
