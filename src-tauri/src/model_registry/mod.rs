use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
    pub display_name: String,
    pub description: String,
    pub technical_name: String,
    pub parameter_count: String,
    pub size_bytes: u64,
    pub source_url: String,
    pub quantization: String,
    pub sha256: String,
    pub capability_tags: Vec<String>,
    pub priority: u32,
}

const BUNDLED_REGISTRY: &str = include_str!("registry.json");

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

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
    current_tier_candidates(registry, tier_id)
        .into_iter()
        .min_by_key(|model| model.priority)
}

/// Returns the curated models offered for one hardware tier, preserving the
/// registry order while ignoring any accidental duplicate model ids.
///
/// The same model intentionally appears in several different tiers in the
/// bundled registry. Deduplication happens only within the selected tier so a
/// caller never receives the same option twice without losing the per-tier
/// ordering and recommendation priority.
pub fn current_tier_candidates<'a>(
    registry: &'a Registry,
    tier_id: &str,
) -> Vec<&'a ModelCandidate> {
    let Some(tier) = registry.tiers.iter().find(|tier| tier.tier_id == tier_id) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    tier.models
        .iter()
        .filter(|model| seen.insert(model.model_id.as_str()))
        .collect()
}

/// Finds one curated model by its stable id across all hardware tiers.
/// Models shared by multiple tiers have identical download metadata, so the
/// first registry occurrence is the canonical one.
pub fn find_candidate<'a>(registry: &'a Registry, model_id: &str) -> Option<&'a ModelCandidate> {
    registry
        .tiers
        .iter()
        .flat_map(|tier| tier.models.iter())
        .find(|model| model.model_id == model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tier_has_the_expected_recommendation_and_catalog() {
        let registry = bundled();
        assert_eq!(registry.schema_version, CURRENT_SCHEMA_VERSION);

        let cases = [
            (
                "unknown",
                "qwen3.5-4b-q4_k_m",
                vec!["qwen3.5-4b-q4_k_m", "minicpm5-1b-q4_k_m"],
            ),
            (
                "apple-silicon-8gb",
                "minicpm5-1b-q4_k_m",
                vec!["minicpm5-1b-q4_k_m"],
            ),
            (
                "apple-silicon-16gb",
                "qwen3.5-4b-q4_k_m",
                vec!["qwen3.5-4b-q4_k_m", "minicpm5-1b-q4_k_m"],
            ),
            (
                "apple-silicon-32-to-63gb",
                "qwen3.5-4b-q4_k_m",
                vec!["qwen3.5-4b-q4_k_m", "minicpm5-1b-q4_k_m", "qwen3-8b-q4_k_m"],
            ),
            (
                "apple-silicon-64gb-plus",
                "qwen3-8b-q4_k_m",
                vec!["qwen3-8b-q4_k_m", "qwen3.5-4b-q4_k_m"],
            ),
        ];

        for (tier_id, recommended_id, expected_ids) in cases {
            let m = best_candidate_for_tier(&registry, tier_id)
                .unwrap_or_else(|| panic!("tier {tier_id} must resolve a candidate"));
            assert_eq!(m.model_id, recommended_id, "tier {tier_id}");
            assert_eq!(
                current_tier_candidates(&registry, tier_id)
                    .into_iter()
                    .map(|candidate| candidate.model_id.as_str())
                    .collect::<Vec<_>>(),
                expected_ids,
                "tier {tier_id}"
            );
        }
    }

    #[test]
    fn candidate_metadata_matches_the_curated_downloads() {
        let registry = bundled();
        let cases = [
            (
                "minicpm5-1b-q4_k_m",
                "Faster",
                "MiniCPM5 1B",
                "1B",
                688_065_920,
                "81b64d05a23b17b34c475f42b3e72fbde62d4b92cc34541f7a8031d0752deafa",
            ),
            (
                "qwen3.5-4b-q4_k_m",
                "Balanced",
                "Qwen3.5 4B",
                "4B",
                2_740_937_888,
                "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
            ),
            (
                "qwen3-8b-q4_k_m",
                "Advanced",
                "Qwen3 8B",
                "8B",
                5_027_784_512,
                "120307ba529eb2439d6c430d94104dabd578497bc7bfe7e322b5d9933b449bd4",
            ),
        ];

        for (id, display_name, technical_name, parameter_count, size_bytes, sha256) in cases {
            let candidate = find_candidate(&registry, id)
                .unwrap_or_else(|| panic!("curated model {id} must exist"));
            assert_eq!(candidate.display_name, display_name);
            assert_eq!(candidate.technical_name, technical_name);
            assert_eq!(candidate.parameter_count, parameter_count);
            assert_eq!(candidate.size_bytes, size_bytes);
            assert_eq!(candidate.quantization, "Q4_K_M");
            assert_eq!(candidate.sha256, sha256);
            assert!(!candidate.description.trim().is_empty());
        }
    }

    #[test]
    fn tier_candidates_deduplicate_model_ids_and_unknown_tiers_are_empty() {
        let mut registry = bundled();
        let tier = registry
            .tiers
            .iter_mut()
            .find(|tier| tier.tier_id == "apple-silicon-16gb")
            .expect("16 GB tier");
        tier.models.push(tier.models[0].clone());

        let candidates = current_tier_candidates(&registry, "apple-silicon-16gb");
        assert_eq!(candidates.len(), 2);
        assert!(current_tier_candidates(&registry, "does-not-exist").is_empty());
        assert!(best_candidate_for_tier(&registry, "does-not-exist").is_none());
        assert!(find_candidate(&registry, "does-not-exist").is_none());
    }
}
