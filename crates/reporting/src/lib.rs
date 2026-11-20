//! `reporting` crate — Markdown/CSV report generation.
//!
//! Behaviour-preserving port of the Go `report` package (`ARTEX/report`,
//! 369 lines across `report.go`, `findings.go` and `report_test.go`). The
//! package is pure logic: it renders deliverables from in-memory findings and
//! never touches the database, so callers build [`FindingReportInput`] values
//! from whatever data layer they use.
//!
//! # Mapping to the Go source
//!
//! | Go function                         | Rust function                      |
//! | ----------------------------------- | ---------------------------------- |
//! | `report.Markdown` (`report.go`)     | [`render_task_markdown`]           |
//! | `report.FindingsMarkdown`           | [`render_markdown`]                |
//! | `report.SingleFindingMarkdown`      | [`render_single_finding_markdown`] |
//! | `report.FindingFilename`            | [`finding_filename`]               |
//! | `report.FindingsCSV`                | [`render_csv`] / [`render_csv_to`] |
//!
//! [`render_markdown`] is the batch "findings summary report" the task asks
//! for: it has the H1 heading, a severity-count summary table (the metadata
//! table), findings grouped/sorted by severity, and fenced evidence quotes.
//! [`render_task_markdown`] is the per-task report from `report.go` (title,
//! goal, asset counts, PoC evidence and the coverage-gap section).
//!
//! # Behaviour-preserving notes
//!
//! - Severity ranking is `critical=0, high=1, medium=2, low=3, ""=4`. Go's
//!   `sevRank` is a map lookup, so an *unknown* non-empty severity (e.g.
//!   `info`) yields the zero value `0` and sorts as if critical. That quirk is
//!   reproduced exactly in [`sev_rank`].
//! - Findings are sorted with a **stable** sort (Go `sort.SliceStable`).
//! - CSV output starts with a UTF-8 BOM (`\u{feff}`) and uses Go
//!   `encoding/csv` quoting rules (quote on `,`, `"`, `\r`, `\n`, a leading
//!   whitespace rune, or the literal `\.`), with `\n` record terminators.
//! - `include_evidence` / `include_assets` / `include_coverage` are extensions
//!   requested by the port; their defaults (`true`) reproduce Go's output.
//! - `FindingReportInput` also carries `company_name` and `assets` so a caller
//!   can pass per-finding context. The Go renderers do not emit them, so they
//!   are intentionally not printed (preserving the exact Go format).
//! - `finding_filename` truncates on a UTF-8 char boundary rather than Go's
//!   raw byte slice (Go can emit invalid UTF-8 for long non-ASCII names; a Rust
//!   `String` cannot).

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};

/// Caps the coverage-gap list in the task report; the rest is counted
/// (Go `reportMaxGaps`).
pub const REPORT_MAX_GAPS: usize = 200;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the reporting crate.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// A report input was structurally invalid.
    #[error("invalid report input: {0}")]
    InvalidInput(String),
    /// Writing the rendered report to an [`std::io::Write`] sink failed.
    #[error("report render failed: {0}")]
    Render(String),
}

// ---------------------------------------------------------------------------
// Input model (mirrors the Go structs the renderers consume)
// ---------------------------------------------------------------------------

/// One traffic-evidence binding attached to a finding.
///
/// Mirrors the subset of Go `db.FindingTrafficBinding` the renderers read
/// (`ID`, `Role`, `Note`, `Snapshot.Method`, `Snapshot.URL`,
/// `Snapshot.Status`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrafficBinding {
    /// Binding id (Go `FindingTrafficBinding.ID`).
    pub id: i64,
    /// Role of the evidence, e.g. `request` / `response` (Go `Role`).
    pub role: String,
    /// Free-form note (Go `Note`).
    pub note: String,
    /// HTTP method from the snapshot (Go `Snapshot.Method`).
    pub method: String,
    /// URL from the snapshot (Go `Snapshot.URL`).
    pub url: String,
    /// HTTP status from the snapshot (Go `Snapshot.Status`).
    pub status: i64,
}

/// One coverage-ledger entry.
///
/// Mirrors the fields of Go `db.CoverageEntry`; the renderers only use the
/// entry count, but the full struct is kept so callers can build it directly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageEntry {
    /// Tested surface, e.g. `/login` (Go `Surface`).
    pub surface: String,
    /// Risk area, e.g. `sqli` (Go `RiskArea`).
    pub risk_area: String,
    /// Outcome, e.g. `needs_follow_up` (Go `Outcome`).
    pub outcome: String,
    /// Evidence text (Go `Evidence`).
    pub evidence: String,
    /// Reason text (Go `Reason`).
    pub reason: String,
}

/// One open coverage gap (Go `db.CoverageGap`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageGap {
    /// Gap kind, e.g. `needs_follow_up` / `unrecorded_risk_class`.
    pub kind: String,
    /// Surface; empty means "all surfaces" (Go `Surface`).
    pub surface: String,
    /// Risk area (Go `RiskArea`).
    pub risk_area: String,
}

impl fmt::Display for CoverageGap {
    /// Formats exactly like Go `CoverageGap.String()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.surface.is_empty() {
            write!(f, "{} {}", self.kind, self.risk_area)
        } else {
            write!(f, "{} {} {}", self.kind, self.surface, self.risk_area)
        }
    }
}

/// Coverage completeness block (Go `db.CoverageCompleteness`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageCompleteness {
    /// Whether the run completed (Go `Complete`).
    pub complete: bool,
    /// State: `complete` / `in_progress` / `paused` / `timeout` / `budget` /
    /// `aborted` (Go `State`).
    pub state: String,
    /// Always-set caveat (Go `Caveat`).
    pub caveat: String,
}

/// Coverage ledger (Go `db.CoverageLedger`), `None` when the task's coverage
/// feature is off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageLedger {
    /// Recorded ledger entries (Go `Entries`).
    pub entries: Vec<CoverageEntry>,
    /// Open gaps (Go `Gaps`).
    pub gaps: Vec<CoverageGap>,
    /// Completeness block (Go `Completeness`).
    pub completeness: Option<CoverageCompleteness>,
}

/// A single finding as consumed by the renderers.
///
/// Mirrors the fields of Go `db.DBFinding` that the report package reads,
/// plus the task name (`TaskDescription`) and the requested `company_name` /
/// `assets` context fields.
#[derive(Debug, Clone, Default)]
pub struct FindingReportInput {
    /// Finding id (Go `DBFinding.ID`).
    pub id: i64,
    /// Vulnerability class, e.g. `sqli` (Go `VulnClass`).
    pub vuln_class: String,
    /// Readable vulnerability name; empty falls back to `vuln_class`
    /// (Go `Name`).
    pub name: String,
    /// Severity: `critical` / `high` / `medium` / `low` (Go `Severity`).
    pub severity: String,
    /// Summary text (Go `Summary`).
    pub summary: String,
    /// Evidence / PoC text (Go `Evidence`, and `Evidence.PoC` for the task
    /// report).
    pub evidence: String,
    /// Triage status, e.g. `pending` (Go `Status`).
    pub status: String,
    /// Verification status: `verified` / `unverified` / ... (Go
    /// `VerificationStatus`).
    pub verification_status: String,
    /// Why the verification status was reached (Go `VerificationNote`).
    pub verification_note: String,
    /// Why the conclusion holds (Go `ConfidenceRationale`).
    pub confidence_rationale: String,
    /// What was tested and did not work (Go `Counterevidence`).
    pub counterevidence: String,
    /// Detailed Markdown report (Go `Report`).
    pub report: String,
    /// Proof-of-exploitation level 0-4 (Go `PoELevel`).
    pub poe_level: i32,
    /// Owning task description (Go `TaskDescription`).
    pub task_name: String,
    /// Owning company name (port extension; not emitted by the Go renderers).
    pub company_name: String,
    /// Asset identifiers for this finding (port extension; not emitted by the
    /// Go renderers).
    pub assets: Vec<String>,
    /// When the finding was created (Go `CreatedAt`).
    pub created_at: DateTime<Utc>,
    /// Current evidence version (Go `EvidenceVersion`).
    pub evidence_version: i64,
    /// Evidence version the detailed report was written against (Go
    /// `ReportEvidenceVersion`).
    pub report_evidence_version: i64,
    /// Traffic-evidence bindings (Go `TrafficBindings`).
    pub traffic_bindings: Vec<TrafficBinding>,
}

/// Report-level options (mirrors Go `report.Input` minus the findings slice,
/// plus the requested render toggles).
#[derive(Debug, Clone)]
pub struct ReportOptions {
    /// Report title. Empty selects the Go default per renderer.
    pub title: String,
    /// Task goal (Go `Input.Goal`).
    pub goal: String,
    /// Generation timestamp (Go `Input.GeneratedAt`).
    pub generated_at: DateTime<Utc>,
    /// Asset type -> count, rendered sorted by key (Go `Input.AssetCounts`).
    pub asset_counts: BTreeMap<String, usize>,
    /// Coverage ledger; `None` when coverage is off (Go `Input.Coverage`).
    pub coverage: Option<CoverageLedger>,
    /// Emit evidence / PoC fenced blocks (default `true`; Go always emits).
    pub include_evidence: bool,
    /// Emit the `Assets:` summary line in the task report (default `true`; Go
    /// always emits).
    pub include_assets: bool,
    /// Emit the coverage-gap section in the task report (default `true`; Go
    /// always emits when a ledger is present).
    pub include_coverage: bool,
}

impl Default for ReportOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            goal: String::new(),
            generated_at: Utc::now(),
            asset_counts: BTreeMap::new(),
            coverage: None,
            include_evidence: true,
            include_assets: true,
            include_coverage: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (ported from report.go)
// ---------------------------------------------------------------------------

/// Go `sevRank`: unknown non-empty severities map to `0` (the Go zero value),
/// matching map-lookup semantics.
fn sev_rank(severity: &str) -> i32 {
    match severity {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        "" => 4,
        _ => 0,
    }
}

/// Go `nz`: return `default` when `s` is empty after trimming, else `s`.
fn nz<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.trim().is_empty() {
        default
    } else {
        s
    }
}

/// Go `findingTitle`: `name` -> `vulnclass` -> `"Uncategorized"`.
fn finding_title(f: &FindingReportInput) -> &str {
    nz(&f.name, nz(&f.vuln_class, "Uncategorized"))
}

/// Go `2006-01-02 15:04:05`.
fn fmt_time(t: &DateTime<Utc>) -> String {
    t.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Sorts findings by severity ascending, then `created_at` descending, stable
/// (Go `sortFindingsForExport`).
fn sort_for_export(items: &mut [&FindingReportInput]) {
    items.sort_by(|a, b| {
        let (ra, rb) = (sev_rank(&a.severity), sev_rank(&b.severity));
        if ra != rb {
            ra.cmp(&rb)
        } else {
            b.created_at.cmp(&a.created_at)
        }
    });
}

/// Stable sort by severity ascending only (Go `report.Markdown`).
fn sort_by_severity(items: &mut [&FindingReportInput]) {
    items.sort_by_key(|f| sev_rank(&f.severity));
}

// ---------------------------------------------------------------------------
// Markdown: findings summary report (Go `report.FindingsMarkdown`)
// ---------------------------------------------------------------------------

/// Renders a batch of findings as a summary Markdown report.
///
/// Faithful port of Go `report.FindingsMarkdown`: an H1 title, a
/// `Generated at` / `Total findings` metadata header, a `## Summary` severity
/// count table, and `## Vulnerability Details` entries with class/status/task/
/// found-at metadata, summary, an `**Evidence:**` fenced block, the detailed
/// report, and related traffic evidence. Findings are sorted by severity then
/// newest first. When `opts.title` is non-empty it replaces the default
/// `Vulnerability Findings Summary Report` H1; `opts.include_evidence = false`
/// suppresses the evidence blocks.
pub fn render_markdown(inputs: &[FindingReportInput], opts: &ReportOptions) -> String {
    let mut items: Vec<&FindingReportInput> = inputs.iter().collect();
    sort_for_export(&mut items);

    let title = nz(&opts.title, "Vulnerability Findings Summary Report");

    let mut b = String::new();
    b.push_str(&format!("# {title}\n\n"));
    b.push_str(&format!(
        "- **Generated at**: {}\n",
        fmt_time(&opts.generated_at)
    ));
    b.push_str(&format!("- **Total findings**: {}\n\n", items.len()));

    // Summary: counts per severity (Go reads only the four known keys, so an
    // unknown severity is listed but not counted in the table).
    let mut counts = [0usize; 4];
    for it in &items {
        match it.severity.as_str() {
            "critical" => counts[0] += 1,
            "high" => counts[1] += 1,
            "medium" => counts[2] += 1,
            "low" => counts[3] += 1,
            _ => {}
        }
    }
    b.push_str("## Summary\n\n");
    b.push_str("| Severity | Count |\n| --- | --- |\n");
    b.push_str(&format!("| Critical | {} |\n", counts[0]));
    b.push_str(&format!("| High | {} |\n", counts[1]));
    b.push_str(&format!("| Medium | {} |\n", counts[2]));
    b.push_str(&format!("| Low | {} |\n", counts[3]));
    b.push('\n');

    if items.is_empty() {
        b.push_str("_No matching vulnerabilities._\n");
        return b;
    }

    b.push_str("## Vulnerability Details\n\n");
    for (i, f) in items.iter().enumerate() {
        b.push_str(&format!(
            "### {}. [{}] {}\n\n",
            i + 1,
            nz(&f.severity, "info").to_uppercase(),
            finding_title(f)
        ));
        if !f.vuln_class.is_empty() {
            b.push_str(&format!("- **Class**: {}\n", f.vuln_class));
        }
        b.push_str(&format!("- **Status**: {}\n", nz(&f.status, "pending")));
        let task = f.task_name.trim();
        if !task.is_empty() {
            b.push_str(&format!("- **Task**: {task}\n"));
        }
        b.push_str(&format!("- **Found at**: {}\n\n", fmt_time(&f.created_at)));
        let summary = f.summary.trim();
        if !summary.is_empty() {
            b.push_str(&format!("{summary}\n\n"));
        }
        if opts.include_evidence {
            let evidence = f.evidence.trim();
            if !evidence.is_empty() {
                b.push_str("**Evidence:**\n\n```\n");
                b.push_str(evidence);
                b.push_str("\n```\n\n");
            }
        }
        let report = f.report.trim();
        if !report.is_empty() {
            b.push_str("**Detailed report:**\n\n");
            b.push_str(report);
            b.push_str("\n\n");
        }
        b.push_str(&traffic_markdown(f, false));
        b.push_str("---\n\n");
    }
    b
}

// ---------------------------------------------------------------------------
// Markdown: single finding (Go `report.SingleFindingMarkdown`)
// ---------------------------------------------------------------------------

/// Renders one finding as a standalone Markdown document.
///
/// Faithful port of Go `report.SingleFindingMarkdown`: a `[SEVERITY] title` H1,
/// a metadata bullet list, then `## Summary`, `## Evidence` and
/// `## Detailed Report` sections when present, followed by related traffic
/// evidence with attachment links.
pub fn render_single_finding_markdown(f: &FindingReportInput, opts: &ReportOptions) -> String {
    let mut b = String::new();
    b.push_str(&format!(
        "# [{}] {}\n\n",
        nz(&f.severity, "info").to_uppercase(),
        finding_title(f)
    ));
    if !f.vuln_class.is_empty() {
        b.push_str(&format!("- **Class**: {}\n", f.vuln_class));
    }
    b.push_str(&format!("- **Severity**: {}\n", nz(&f.severity, "info")));
    b.push_str(&format!("- **Status**: {}\n", nz(&f.status, "pending")));
    let task = f.task_name.trim();
    if !task.is_empty() {
        b.push_str(&format!("- **Task**: {task}\n"));
    }
    b.push_str(&format!("- **Found at**: {}\n", fmt_time(&f.created_at)));
    b.push_str(&format!(
        "- **Generated at**: {}\n\n",
        fmt_time(&opts.generated_at)
    ));
    let summary = f.summary.trim();
    if !summary.is_empty() {
        b.push_str(&format!("## Summary\n\n{summary}\n\n"));
    }
    if opts.include_evidence {
        let evidence = f.evidence.trim();
        if !evidence.is_empty() {
            b.push_str(&format!("## Evidence\n\n```\n{evidence}\n```\n\n"));
        }
    }
    let report = f.report.trim();
    if !report.is_empty() {
        b.push_str("## Detailed Report\n\n");
        b.push_str(report);
        b.push('\n');
    }
    b.push_str(&traffic_markdown(f, true));
    b
}

// ---------------------------------------------------------------------------
// Markdown: task report (Go `report.Markdown`)
// ---------------------------------------------------------------------------

/// Renders the per-task report.
///
/// Faithful port of Go `report.Markdown` (`report.go`): a
/// `Penetration Test Report — <title>` H1 with goal and generated-at metadata,
/// a `## Summary` with the confirmed-finding count and asset counts, a
/// `## Findings` list sorted by severity with `**PoC / Evidence:**` fenced
/// blocks, and a `## Coverage gaps` section when `opts.coverage` is present.
/// `opts.include_assets = false` omits the asset-count line and
/// `opts.include_coverage = false` omits the coverage section (extensions;
/// defaults reproduce Go).
pub fn render_task_markdown(inputs: &[FindingReportInput], opts: &ReportOptions) -> String {
    let title = nz(&opts.title, "Unnamed task");
    let goal = nz(&opts.goal, "(unspecified)");

    let mut b = String::new();
    b.push_str(&format!("# Penetration Test Report — {title}\n\n"));
    b.push_str(&format!("- **Task goal**: {goal}\n"));
    b.push_str(&format!(
        "- **Generated at**: {}\n\n",
        fmt_time(&opts.generated_at)
    ));

    // Summary.
    b.push_str("## Summary\n\n");
    b.push_str(&format!("- Confirmed findings: **{}**\n", inputs.len()));
    if opts.include_assets {
        b.push_str("- Assets: ");
        for (i, (ty, count)) in opts.asset_counts.iter().enumerate() {
            if i > 0 {
                b.push_str(", ");
            }
            b.push_str(&format!("{ty} {count}"));
        }
        b.push('\n');
    }
    b.push('\n');

    // Findings.
    b.push_str("## Findings\n\n");
    if inputs.is_empty() {
        b.push_str("_No vulnerabilities confirmed this time._\n\n");
    } else {
        let mut fs: Vec<&FindingReportInput> = inputs.iter().collect();
        sort_by_severity(&mut fs);
        for (i, f) in fs.iter().enumerate() {
            b.push_str(&format!(
                "### {}. [{}] {}\n\n",
                i + 1,
                nz(&f.severity, "info").to_uppercase(),
                nz(&f.name, nz(&f.vuln_class, "Uncategorized"))
            ));
            b.push_str(&format!("{}\n\n", nz(&f.summary, "")));
            if opts.include_evidence && !f.evidence.is_empty() {
                b.push_str("**PoC / Evidence:**\n\n```\n");
                b.push_str(&f.evidence);
                b.push_str("\n```\n\n");
            }
        }
    }

    // Coverage gaps.
    if opts.include_coverage {
        if let Some(c) = &opts.coverage {
            b.push_str("## Coverage gaps\n\n");
            if let Some(comp) = &c.completeness {
                b.push_str(&format!(
                    "- **Completeness**: {}. {}\n",
                    comp.state, comp.caveat
                ));
            }
            b.push_str(&format!(
                "- Ledger entries: {}, open gaps: {}\n",
                c.entries.len(),
                c.gaps.len()
            ));
            for (i, gap) in c.gaps.iter().enumerate() {
                if i == REPORT_MAX_GAPS {
                    b.push_str(&format!(
                        "- and {} more (see the coverage API)\n",
                        c.gaps.len() - REPORT_MAX_GAPS
                    ));
                    break;
                }
                b.push_str(&format!("- {gap}\n"));
            }
            b.push('\n');
        }
    }

    b
}

// ---------------------------------------------------------------------------
// Markdown: related traffic evidence (Go `findingTrafficMarkdown`)
// ---------------------------------------------------------------------------

/// Port of Go `findingTrafficMarkdown`. `attachments` adds per-binding
/// request/response message links (used by the single-finding renderer).
fn traffic_markdown(f: &FindingReportInput, attachments: bool) -> String {
    let stale = !f.report.is_empty() && f.evidence_version != f.report_evidence_version;
    if f.traffic_bindings.is_empty() && !stale {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("\n## Related Traffic Evidence\n\n");
    out.push_str(&format!(
        "Evidence version: {}; bindings: {}.\n\n",
        f.evidence_version,
        f.traffic_bindings.len()
    ));
    if stale {
        out.push_str("The evidence has changed; the detailed report needs updating.\n\n");
    }
    for (i, binding) in f.traffic_bindings.iter().enumerate() {
        out.push_str(&format!(
            "{}. **Evidence #{} · {}** — `{} {}`, status {}\n",
            i + 1,
            binding.id,
            binding.role,
            binding.method,
            binding.url.replace('`', "%60"),
            binding.status
        ));
        if !binding.note.is_empty() {
            out.push_str("   ");
            out.push_str(&binding.note.replace('\n', "\n   "));
            out.push('\n');
        }
        if attachments {
            out.push_str(&format!(
                "   [Request message](evidence/{}/{}/request.http) · [Response message](evidence/{}/{}/response.http)\n",
                f.id, binding.id, f.id, binding.id
            ));
        }
    }
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// CSV (Go `report.FindingsCSV`)
// ---------------------------------------------------------------------------

/// Renders a batch of findings as CSV (header row + one row per finding).
///
/// Faithful port of Go `report.FindingsCSV`: a UTF-8 BOM prefix, the header
/// `ID,Name,Class,Severity,Status,Task,Found At,Summary,Traffic Evidence
/// Count,Traffic Evidence IDs`, findings sorted by severity then newest first,
/// and Go `encoding/csv` quoting. Long report/evidence text is intentionally
/// omitted (summary-level only), matching Go.
pub fn render_csv(inputs: &[FindingReportInput]) -> String {
    let mut buf: Vec<u8> = Vec::new();
    if render_csv_to(inputs, &mut buf).is_err() {
        return String::new();
    }
    String::from_utf8(buf).unwrap_or_default()
}

/// Writes the CSV rendering of `inputs` to `w`.
///
/// Fallible variant of [`render_csv`] for callers that stream to an
/// [`std::io::Write`] sink. The in-memory encoding is always valid UTF-8.
pub fn render_csv_to<W: std::io::Write>(
    inputs: &[FindingReportInput],
    w: &mut W,
) -> Result<(), ReportError> {
    let csv = build_csv(inputs);
    w.write_all(csv.as_bytes())
        .map_err(|e| ReportError::Render(e.to_string()))
}

fn build_csv(inputs: &[FindingReportInput]) -> String {
    let mut items: Vec<&FindingReportInput> = inputs.iter().collect();
    sort_for_export(&mut items);

    let mut out = String::new();
    out.push('\u{feff}'); // UTF-8 BOM (Go `"\xEF\xBB\xBF"`)
    push_csv_record(
        &mut out,
        &[
            "ID".to_string(),
            "Name".to_string(),
            "Class".to_string(),
            "Severity".to_string(),
            "Status".to_string(),
            "Task".to_string(),
            "Found At".to_string(),
            "Summary".to_string(),
            "Traffic Evidence Count".to_string(),
            "Traffic Evidence IDs".to_string(),
        ],
    );
    for f in items {
        let ids: Vec<String> = f
            .traffic_bindings
            .iter()
            .map(|b| b.id.to_string())
            .collect();
        push_csv_record(
            &mut out,
            &[
                f.id.to_string(),
                finding_title(f).to_string(),
                f.vuln_class.clone(),
                nz(&f.severity, "info").to_string(),
                nz(&f.status, "pending").to_string(),
                f.task_name.clone(),
                fmt_time(&f.created_at),
                f.summary.trim().to_string(),
                f.traffic_bindings.len().to_string(),
                ids.join(","),
            ],
        );
    }
    out
}

fn push_csv_record(out: &mut String, fields: &[String]) {
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_csv_field(out, field);
    }
    out.push('\n');
}

/// Appends one CSV field, quoting per Go `encoding/csv` rules.
fn push_csv_field(out: &mut String, field: &str) {
    let needs_quotes = !field.is_empty()
        && (field == "\\."
            || field.contains(',')
            || field.contains('"')
            || field.contains('\r')
            || field.contains('\n')
            || field.chars().next().is_some_and(char::is_whitespace));
    if needs_quotes {
        out.push('"');
        for c in field.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

// ---------------------------------------------------------------------------
// Filename (Go `report.FindingFilename`)
// ---------------------------------------------------------------------------

/// Generates a safe `.md` filename for "one file per vulnerability", e.g.
/// `critical_SQL_injection_#123.md`.
///
/// Faithful port of Go `report.FindingFilename`: `severity_title_#id`, with
/// runs of path/control characters replaced by `_`, `._` trimmed from the
/// ends, a `finding_<id>` fallback, a final base-name pass and a 120-char cap
/// (char-boundary safe here). Go's regexp keeps `\p{Han}\p{L}\p{N}._-`; Rust's
/// `char::is_alphanumeric` is the equivalent letter/number class.
pub fn finding_filename(f: &FindingReportInput) -> String {
    let sev = nz(&f.severity, "info");
    let title = finding_title(f);
    let raw = format!("{sev}_{title}_#{}", f.id);
    let sanitized = sanitize_filename(&raw);
    let trimmed = trim_dot_underscore(&sanitized);
    let base = if trimmed.is_empty() {
        format!("finding_{}", f.id)
    } else {
        trimmed.to_string()
    };
    let base = path_base(&base);
    let capped: String = if base.chars().count() > 120 {
        base.chars().take(120).collect()
    } else {
        base.to_string()
    };
    format!("{capped}.md")
}

/// Replaces each run of characters outside `[letter|number|.|_-]` with `_`
/// (Go `unsafeFilenameChars.ReplaceAllString(name, "_")`).
fn sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    let mut in_unsafe = false;
    for c in name.chars() {
        let safe = c.is_alphanumeric() || c == '.' || c == '_' || c == '-';
        if safe {
            if in_unsafe {
                out.push('_');
                in_unsafe = false;
            }
            out.push(c);
        } else {
            in_unsafe = true;
        }
    }
    if in_unsafe {
        out.push('_');
    }
    out
}

/// Go `strings.Trim(name, "._")`.
fn trim_dot_underscore(s: &str) -> &str {
    s.trim_matches(|c| c == '.' || c == '_')
}

/// Go `path.Base`: strip trailing `/`, then take the substring after the last
/// `/` (returns `"/"` for the root).
fn path_base(name: &str) -> &str {
    let trimmed = name.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(secs, 0).expect("valid timestamp")
    }

    fn base_finding(severity: &str, name: &str) -> FindingReportInput {
        FindingReportInput {
            id: 1,
            vuln_class: "xss".to_string(),
            name: name.to_string(),
            severity: severity.to_string(),
            summary: "A summary.".to_string(),
            evidence: "GET / HTTP/1.1".to_string(),
            status: "confirmed".to_string(),
            task_name: "acme".to_string(),
            created_at: ts(1_700_000_000),
            ..Default::default()
        }
    }

    fn opts() -> ReportOptions {
        ReportOptions {
            title: String::new(),
            generated_at: ts(1_700_000_100),
            ..Default::default()
        }
    }

    #[test]
    fn markdown_has_headings_and_metadata() {
        let inputs = vec![base_finding("high", "SQL injection")];
        let md = render_markdown(&inputs, &opts());
        assert!(md.starts_with("# Vulnerability Findings Summary Report\n\n"));
        assert!(md.contains("- **Generated at**: 2023-11-14 22:15:00\n"));
        assert!(md.contains("- **Total findings**: 1\n"));
        assert!(md.contains("## Summary\n"));
        assert!(md.contains("| Severity | Count |\n| --- | --- |\n"));
        assert!(md.contains("| High | 1 |\n"));
        assert!(md.contains("## Vulnerability Details\n"));
        assert!(md.contains("### 1. [HIGH] SQL injection\n"));
        assert!(md.contains("- **Class**: xss\n"));
        assert!(md.contains("- **Status**: confirmed\n"));
        assert!(md.contains("- **Task**: acme\n"));
        assert!(md.contains("- **Found at**: 2023-11-14 22:13:20\n"));
        assert!(md.contains("A summary.\n"));
    }

    #[test]
    fn markdown_title_override() {
        let inputs = vec![base_finding("low", "x")];
        let mut o = opts();
        o.title = "Custom".to_string();
        let md = render_markdown(&inputs, &o);
        assert!(md.starts_with("# Custom\n\n"));
    }

    #[test]
    fn markdown_orders_by_severity_then_recency() {
        let mut critical = base_finding("critical", "crit");
        critical.id = 1;
        critical.created_at = ts(1_700_000_000);
        let mut low = base_finding("low", "low");
        low.id = 2;
        low.created_at = ts(1_700_000_900);
        let mut high = base_finding("high", "high");
        high.id = 3;
        let inputs = vec![low, high, critical];
        let md = render_markdown(&inputs, &opts());
        let crit = md.find("[CRITICAL]").expect("critical heading");
        let high = md.find("[HIGH]").expect("high heading");
        let low = md.find("[LOW]").expect("low heading");
        assert!(
            crit < high && high < low,
            "expected critical, high, low order"
        );
    }

    #[test]
    fn markdown_evidence_included() {
        let inputs = vec![base_finding("medium", "m")];
        let md = render_markdown(&inputs, &opts());
        assert!(md.contains("**Evidence:**\n\n```\nGET / HTTP/1.1\n```\n"));
    }

    #[test]
    fn markdown_evidence_excluded() {
        let inputs = vec![base_finding("medium", "m")];
        let mut o = opts();
        o.include_evidence = false;
        let md = render_markdown(&inputs, &o);
        assert!(!md.contains("**Evidence:**"));
        assert!(!md.contains("GET / HTTP/1.1"));
    }

    #[test]
    fn markdown_detailed_report_included() {
        let mut f = base_finding("low", "l");
        f.report = "# Detail\n\nbody".to_string();
        let md = render_markdown(&[f], &opts());
        assert!(md.contains("**Detailed report:**\n\n# Detail\n\nbody\n\n"));
    }

    #[test]
    fn markdown_empty_input() {
        let md = render_markdown(&[], &opts());
        assert!(md.contains("- **Total findings**: 0\n"));
        assert!(md.contains("| Low | 0 |\n"));
        assert!(md.contains("_No matching vulnerabilities._\n"));
        assert!(!md.contains("## Vulnerability Details"));
    }

    #[test]
    fn markdown_unknown_severity_sorts_as_critical() {
        // Go map zero value: unknown severity ranks 0 (before high).
        let mut info = base_finding("info", "info");
        info.id = 1;
        let mut high = base_finding("high", "high");
        high.id = 2;
        let md = render_markdown(&[high, info], &opts());
        let i = md.find("[INFO]").expect("info heading");
        let h = md.find("[HIGH]").expect("high heading");
        assert!(i < h, "unknown severity must sort as rank 0");
    }

    #[test]
    fn csv_has_bom_and_header() {
        let csv = render_csv(&[]);
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains(
            "ID,Name,Class,Severity,Status,Task,Found At,Summary,Traffic Evidence Count,Traffic Evidence IDs\n"
        ));
        // Empty input: only the header record.
        assert_eq!(csv.matches('\n').count(), 1);
    }

    #[test]
    fn csv_row_fields() {
        let mut f = base_finding("high", "SQL injection");
        f.id = 42;
        f.vuln_class = "sqli".to_string();
        f.task_name = "acme".to_string();
        f.summary = "  padded summary  ".to_string();
        f.traffic_bindings = vec![
            TrafficBinding {
                id: 7,
                ..Default::default()
            },
            TrafficBinding {
                id: 9,
                ..Default::default()
            },
        ];
        let csv = render_csv(&[f]);
        assert!(csv.contains(
            "42,SQL injection,sqli,high,confirmed,acme,2023-11-14 22:13:20,padded summary,2,\"7,9\"\n"
        ));
    }

    #[test]
    fn csv_escapes_commas_quotes_and_newlines() {
        let mut f = base_finding("low", "name,with,commas");
        f.summary = "line1\nline2 \"quoted\"".to_string();
        f.task_name = "task".to_string();
        let csv = render_csv(&[f]);
        // Name with commas is quoted; summary with quote/newline is quoted and
        // internal quotes are doubled.
        assert!(csv.contains("\"name,with,commas\""));
        assert!(csv.contains("\"line1\nline2 \"\"quoted\"\"\""));
    }

    #[test]
    fn csv_quotes_leading_whitespace_field() {
        // Go trims the summary (strings.TrimSpace) before writing, so a
        // leading-space summary is stored trimmed and not quoted.
        let mut f = base_finding("low", "x");
        f.summary = " leading".to_string();
        let csv = render_csv(&[f]);
        assert!(csv.contains(",leading,"));
        assert!(!csv.contains("\" leading\""));
    }

    #[test]
    fn csv_sorted_by_severity_then_recency() {
        let mut critical = base_finding("critical", "c");
        critical.id = 1;
        critical.created_at = ts(1_700_000_000);
        let mut low = base_finding("low", "l");
        low.id = 2;
        low.created_at = ts(1_700_000_900);
        let csv = render_csv(&[low, critical]);
        let c = csv.find(",critical,").expect("critical row");
        let l = csv.find(",low,").expect("low row");
        assert!(c < l);
    }

    #[test]
    fn task_markdown_structure() {
        let mut counts = BTreeMap::new();
        counts.insert("domain".to_string(), 2);
        counts.insert("ip".to_string(), 1);
        let o = ReportOptions {
            title: "Acme".to_string(),
            goal: "test it".to_string(),
            generated_at: ts(1_700_000_100),
            asset_counts: counts,
            ..Default::default()
        };
        let inputs = vec![base_finding("high", "XSS")];
        let md = render_task_markdown(&inputs, &o);
        assert!(md.starts_with("# Penetration Test Report — Acme\n\n"));
        assert!(md.contains("- **Task goal**: test it\n"));
        assert!(md.contains("- Confirmed findings: **1**\n"));
        // Asset counts are sorted by key: domain before ip.
        assert!(md.contains("- Assets: domain 2, ip 1\n"));
        assert!(md.contains("### 1. [HIGH] XSS\n"));
        assert!(md.contains("**PoC / Evidence:**\n\n```\nGET / HTTP/1.1\n```\n"));
    }

    #[test]
    fn task_markdown_empty_findings() {
        let md = render_task_markdown(&[], &opts());
        assert!(md.contains("_No vulnerabilities confirmed this time._\n"));
    }

    #[test]
    fn task_markdown_assets_toggle() {
        let mut counts = BTreeMap::new();
        counts.insert("ip".to_string(), 1);
        let o = ReportOptions {
            asset_counts: counts,
            include_assets: false,
            ..opts()
        };
        let md = render_task_markdown(&[], &o);
        assert!(!md.contains("- Assets:"));
    }

    #[test]
    fn task_markdown_coverage_section() {
        let ledger = CoverageLedger {
            entries: vec![CoverageEntry::default(), CoverageEntry::default()],
            gaps: vec![
                CoverageGap {
                    kind: "needs_follow_up".to_string(),
                    surface: "/a".to_string(),
                    risk_area: "xss".to_string(),
                },
                CoverageGap {
                    kind: "unrecorded_risk_class".to_string(),
                    surface: String::new(),
                    risk_area: "sqli".to_string(),
                },
            ],
            completeness: Some(CoverageCompleteness {
                state: "timeout".to_string(),
                caveat: "Incomplete scan".to_string(),
                ..Default::default()
            }),
        };
        let o = ReportOptions {
            coverage: Some(ledger),
            ..opts()
        };
        let md = render_task_markdown(&[], &o);
        assert!(md.contains("## Coverage gaps\n"));
        assert!(md.contains("- **Completeness**: timeout. Incomplete scan\n"));
        assert!(md.contains("- Ledger entries: 2, open gaps: 2\n"));
        assert!(md.contains("- needs_follow_up /a xss\n"));
        assert!(md.contains("- unrecorded_risk_class sqli\n"));
    }

    #[test]
    fn task_markdown_coverage_capped() {
        let gaps: Vec<CoverageGap> = (0..(REPORT_MAX_GAPS + 30))
            .map(|_| CoverageGap {
                kind: "needs_follow_up".to_string(),
                surface: "/p".to_string(),
                risk_area: "xss".to_string(),
            })
            .collect();
        let o = ReportOptions {
            coverage: Some(CoverageLedger {
                gaps,
                ..Default::default()
            }),
            ..opts()
        };
        let md = render_task_markdown(&[], &o);
        assert_eq!(
            md.matches("- needs_follow_up /p xss\n").count(),
            REPORT_MAX_GAPS
        );
        assert!(md.contains("- and 30 more (see the coverage API)\n"));
    }

    #[test]
    fn task_markdown_no_coverage_section_when_none() {
        let md = render_task_markdown(&[], &opts());
        assert!(!md.contains("## Coverage gaps"));
    }

    #[test]
    fn traffic_markdown_emitted_when_present() {
        let mut f = base_finding("low", "l");
        f.traffic_bindings = vec![TrafficBinding {
            id: 5,
            role: "request".to_string(),
            note: "line1\nline2".to_string(),
            method: "GET".to_string(),
            url: "https://x/`weird`".to_string(),
            status: 200,
        }];
        let md = render_markdown(&[f], &opts());
        assert!(md.contains("## Related Traffic Evidence\n"));
        assert!(md.contains("Evidence version: 0; bindings: 1.\n"));
        assert!(
            md.contains("1. **Evidence #5 · request** — `GET https://x/%60weird%60`, status 200\n")
        );
        assert!(md.contains("   line1\n   line2\n"));
        // No attachment links in the batch report.
        assert!(!md.contains("[Request message]"));
    }

    #[test]
    fn single_finding_markdown_sections() {
        let mut f = base_finding("critical", "RCE");
        f.report = "detailed".to_string();
        let md = render_single_finding_markdown(&f, &opts());
        assert!(md.starts_with("# [CRITICAL] RCE\n\n"));
        assert!(md.contains("- **Severity**: critical\n"));
        assert!(md.contains("## Summary\n\nA summary.\n"));
        assert!(md.contains("## Evidence\n\n```\nGET / HTTP/1.1\n```\n"));
        assert!(md.contains("## Detailed Report\n\ndetailed\n"));
    }

    #[test]
    fn single_finding_markdown_attachment_links() {
        let mut f = base_finding("low", "l");
        f.id = 3;
        f.traffic_bindings = vec![TrafficBinding {
            id: 8,
            ..Default::default()
        }];
        let md = render_single_finding_markdown(&f, &opts());
        assert!(md.contains(
            "[Request message](evidence/3/8/request.http) · [Response message](evidence/3/8/response.http)"
        ));
    }

    #[test]
    fn filename_basic() {
        let mut f = base_finding("critical", "SQL injection");
        f.id = 123;
        assert_eq!(finding_filename(&f), "critical_SQL_injection__123.md");
    }

    #[test]
    fn filename_sanitizes_unsafe_chars() {
        let mut f = base_finding("high", "a/b:c*d");
        f.id = 7;
        assert_eq!(finding_filename(&f), "high_a_b_c_d__7.md");
    }

    #[test]
    fn filename_fallback_when_all_unsafe() {
        let mut f = base_finding("", "");
        f.id = 9;
        // name empty → title falls back to vuln_class ("xss"); severity empty →
        // "info"; '#' is unsafe and becomes '_'.
        assert_eq!(finding_filename(&f), "info_xss__9.md");
    }

    #[test]
    fn filename_caps_length() {
        let mut f = base_finding("low", &"a".repeat(200));
        f.id = 1;
        let name = finding_filename(&f);
        // "low_" + 120 chars + ".md" (cap applied to the stem, then ".md").
        let stem = name.strip_suffix(".md").expect("md suffix");
        assert_eq!(stem.chars().count(), 120);
    }
}
