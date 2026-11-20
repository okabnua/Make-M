# ADDENDUM — EXHAUSTIVE SOURCE AUDIT & HIDDEN COMPONENT DISCOVERY

This addendum extends the ARTEX Rust-Native Rebuild specification.

It is mandatory and takes precedence over any incomplete component lists in the original migration prompt.

## A. COMPLETE REPOSITORY ACCOUNTING

Do not assume the documented modules are the complete application.

Use Git-based repository discovery:

- `git ls-files`
- Recursive source inspection.
- Import and dependency analysis.
- Runtime entry-point analysis.
- API route extraction.
- Database migration extraction.
- Configuration key extraction.
- Frontend route and component extraction.
- Script and CI workflow inspection.

Inspect every tracked file and determine its role.

Classify files as:

- Runtime source.
- Frontend source.
- Browser extension.
- Database schema.
- Configuration.
- Deployment.
- Build tooling.
- Test and fixture.
- Documentation.
- Security policy.
- Generated artifact.
- Development tooling.
- Licensing and legal.
- Other.

Every file must have an explicit disposition.

Nothing is allowed to disappear simply because it appears unimportant.

## B. ADDITIONAL ARTEX FUNCTIONALITY AUDIT

Explicitly discover and verify:

1. First-run setup token generation and validation.
2. Initial administrator or user claim.
3. Login sessions and authentication lifecycle.
4. Password reset functionality.
5. Login rate limiting.
6. Trusted reverse proxy configuration.
7. Company and user ownership relationships.
8. Task categories and templates.
9. Task relations and archives.
10. Main agent conversations.
11. Goal decomposition.
12. Side questions.
13. Judge and compactor LLM roles.
14. LLM usage accounting and cache metrics.
15. Multi-provider worker rotation semantics.
16. Provider failover and exclusion behavior.
17. Persistent worker todolists.
18. Cross-worker trace retrieval.
19. Graph-change debounce.
20. Frontier intent claiming and dispatch.
21. Scope coverage denominator.
22. Target gate behavior and defaults.
23. Interception decision attribution.
24. Exact task and intent budget behavior.
25. Forced wrap-up and resumed-task semantics.
26. Runtime noise profile controls.
27. Scanner-specific low-and-slow guidance.
28. Prompt-injection boundaries.
29. Generated MITM CA lifecycle and trust.
30. HTTP decompression and evidence quote verification.
31. Browser extension authentication and capture lifecycle.
32. Traffic replay and binary response handling.
33. Encrypted traffic snapshots.
34. Reporting and export proof-strength semantics.
35. MCP Streamable HTTP.
36. Legacy MCP SSE and session messaging.
37. Web search providers and encrypted configuration.
38. ScopeSentry synchronization.
39. Scanner tool availability and versions.
40. Skills and data persistence.
41. Installation and upgrade workflows.
42. Password reset and recovery workflows.
43. Background daemon restart behavior.
44. Reverse proxy and SSE compatibility.
45. Cross-platform builds.
46. Release checksums, SBOM and provenance.

For each item, inspect the actual implementation and determine whether it exists, its precise behavior, its configuration, and its dependencies.

Do not assume that a README description fully explains its implementation.

## C. EXACT BEHAVIORAL SEMANTICS

Record the original behavior of:

- Defaults.
- Optional values.
- Missing values.
- Null values.
- Invalid input.
- Authentication failures.
- Permission failures.
- Database conflicts.
- Concurrent requests.
- Duplicate requests.
- Retry conditions.
- Timeout conditions.
- Partial failures.
- Restart recovery.
- Shutdown behavior.
- Ordering guarantees.
- Persistence guarantees.
- External service failures.

Preserve important constants, limits, API defaults and state transitions.

Do not replace precise behavior with generic implementations.

## D. RUST-ANYTHING WHOLE-APPLICATION WORKFLOW

Follow the installed rust-anything documentation and actual CLI.

For the HTTP backend, use the documented whole-application sequence:

1. `rust-anything analyze ORIGINAL_REPO --whole-app`
2. `rust-anything app record ORIGINAL_REPO --http -- VERIFIED_SERVER_COMMAND`
3. `rust-anything app replay ORIGINAL_REPO`
4. `rust-anything app front-door ORIGINAL_REPO`
5. Migrate and verify individual units.
6. Wire verified routes and units using the supported `app wire` workflow.
7. Replay the entire application after every integration wave.
8. Reduce original-language fallback to zero.
9. Verify every unit gate.
10. Execute `rust-anything app lift` only after every prerequisite passes.

The actual server command, route definitions and working directory must be discovered from the original repository.

For individual Go packages, use the supported Go bridge workflow where appropriate:

- Analyze package dependencies.
- Generate bridge.
- Record original behavior.
- Build Rust implementation.
- Differential test.
- Benchmark.
- Shadow.
- Gate.

Do not confuse package-level bridging with whole-application HTTP migration.

Use black-box app recording for application contracts and explicit integration tests for side effects not observed by the recorder.

Do not invent rust-anything CLI options.

## E. UNOBSERVED BEHAVIOR REQUIREMENTS

A successful recording is not proof that every possible behavior is preserved.

Supplement recordings with:

- Original project tests.
- Custom HTTP request corpora.
- Database state comparisons.
- Filesystem state comparisons.
- Subprocess behavior tests.
- External service contract tests.
- Concurrency stress tests.
- Restart and recovery tests.
- Authentication and authorization matrices.
- Security regression tests.
- Property-based tests.
- Fuzz tests where appropriate.

Every important side effect must have an independent verification method.

Maintain a list of behaviors that cannot be fully proven through differential recording.

## F. FINAL SOURCE DISPOSITION

Before final cutover, generate `rust/SOURCE-DISPOSITION.md`.

For every tracked source file, document:

- Original path.
- File type.
- Purpose.
- Rust equivalent or preserved asset.
- Whether it remains in the final repository.
- Whether it remains in the final runtime.
- Verification evidence.
- Reason for any removal.

A removal requires documented justification.

No tracked file can disappear silently.

## G. RELEASE AND DEPLOYMENT PARITY

Verify the final application supports all required original operational workflows.

Include:

- Docker and Docker Compose.
- Native Linux builds.
- Windows builds.
- Startup and shutdown.
- Restart supervision.
- Setup wizard.
- Configuration migration.
- PostgreSQL backup and restore.
- Persistent data directories.
- Scanner installation and verification.
- Reverse proxy deployment.
- HTTPS and SSE.
- Release archives.
- Checksums.
- SBOM.
- Build provenance.
- Upgrade and rollback.

Document any intentional differences.

## H. COMPLETION REQUIREMENT

The application is NOT complete merely because:

- Cargo build succeeds.
- The homepage loads.
- Authentication works.
- One task executes.
- One LLM provider works.
- One scanner works.
- The database connects.
- Basic HTTP tests pass.

Completion requires full source accounting, feature parity, integration parity, security parity, data compatibility, and verified deployment behavior.

Continue discovering missing components until the repository inventory, dependency graph, runtime graph, and feature matrix agree.

Any discrepancy must be resolved before final certification.