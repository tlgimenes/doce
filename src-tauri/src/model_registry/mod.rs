use serde::{Deserialize, Serialize};

/// research.md §23: versioned registry, bundled fallback + remote refresh.
// Not renamed to camelCase: these are internal-only (never returned
// directly from a command) and parsed from the hand-written, snake_case
// `registry.json` below.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Registry {
    pub schema_version: u32,
    pub updated_at: String,
    pub tiers: Vec<Tier>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Tier {
    pub tier_id: String,
    pub min_ram_gb: u32,
    pub chip_families: Vec<String>,
    pub models: Vec<ModelCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ModelCandidate {
    pub model_id: String,
    pub source_url: String,
    pub quantization: String,
    pub sha256: String,
    pub capability_tags: Vec<String>,
    pub priority: u32,
}

const BUNDLED_REGISTRY: &str = include_str!("registry.json");

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Loads the bundled registry (always available, no network dependency —
/// FR-002's onboarding must never block on network access to the registry
/// itself, only the subsequent model download).
pub fn bundled() -> Registry {
    serde_json::from_str(BUNDLED_REGISTRY).expect("bundled registry.json must be valid")
}

/// Picks the best candidate model for a hardware tier id, ordered by
/// `priority` so a fallback is available if the top choice fails
/// (research.md §23).
pub fn best_candidate_for_tier<'a>(
    registry: &'a Registry,
    tier_id: &str,
) -> Option<&'a ModelCandidate> {
    registry
        .tiers
        .iter()
        .find(|t| t.tier_id == tier_id)
        .and_then(|t| t.models.iter().min_by_key(|m| m.priority))
}
