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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tier_resolves_to_the_single_qwen35_model() {
        let registry = bundled();
        let tiers = [
            "apple-silicon-8gb",
            "apple-silicon-16gb",
            "apple-silicon-32gb",
            "apple-silicon-64gb-plus",
        ];
        for tier_id in tiers {
            let m = best_candidate_for_tier(&registry, tier_id)
                .unwrap_or_else(|| panic!("tier {tier_id} must resolve a candidate"));
            assert_eq!(m.model_id, "qwen3.5-4b-q4_k_m", "tier {tier_id}");
            assert_eq!(
                m.sha256, "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
                "tier {tier_id}"
            );
        }
    }
}
