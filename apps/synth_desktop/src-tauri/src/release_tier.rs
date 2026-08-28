//! The build-maturity envelope.
//!
//! `contracts/release-tiers-v1.toml` is the single source of truth for which
//! features belong to which maturity tier and which verification items gate a
//! release at each tier. This module embeds it, resolves the compile-time
//! build tier from the `tier-*` cargo features, and answers the two envelope
//! questions: is a feature *included* in this build, and is it currently
//! *enabled* (included AND its runtime flag, if any, is on).
//!
//! The envelope is structural, not advisory: a `tier-*` cargo feature chain
//! (`tier-dev` ⊃ `tier-alpha` ⊃ `tier-beta` ⊃ `tier-stable` ⊃ `tier-core`)
//! compiles gated host code out of narrower builds, and the renderer bundle
//! applies the same tier through Vite defines. Runtime flags can only narrow
//! the envelope, never widen it.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const CONTRACT_TOML: &str = include_str!("../../../../contracts/release-tiers-v1.toml");

// A build must select exactly one point on the tier chain. Every tier implies
// `tier-core`, so its absence means the build opted out of default features
// without choosing a tier.
#[cfg(not(feature = "tier-core"))]
compile_error!(
    "no build tier selected: default features supply tier-stable; \
     with --no-default-features pass one of tier-core/stable/beta/alpha/dev"
);

// Incompatible configuration is a build error, not a warning: the QA control
// plane (eval-driver route table) must be structurally absent from
// stable/core artifacts.
#[cfg(all(feature = "eval-driver", not(feature = "tier-beta")))]
compile_error!(
    "the eval-driver QA control plane cannot ship inside a stable/core envelope; \
     build it with tier-beta, tier-alpha, or tier-dev"
);

/// Maturity tiers, narrowest to widest. Declaration order is the envelope
/// order, so `Ord` compares inclusion breadth.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Core,
    Stable,
    Beta,
    Alpha,
    Dev,
}

impl Tier {
    pub const ALL: [Tier; 5] = [Tier::Core, Tier::Stable, Tier::Beta, Tier::Alpha, Tier::Dev];

    pub const fn name(self) -> &'static str {
        match self {
            Tier::Core => "core",
            Tier::Stable => "stable",
            Tier::Beta => "beta",
            Tier::Alpha => "alpha",
            Tier::Dev => "dev",
        }
    }

    pub fn parse(value: &str) -> Result<Tier> {
        Tier::ALL
            .into_iter()
            .find(|tier| tier.name() == value)
            .ok_or_else(|| anyhow!("unknown tier {value:?}; expected core/stable/beta/alpha/dev"))
    }
}

/// The tier this binary was compiled at: the widest `tier-*` cargo feature.
pub const BUILD_TIER: Tier = if cfg!(feature = "tier-dev") {
    Tier::Dev
} else if cfg!(feature = "tier-alpha") {
    Tier::Alpha
} else if cfg!(feature = "tier-beta") {
    Tier::Beta
} else if cfg!(feature = "tier-stable") {
    Tier::Stable
} else {
    Tier::Core
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Enforcement {
    /// A cargo feature compiles the host code out of excluded tiers.
    Compiled,
    /// A Vite define statically eliminates the renderer code in excluded tiers.
    Bundled,
    /// Maturity classification only; pre-envelope code, not yet gated.
    Declared,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeFlag {
    pub key: String,
    pub default: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FeatureSpec {
    pub name: String,
    pub summary: String,
    pub owner: String,
    pub min_tier: Tier,
    pub enforcement: Enforcement,
    #[serde(default)]
    pub cargo_feature: Option<String>,
    #[serde(default)]
    pub runtime_flag: Option<RuntimeFlag>,
    #[serde(default)]
    pub grandfathered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    Required,
    Recommended,
    Optional,
    Excluded,
}

impl Disposition {
    pub fn name(self) -> &'static str {
        match self {
            Disposition::Required => "required",
            Disposition::Recommended => "recommended",
            Disposition::Optional => "optional",
            Disposition::Excluded => "excluded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerificationKind {
    Test,
    Eval,
    Drill,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dispositions {
    pub core: Disposition,
    pub stable: Disposition,
    pub beta: Disposition,
    pub alpha: Disposition,
    pub dev: Disposition,
}

impl Dispositions {
    pub fn at(&self, tier: Tier) -> Disposition {
        match tier {
            Tier::Core => self.core,
            Tier::Stable => self.stable,
            Tier::Beta => self.beta,
            Tier::Alpha => self.alpha,
            Tier::Dev => self.dev,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationSpec {
    pub name: String,
    pub kind: VerificationKind,
    pub command: String,
    pub dispositions: Dispositions,
}

#[derive(Debug, Deserialize)]
pub struct TierContract {
    pub schema: u32,
    pub contract_version: String,
    pub tier_order: Vec<String>,
    #[serde(rename = "feature")]
    pub features: Vec<FeatureSpec>,
    #[serde(rename = "verification")]
    pub verification: Vec<VerificationSpec>,
}

pub fn contract() -> &'static TierContract {
    static CONTRACT: OnceLock<TierContract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let parsed: TierContract =
            toml::from_str(CONTRACT_TOML).expect("contracts/release-tiers-v1.toml must parse");
        assert_eq!(parsed.schema, 1, "unsupported release-tier contract schema");
        let expected_order: Vec<&str> = Tier::ALL.iter().map(|tier| tier.name()).collect();
        assert_eq!(
            parsed.tier_order, expected_order,
            "tier_order must match the envelope progression; reordering is a contract_version bump"
        );
        let mut seen = std::collections::HashSet::new();
        for feature in &parsed.features {
            assert!(
                seen.insert(feature.name.as_str()),
                "duplicate feature {}",
                feature.name
            );
            assert!(
                feature.owner.starts_with("workshop-"),
                "feature {} owner must be a workshop team",
                feature.name
            );
            // The safety rule: anything above beta maturity must be
            // structurally gated. `declared` exists only to classify code that
            // predates the envelope, and every such feature says so.
            if feature.min_tier >= Tier::Alpha {
                assert!(
                    feature.enforcement != Enforcement::Declared,
                    "feature {} is {}-tier and must be compiled or bundled, not declared",
                    feature.name,
                    feature.min_tier.name()
                );
            }
            if feature.enforcement == Enforcement::Declared {
                assert!(
                    feature.grandfathered,
                    "feature {} is declared-only; new features must be gated (or marked grandfathered)",
                    feature.name
                );
            }
            if feature.enforcement == Enforcement::Compiled {
                assert!(
                    feature.cargo_feature.is_some(),
                    "compiled feature {} must name its cargo_feature",
                    feature.name
                );
            }
        }
        let mut seen = std::collections::HashSet::new();
        for item in &parsed.verification {
            assert!(
                seen.insert(item.name.as_str()),
                "duplicate verification item {}",
                item.name
            );
            assert!(
                !item.command.trim().is_empty(),
                "verification item {} needs a command",
                item.name
            );
        }
        parsed
    })
}

pub fn feature(name: &str) -> Option<&'static FeatureSpec> {
    contract()
        .features
        .iter()
        .find(|feature| feature.name == name)
}

/// Whether a feature is *classified* into the envelope of `tier`.
pub fn included_at(spec: &FeatureSpec, tier: Tier) -> bool {
    spec.min_tier <= tier
}

/// Whether the feature's code is actually in a build of `tier`.
///
/// For structurally gated features (compiled/bundled) this equals
/// [`included_at`]. A grandfathered `declared` feature predates the envelope
/// and ships in every build until its gate lands — the gap between
/// classification and presence is the visible gating backlog, never a reason
/// to report working code as absent.
pub fn present_at(spec: &FeatureSpec, tier: Tier) -> bool {
    included_at(spec, tier) || (spec.enforcement == Enforcement::Declared && spec.grandfathered)
}

/// Whether the feature's code is in *this* build.
pub fn feature_included(name: &str) -> bool {
    feature(name).is_some_and(|spec| present_at(spec, BUILD_TIER))
}

/// Whether the feature is present AND its runtime flag (if any) is on.
/// Resolution: `WORKSHOP_FLAG_<KEY>` env override, else the contract default.
/// The envelope caps this: a feature absent from the build is never enabled.
pub fn feature_enabled(name: &str) -> bool {
    let Some(spec) = feature(name) else {
        return false;
    };
    if !present_at(spec, BUILD_TIER) {
        return false;
    }
    runtime_flag_on(spec)
}

fn runtime_flag_on(spec: &FeatureSpec) -> bool {
    let Some(flag) = &spec.runtime_flag else {
        return true;
    };
    let env_key = format!("WORKSHOP_FLAG_{}", flag.key.to_ascii_uppercase());
    match std::env::var(env_key) {
        Ok(value) => matches!(value.trim(), "1" | "true" | "on" | "yes"),
        Err(_) => flag.default,
    }
}

/// The verification plan for releasing at `tier`, grouped by disposition.
#[derive(Clone, Debug, Serialize)]
pub struct VerificationPlan {
    pub tier: Tier,
    pub required: Vec<&'static VerificationSpec>,
    pub recommended: Vec<&'static VerificationSpec>,
    pub optional: Vec<&'static VerificationSpec>,
    pub excluded: Vec<&'static VerificationSpec>,
}

pub fn plan_for(tier: Tier) -> VerificationPlan {
    let mut plan = VerificationPlan {
        tier,
        required: Vec::new(),
        recommended: Vec::new(),
        optional: Vec::new(),
        excluded: Vec::new(),
    };
    for item in &contract().verification {
        match item.dispositions.at(tier) {
            Disposition::Required => plan.required.push(item),
            Disposition::Recommended => plan.recommended.push(item),
            Disposition::Optional => plan.optional.push(item),
            Disposition::Excluded => plan.excluded.push(item),
        }
    }
    plan
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FeatureReport {
    pub name: String,
    pub summary: String,
    pub owner: String,
    pub min_tier: Tier,
    pub enforcement: Enforcement,
    /// Classified inside this build's envelope (min_tier ≤ build tier).
    pub included: bool,
    /// Actually in the binary: `included`, or grandfathered pre-envelope code.
    pub present: bool,
    pub enabled: bool,
    pub runtime_flag: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseTierReport {
    pub tier: Tier,
    pub contract_version: String,
    pub features: Vec<FeatureReport>,
}

pub fn report() -> ReleaseTierReport {
    ReleaseTierReport {
        tier: BUILD_TIER,
        contract_version: contract().contract_version.clone(),
        features: contract()
            .features
            .iter()
            .map(|spec| FeatureReport {
                name: spec.name.clone(),
                summary: spec.summary.clone(),
                owner: spec.owner.clone(),
                min_tier: spec.min_tier,
                enforcement: spec.enforcement,
                included: included_at(spec, BUILD_TIER),
                present: present_at(spec, BUILD_TIER),
                enabled: present_at(spec, BUILD_TIER) && runtime_flag_on(spec),
                runtime_flag: spec.runtime_flag.as_ref().map(|flag| flag.key.clone()),
            })
            .collect(),
    }
}

/// Display-safe build envelope for the renderer: the compiled tier and the
/// per-feature included/enabled resolution.
#[tauri::command]
#[specta::specta]
pub fn release_tier_get() -> ReleaseTierReport {
    report()
}

