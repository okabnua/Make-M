//! Hierarchical run budgets — Rust port of ARTEX `agent/budget.go`.
//!
//! Budgets are hierarchical (task -> intent) and off by default: a dimension
//! with no configured budget is unlimited. Each dimension is metered
//! independently, and a worker is not started once the task's remaining budget
//! cannot cover the worker's estimated cost (the average of this task's
//! finished workers, else 50000 tokens and 10 turns) or once the remaining
//! budget reaches the wrap-up reserve.
//!
//! Behavior-preserving notes:
//! - Environment values are read the same way as Go: `strconv.ParseInt` on the
//!   trimmed value, and unset, invalid, zero or negative means unlimited.
//! - Remaining values never go below zero and stay unlimited when uncapped.
//! - The wrap-up reserve is `max(5% of the task budget, 20000 tokens)` and
//!   `max(5% of the task budget, 2 turns)`; it is zero for an unlimited
//!   dimension (matching Go's `WrapupMargin`).
//! - `can_start_worker` reproduces `BudgetLedger.State().Dispatch`: no dispatch
//!   once the reserve is reached, and not when the remaining task budget is
//!   below one worker's estimate (clamped by the intent budget, never above it).
//! - Arithmetic saturates instead of overflowing.

#![forbid(unsafe_code)]

/// Wrap-up reserve as a percentage of the task budget (Go `BudgetMarginPercent`).
pub const BUDGET_MARGIN_PERCENT: u64 = 5;
/// Token floor of the wrap-up reserve (Go `BudgetMarginMinTokens`).
pub const BUDGET_MARGIN_MIN_TOKENS: u64 = 20_000;
/// Turn floor of the wrap-up reserve (Go `BudgetMarginMinTurns`).
pub const BUDGET_MARGIN_MIN_TURNS: u64 = 2;
/// Default token estimate for one worker run before any worker finished
/// (Go `DefaultWorkerEstimateTokens`).
pub const DEFAULT_WORKER_ESTIMATE_TOKENS: u64 = 50_000;
/// Default turn estimate for one worker run before any worker finished
/// (Go `DefaultWorkerEstimateTurns`).
pub const DEFAULT_WORKER_ESTIMATE_TURNS: u32 = 10;

/// One finished worker run's measured cost, used to estimate the next worker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkerCost {
    /// Tokens consumed (input + output).
    pub tokens: u64,
    /// Metered turns (one per LLM call).
    pub turns: u32,
}

/// Task- and intent-level token/turn budgets plus the running counters.
///
/// `None` means unlimited for that dimension. Counters saturate on overflow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Budget {
    task_token_budget: Option<u64>,
    task_turn_budget: Option<u64>,
    intent_token_budget: Option<u64>,
    intent_turn_budget: Option<u64>,

    task_tokens_used: u64,
    task_turns_used: u64,
    intent_tokens_used: u64,
    intent_turns_used: u64,
}

impl Budget {
    /// Builds a budget from explicit caps; `None` means unlimited.
    #[must_use]
    pub fn new(
        task_tokens: Option<u64>,
        task_turns: Option<u64>,
        intent_tokens: Option<u64>,
        intent_turns: Option<u64>,
    ) -> Self {
        Self {
            task_token_budget: task_tokens,
            task_turn_budget: task_turns,
            intent_token_budget: intent_tokens,
            intent_turn_budget: intent_turns,
            ..Self::default()
        }
    }

    /// Builds a budget with every dimension unlimited.
    #[must_use]
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Reads `ARTEX_TASK_BUDGET_{TOKENS,TURNS}` and
    /// `ARTEX_INTENT_BUDGET_{TOKENS,TURNS}` (Go `BudgetsFromEnv`).
    ///
    /// Unset, invalid, zero or negative values are unlimited.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            task_token_budget: env_cap("ARTEX_TASK_BUDGET_TOKENS"),
            task_turn_budget: env_cap("ARTEX_TASK_BUDGET_TURNS"),
            intent_token_budget: env_cap("ARTEX_INTENT_BUDGET_TOKENS"),
            intent_turn_budget: env_cap("ARTEX_INTENT_BUDGET_TURNS"),
            ..Self::default()
        }
    }

    /// Resets the current intent's counters (Go starts a fresh `IntentMeter`
    /// for every worker run). Task counters are untouched.
    pub fn start_intent(&mut self) {
        self.intent_tokens_used = 0;
        self.intent_turns_used = 0;
    }

    /// Task tokens left, or `None` when unlimited.
    #[must_use]
    pub fn remaining_task_tokens(&self) -> Option<u64> {
        self.task_token_budget
            .map(|cap| cap.saturating_sub(self.task_tokens_used))
    }

    /// Task turns left, or `None` when unlimited.
    #[must_use]
    pub fn remaining_task_turns(&self) -> Option<u64> {
        self.task_turn_budget
            .map(|cap| cap.saturating_sub(self.task_turns_used))
    }

    /// Intent tokens left, or `None` when unlimited.
    #[must_use]
    pub fn remaining_intent_tokens(&self) -> Option<u64> {
        self.intent_token_budget
            .map(|cap| cap.saturating_sub(self.intent_tokens_used))
    }

    /// Intent turns left, or `None` when unlimited.
    #[must_use]
    pub fn remaining_intent_turns(&self) -> Option<u64> {
        self.intent_turn_budget
            .map(|cap| cap.saturating_sub(self.intent_turns_used))
    }

    /// Wrap-up token reserve: `max(5% of the task token budget, 20000)`, or 0
    /// when the task token budget is unlimited (Go `WrapupMargin`).
    #[must_use]
    pub fn wrapup_reserve_tokens(&self) -> u64 {
        reserve(self.task_token_budget, BUDGET_MARGIN_MIN_TOKENS)
    }

    /// Wrap-up turn reserve: `max(5% of the task turn budget, 2)`, or 0 when
    /// the task turn budget is unlimited (Go `WrapupMargin`).
    #[must_use]
    pub fn wrapup_reserve_turns(&self) -> u64 {
        reserve(self.task_turn_budget, BUDGET_MARGIN_MIN_TURNS)
    }

    /// True when either remaining task dimension is at or under its wrap-up
    /// reserve (Go `BudgetState.AtMargin`): dispatch must stop and the task
    /// settles through one forced wrap-up round.
    #[must_use]
    pub fn is_forced_wrapup(&self) -> bool {
        let tokens_at_margin = self
            .remaining_task_tokens()
            .is_some_and(|rem| rem <= self.wrapup_reserve_tokens());
        let turns_at_margin = self
            .remaining_task_turns()
            .is_some_and(|rem| rem <= self.wrapup_reserve_turns());
        tokens_at_margin || turns_at_margin
    }

    /// Whether a new worker may start given its estimated cost (Go
    /// `BudgetLedger.State().Dispatch`).
    ///
    /// False once the wrap-up reserve is reached. Otherwise the remaining task
    /// budget must cover the estimate, which is first clamped by the intent
    /// budget so a worker is never required to exceed the intent cap.
    #[must_use]
    pub fn can_start_worker(&self, estimated_tokens: u64, estimated_turns: u32) -> bool {
        if self.is_forced_wrapup() {
            return false;
        }
        let est_tokens = match self.intent_token_budget {
            Some(cap) => estimated_tokens.min(cap),
            None => estimated_tokens,
        };
        let est_turns = match self.intent_turn_budget {
            Some(cap) => u32::try_from(cap.min(u64::from(estimated_turns))).unwrap_or(u32::MAX),
            None => estimated_turns,
        };
        let tokens_ok = self
            .remaining_task_tokens()
            .is_none_or(|rem| rem >= est_tokens);
        let turns_ok = self
            .remaining_task_turns()
            .is_none_or(|rem| rem >= u64::from(est_turns));
        tokens_ok && turns_ok
    }

    /// Task tokens consumed so far.
    #[must_use]
    pub fn task_tokens_used(&self) -> u64 {
        self.task_tokens_used
    }

    /// Task turns consumed so far.
    #[must_use]
    pub fn task_turns_used(&self) -> u64 {
        self.task_turns_used
    }

    /// Current intent's tokens consumed so far.
    #[must_use]
    pub fn intent_tokens_used(&self) -> u64 {
        self.intent_tokens_used
    }

    /// Current intent's turns consumed so far.
    #[must_use]
    pub fn intent_turns_used(&self) -> u64 {
        self.intent_turns_used
    }

    /// Accumulates task tokens (the planner's own rounds), saturating.
    pub fn record_task_tokens(&mut self, n: u64) {
        self.task_tokens_used = self.task_tokens_used.saturating_add(n);
    }

    /// Accumulates task turns (the planner's own rounds), saturating.
    pub fn record_task_turns(&mut self, n: u64) {
        self.task_turns_used = self.task_turns_used.saturating_add(n);
    }

    /// Accumulates the current intent's tokens, saturating.
    ///
    /// Mirrors one worker call: the Go `IntentMeter.Record` adds the same
    /// usage to both the intent and the task, so callers record both.
    pub fn record_intent_tokens(&mut self, n: u64) {
        self.intent_tokens_used = self.intent_tokens_used.saturating_add(n);
    }

    /// Accumulates the current intent's turns, saturating.
    pub fn record_intent_turns(&mut self, n: u64) {
        self.intent_turns_used = self.intent_turns_used.saturating_add(n);
    }

    /// Estimates one worker's cost from this task's finished workers (Go
    /// `BudgetLedger.State` estimate): the truncated average, or the defaults
    /// when no worker has finished.
    #[must_use]
    pub fn estimated_worker_cost(finished_workers: &[WorkerCost]) -> (u64, u32) {
        estimated_worker_cost(finished_workers)
    }
}

/// Pure worker-cost estimator (Go `BudgetLedger.State`): the truncated average
/// of `finished_workers`, or 50000 tokens and 10 turns when empty.
#[must_use]
pub fn estimated_worker_cost(finished_workers: &[WorkerCost]) -> (u64, u32) {
    if finished_workers.is_empty() {
        return (
            DEFAULT_WORKER_ESTIMATE_TOKENS,
            DEFAULT_WORKER_ESTIMATE_TURNS,
        );
    }
    let count = finished_workers.len() as u64;
    let mut total_tokens = 0_u64;
    let mut total_turns = 0_u64;
    for worker in finished_workers {
        total_tokens = total_tokens.saturating_add(worker.tokens);
        total_turns = total_turns.saturating_add(u64::from(worker.turns));
    }
    let avg_tokens = total_tokens / count;
    let avg_turns = total_turns / count;
    let turns = u32::try_from(avg_turns).unwrap_or(u32::MAX);
    (avg_tokens, turns)
}

/// Parses one env var like Go: trimmed base-10 int64; unset/invalid/<=0 is
/// unlimited.
fn env_cap(key: &str) -> Option<u64> {
    let raw = std::env::var(key).unwrap_or_default();
    let parsed: i64 = raw.trim().parse().unwrap_or(0);
    if parsed <= 0 {
        None
    } else {
        Some(parsed as u64)
    }
}

/// Go `WrapupMargin`'s per-dimension helper: 0 when unlimited, else
/// `max(capacity * 5 / 100, floor)`.
fn reserve(capacity: Option<u64>, floor: u64) -> u64 {
    match capacity {
        None => 0,
        Some(cap) => (cap.saturating_mul(BUDGET_MARGIN_PERCENT) / 100).max(floor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clear_budget_env() {
        for key in [
            "ARTEX_TASK_BUDGET_TOKENS",
            "ARTEX_TASK_BUDGET_TURNS",
            "ARTEX_INTENT_BUDGET_TOKENS",
            "ARTEX_INTENT_BUDGET_TURNS",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn unlimited_when_unset() {
        let _guard = env_guard();
        clear_budget_env();
        let budget = Budget::from_env();
        assert_eq!(budget.remaining_task_tokens(), None);
        assert_eq!(budget.remaining_task_turns(), None);
        assert_eq!(budget.remaining_intent_tokens(), None);
        assert_eq!(budget.remaining_intent_turns(), None);
        assert_eq!(budget.wrapup_reserve_tokens(), 0);
        assert_eq!(budget.wrapup_reserve_turns(), 0);
        assert!(!budget.is_forced_wrapup());
        assert!(budget.can_start_worker(DEFAULT_WORKER_ESTIMATE_TOKENS, 10));
    }

    #[test]
    fn from_env_reads_caps() {
        let _guard = env_guard();
        clear_budget_env();
        std::env::set_var("ARTEX_TASK_BUDGET_TOKENS", " 12345 ");
        std::env::set_var("ARTEX_TASK_BUDGET_TURNS", "7");
        std::env::set_var("ARTEX_INTENT_BUDGET_TOKENS", "999");
        std::env::set_var("ARTEX_INTENT_BUDGET_TURNS", "3");
        let budget = Budget::from_env();
        assert_eq!(budget.remaining_task_tokens(), Some(12_345));
        assert_eq!(budget.remaining_task_turns(), Some(7));
        assert_eq!(budget.remaining_intent_tokens(), Some(999));
        assert_eq!(budget.remaining_intent_turns(), Some(3));
        clear_budget_env();
    }

    #[test]
    fn from_env_invalid_is_unlimited() {
        let _guard = env_guard();
        clear_budget_env();
        std::env::set_var("ARTEX_TASK_BUDGET_TOKENS", "not-a-number");
        std::env::set_var("ARTEX_TASK_BUDGET_TURNS", "0");
        std::env::set_var("ARTEX_INTENT_BUDGET_TOKENS", "-5");
        std::env::set_var("ARTEX_INTENT_BUDGET_TURNS", "3.5");
        let budget = Budget::from_env();
        assert_eq!(budget.remaining_task_tokens(), None);
        assert_eq!(budget.remaining_task_turns(), None);
        assert_eq!(budget.remaining_intent_tokens(), None);
        assert_eq!(budget.remaining_intent_turns(), None);
        clear_budget_env();
    }

    #[test]
    fn task_token_cap_enforced() {
        let mut budget = Budget::new(Some(1_000), None, None, None);
        budget.record_task_tokens(400);
        assert_eq!(budget.remaining_task_tokens(), Some(600));
        assert_eq!(budget.task_tokens_used(), 400);
        budget.record_task_tokens(600);
        assert_eq!(budget.remaining_task_tokens(), Some(0));
        budget.record_task_tokens(50);
        assert_eq!(budget.remaining_task_tokens(), Some(0));
        assert_eq!(budget.task_tokens_used(), 1_050);
    }

    #[test]
    fn task_turn_cap_enforced() {
        let mut budget = Budget::new(None, Some(10), None, None);
        budget.record_task_turns(4);
        assert_eq!(budget.remaining_task_turns(), Some(6));
        budget.record_task_turns(6);
        assert_eq!(budget.remaining_task_turns(), Some(0));
    }

    #[test]
    fn wrapup_reserve_values() {
        let unlimited = Budget::new(None, None, None, None);
        assert_eq!(unlimited.wrapup_reserve_tokens(), 0);
        assert_eq!(unlimited.wrapup_reserve_turns(), 0);

        let large = Budget::new(Some(1_000_000), Some(1_000), None, None);
        assert_eq!(large.wrapup_reserve_tokens(), 50_000);
        assert_eq!(large.wrapup_reserve_turns(), 50);

        let small = Budget::new(Some(10), Some(1), None, None);
        assert_eq!(small.wrapup_reserve_tokens(), 20_000);
        assert_eq!(small.wrapup_reserve_turns(), 2);
    }

    #[test]
    fn wrapup_reserve_triggers() {
        let mut budget = Budget::new(Some(100_000), Some(1_000), None, None);
        assert!(!budget.is_forced_wrapup());
        budget.record_task_tokens(79_999);
        assert_eq!(budget.remaining_task_tokens(), Some(20_001));
        assert!(!budget.is_forced_wrapup());
        budget.record_task_tokens(1);
        assert_eq!(budget.remaining_task_tokens(), Some(20_000));
        assert!(budget.is_forced_wrapup());
        assert!(!budget.can_start_worker(1, 1));

        let mut turns = Budget::new(None, Some(10), None, None);
        assert!(!turns.is_forced_wrapup());
        turns.record_task_turns(8);
        assert_eq!(turns.remaining_task_turns(), Some(2));
        assert!(turns.is_forced_wrapup());
    }

    #[test]
    fn can_start_worker_uses_default_estimate() {
        let mut budget = Budget::new(Some(60_000), Some(100), None, None);
        assert!(budget.can_start_worker(
            DEFAULT_WORKER_ESTIMATE_TOKENS,
            DEFAULT_WORKER_ESTIMATE_TURNS
        ));
        budget.record_task_tokens(20_000);
        assert_eq!(budget.remaining_task_tokens(), Some(40_000));
        assert!(!budget.can_start_worker(
            DEFAULT_WORKER_ESTIMATE_TOKENS,
            DEFAULT_WORKER_ESTIMATE_TURNS
        ));

        let mut turns = Budget::new(None, Some(20), None, None);
        assert!(turns.can_start_worker(1, DEFAULT_WORKER_ESTIMATE_TURNS));
        turns.record_task_turns(11);
        assert!(!turns.can_start_worker(1, DEFAULT_WORKER_ESTIMATE_TURNS));
    }

    #[test]
    fn can_start_worker_uses_finished_average() {
        let finished = [
            WorkerCost {
                tokens: 10_000,
                turns: 4,
            },
            WorkerCost {
                tokens: 30_000,
                turns: 6,
            },
        ];
        let (tokens, turns) = estimated_worker_cost(&finished);
        assert_eq!((tokens, turns), (20_000, 5));

        let mut budget = Budget::new(Some(25_000), Some(10), None, None);
        assert!(budget.can_start_worker(tokens, turns));
        budget.record_task_tokens(6_000);
        assert!(!budget.can_start_worker(tokens, turns));
    }

    #[test]
    fn can_start_worker_clamped_by_intent_budget() {
        let budget = Budget::new(Some(100_000), Some(100), Some(10_000), Some(10));
        assert!(budget.can_start_worker(DEFAULT_WORKER_ESTIMATE_TOKENS, 10));
    }

    #[test]
    fn intent_level_caps() {
        let mut budget = Budget::new(None, None, Some(1_000), Some(10));
        budget.record_intent_tokens(400);
        budget.record_intent_turns(3);
        assert_eq!(budget.remaining_intent_tokens(), Some(600));
        assert_eq!(budget.remaining_intent_turns(), Some(7));
        assert_eq!(budget.intent_tokens_used(), 400);
        assert_eq!(budget.intent_turns_used(), 3);

        budget.record_intent_tokens(600);
        budget.record_intent_turns(7);
        assert_eq!(budget.remaining_intent_tokens(), Some(0));
        assert_eq!(budget.remaining_intent_turns(), Some(0));

        budget.start_intent();
        assert_eq!(budget.intent_tokens_used(), 0);
        assert_eq!(budget.intent_turns_used(), 0);
        assert_eq!(budget.remaining_intent_tokens(), Some(1_000));
        assert_eq!(budget.remaining_intent_turns(), Some(10));
    }

    #[test]
    fn saturating_arithmetic() {
        let mut budget = Budget::new(
            Some(u64::MAX),
            Some(u64::MAX),
            Some(u64::MAX),
            Some(u64::MAX),
        );
        budget.record_task_tokens(u64::MAX);
        budget.record_task_tokens(1);
        budget.record_task_turns(u64::MAX);
        budget.record_task_turns(1);
        budget.record_intent_tokens(u64::MAX);
        budget.record_intent_tokens(1);
        budget.record_intent_turns(u64::MAX);
        budget.record_intent_turns(1);
        assert_eq!(budget.task_tokens_used(), u64::MAX);
        assert_eq!(budget.task_turns_used(), u64::MAX);
        assert_eq!(budget.intent_tokens_used(), u64::MAX);
        assert_eq!(budget.intent_turns_used(), u64::MAX);
        assert_eq!(budget.remaining_task_tokens(), Some(0));
        assert_eq!(budget.remaining_task_turns(), Some(0));
        assert_eq!(budget.remaining_intent_tokens(), Some(0));
        assert_eq!(budget.remaining_intent_turns(), Some(0));
        assert!(budget.is_forced_wrapup());
    }

    #[test]
    fn estimator_averages_and_defaults() {
        assert_eq!(
            estimated_worker_cost(&[]),
            (
                DEFAULT_WORKER_ESTIMATE_TOKENS,
                DEFAULT_WORKER_ESTIMATE_TURNS
            )
        );
        let finished = [
            WorkerCost {
                tokens: 40_000,
                turns: 8,
            },
            WorkerCost {
                tokens: 60_000,
                turns: 12,
            },
        ];
        assert_eq!(estimated_worker_cost(&finished), (50_000, 10));
        assert_eq!(Budget::estimated_worker_cost(&finished), (50_000, 10));
    }
}
