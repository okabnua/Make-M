//! Shared ARTEX domain types — port of `db/constants.go` string enums.
//!
//! These constants are the wire/database contract: the string values are
//! persisted in Postgres and sent over the API, so they must match the original
//! byte-for-byte.

/// Exploration node kinds (`exploration_nodes.kind`).
pub mod node_kind {
    /// DEPRECATED: legacy task root; new tasks seed an origin fact instead.
    pub const BEGIN: &str = "begin";
    /// A task objective.
    pub const GOAL: &str = "goal";
    /// An exploration direction (planner-generated).
    pub const INTENT: &str = "intent";
    /// A worker's exploration result/conclusion (incl. negative results).
    pub const FACT: &str = "fact";
    /// A confirmed vulnerability (report_finding), distinct from a fact.
    pub const FINDING: &str = "finding";
    pub const HINT: &str = "hint";
    /// A compressed fold of cold intents/facts; lossless, restorable by id.
    pub const DIGEST: &str = "digest";
}

/// Digest node states (`kind='digest'`).
pub mod digest_state {
    pub const ACTIVE: &str = "active";
    pub const SUPERSEDED: &str = "superseded";
}

/// Exploration node states.
pub mod node_state {
    /// Marks the task root fact (KindFact) seeded at task creation.
    pub const ORIGIN: &str = "origin";
    /// Marks an intent soft-deleted by the user.
    pub const DELETED: &str = "deleted";
}

/// Exploration edge relations (`exploration_edges.rel`).
pub mod edge_rel {
    pub const SPAWNS: &str = "spawns";
    pub const DERIVED_FROM: &str = "derived_from";
    pub const YIELDS: &str = "yields";
    pub const PROVES: &str = "proves";
    /// digest --covers--> member.
    pub const COVERS: &str = "covers";
}

/// Asset node types (`assets.type`).
pub mod asset_type {
    pub const ROOT_DOMAIN: &str = "root_domain";
    pub const IP: &str = "ip";
    pub const SUBDOMAIN: &str = "subdomain";
    pub const APP: &str = "app";
    pub const SERVICE: &str = "service";
    pub const ENDPOINT: &str = "endpoint";
}

/// Task status (`tasks.status`).
pub mod task_status {
    pub const CREATED: &str = "created";
    pub const RUNNING: &str = "running";
    pub const PAUSED: &str = "paused";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const TIMEOUT: &str = "timeout";
}

/// LLM provider formats (`llm_profiles.format`).
pub mod llm_format {
    pub const OPENAI: &str = "openai";
    pub const ANTHROPIC: &str = "anthropic";
    pub const OPENAI_RESPONSES: &str = "openai-responses";
    pub const OLLAMA: &str = "ollama";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_original() {
        assert_eq!(node_kind::BEGIN, "begin");
        assert_eq!(node_kind::FINDING, "finding");
        assert_eq!(node_kind::DIGEST, "digest");
        assert_eq!(digest_state::SUPERSEDED, "superseded");
        assert_eq!(node_state::ORIGIN, "origin");
        assert_eq!(node_state::DELETED, "deleted");
        assert_eq!(edge_rel::SPAWNS, "spawns");
        assert_eq!(edge_rel::DERIVED_FROM, "derived_from");
        assert_eq!(edge_rel::COVERS, "covers");
        assert_eq!(asset_type::ROOT_DOMAIN, "root_domain");
        assert_eq!(asset_type::ENDPOINT, "endpoint");
        assert_eq!(task_status::TIMEOUT, "timeout");
        assert_eq!(llm_format::OPENAI_RESPONSES, "openai-responses");
    }
}
