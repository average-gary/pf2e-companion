//! PF2e Remaster rules helpers — pure functions, no DB.
//!
//! Sources:
//! - GM Core p.49 — encounter XP budgets (per party of 4; ±20 per PC delta).
//! - GM Core p.49 — creature XP cost by level-vs-PL delta.
//! - Player Core — sanctification trait taxonomy.
//!
//! Kept deliberately small in Phase 1; the `validate_statblock` helper is a
//! schema sanity check, not a deep rules engine.

use serde::{Deserialize, Serialize};

/// Encounter difficulty band (Remaster names).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Trivial,
    Low,
    Moderate,
    Severe,
    Extreme,
}

impl Difficulty {
    /// Per-PC adjustment when scaling away from the canonical 4-PC budget.
    /// PF2e GM Core: ±20 XP per PC for Low; ±20 for Moderate; ±30 for Severe;
    /// ±40 for Extreme; ±10 for Trivial.
    pub fn per_pc_adjust(self) -> i32 {
        match self {
            Difficulty::Trivial => 10,
            Difficulty::Low => 20,
            Difficulty::Moderate => 20,
            Difficulty::Severe => 30,
            Difficulty::Extreme => 40,
        }
    }

    pub fn base_budget_for_party_of_4(self) -> i32 {
        match self {
            Difficulty::Trivial => 40,
            Difficulty::Low => 60,
            Difficulty::Moderate => 80,
            Difficulty::Severe => 120,
            Difficulty::Extreme => 160,
        }
    }
}

/// Compute the XP budget for an encounter.
pub fn xp_budget(party_size: u8, difficulty: Difficulty) -> i32 {
    let base = difficulty.base_budget_for_party_of_4();
    let delta = (party_size as i32) - 4;
    base + delta * difficulty.per_pc_adjust()
}

/// Creature XP cost relative to party level. PF2e GM Core p.49.
pub fn creature_xp_for_party_level_delta(delta: i32) -> Option<i32> {
    Some(match delta {
        d if d <= -4 => 10,
        -3 => 15,
        -2 => 20,
        -1 => 30,
        0 => 40,
        1 => 60,
        2 => 80,
        3 => 120,
        4 => 160,
        _ => return None,
    })
}

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Lightweight statblock sanity check. Phase 1 ships *structural* validation;
/// PF2e math (level-vs-AC bands, save expectations) is deferred until the
/// Foundry-pf2e schema is mirrored locally.
pub fn validate_statblock(value: &serde_json::Value) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            return ValidationResult {
                valid: false,
                errors: vec!["statblock must be a JSON object".into()],
                warnings,
            };
        }
    };

    for required in ["name", "level", "type"] {
        if !obj.contains_key(required) {
            errors.push(format!("missing required field `{required}`"));
        }
    }

    if let Some(level) = obj.get("level").and_then(|v| v.as_i64()) {
        if !(-1..=25).contains(&level) {
            errors.push(format!("level {level} outside the PF2e range -1..=25"));
        }
    }

    // Sanctification must be one of the 5 Remaster values (per
    // [[remaster-monotheism-fit]] and [[yhwh-deity-template]]).
    if let Some(sanct) = obj.get("sanctification").and_then(|v| v.as_str()) {
        let ok = matches!(
            sanct,
            "holy" | "unholy" | "both" | "none" | "can-choose-holy" | "can-choose-unholy"
        );
        if !ok {
            errors.push(format!(
                "sanctification `{sanct}` is not one of the 5 Remaster values"
            ));
        }
    }

    if let Some(license) = obj.get("license_provenance").and_then(|v| v.as_str()) {
        let ok = matches!(license, "orc" | "community-use" | "homebrew" | "proprietary");
        if !ok {
            errors.push(format!(
                "license_provenance `{license}` not in [orc, community-use, homebrew, proprietary]"
            ));
        }
    } else {
        warnings.push(
            "no license_provenance — defaulting to homebrew on import".into(),
        );
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn xp_budget_canonical_party_of_4() {
        assert_eq!(xp_budget(4, Difficulty::Trivial), 40);
        assert_eq!(xp_budget(4, Difficulty::Low), 60);
        assert_eq!(xp_budget(4, Difficulty::Moderate), 80);
        assert_eq!(xp_budget(4, Difficulty::Severe), 120);
        assert_eq!(xp_budget(4, Difficulty::Extreme), 160);
    }

    #[test]
    fn xp_budget_scales_with_party_size() {
        assert_eq!(xp_budget(5, Difficulty::Severe), 150); // +30
        assert_eq!(xp_budget(3, Difficulty::Severe), 90); // -30
        assert_eq!(xp_budget(6, Difficulty::Extreme), 240); // +80
    }

    #[test]
    fn creature_xp_table() {
        assert_eq!(creature_xp_for_party_level_delta(0), Some(40));
        assert_eq!(creature_xp_for_party_level_delta(-1), Some(30));
        assert_eq!(creature_xp_for_party_level_delta(4), Some(160));
        assert_eq!(creature_xp_for_party_level_delta(-4), Some(10));
        assert_eq!(creature_xp_for_party_level_delta(5), None);
    }

    #[test]
    fn validate_minimal_statblock() {
        let ok = json!({"name": "Goblin", "level": -1, "type": "creature"});
        let r = validate_statblock(&ok);
        assert!(r.valid, "errors: {:?}", r.errors);
    }

    #[test]
    fn validate_rejects_bad_sanctification() {
        let bad = json!({
            "name": "X", "level": 1, "type": "creature",
            "sanctification": "lawful-good"
        });
        let r = validate_statblock(&bad);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("sanctification")));
    }

    #[test]
    fn validate_warns_on_missing_license() {
        let s = json!({"name": "X", "level": 1, "type": "creature"});
        let r = validate_statblock(&s);
        assert!(r.valid);
        assert!(!r.warnings.is_empty());
    }
}
