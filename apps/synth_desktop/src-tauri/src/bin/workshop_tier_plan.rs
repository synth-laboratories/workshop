//! Resolve the release-tier contract for release tooling.
//!
//! `workshop-tier-plan <tier>` prints, as JSON, the feature envelope and the
//! verification plan for releasing at that tier. scripts/release-gate.sh is
//! the primary consumer; humans get the same JSON. Resolution is pure
//! contract data — it does not depend on the tier this binary was built at.

use serde_json::json;
use synth_desktop_lib::release_tier::{self, Tier, VerificationSpec};

fn items(list: &[&'static VerificationSpec]) -> Vec<serde_json::Value> {
    list.iter()
        .map(|item| {
            json!({
                "name": item.name,
                "kind": item.kind,
                "command": item.command,
            })
        })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tier = match args.next().as_deref().map(Tier::parse) {
        Some(Ok(tier)) => tier,
        Some(Err(error)) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
        None => {
            eprintln!("usage: workshop-tier-plan <core|stable|beta|alpha|dev>");
            std::process::exit(2);
        }
    };

    let contract = release_tier::contract();
    let plan = release_tier::plan_for(tier);
    let mut included = Vec::new();
    let mut grandfathered = Vec::new();
    let mut excluded = Vec::new();
    for spec in &contract.features {
        if release_tier::included_at(spec, tier) {
            included.push(spec.name.clone());
        } else if release_tier::present_at(spec, tier) {
            // Pre-envelope code above this tier's classification: the gating
            // backlog a promotion review must look at.
            grandfathered.push(spec.name.clone());
        } else {
            excluded.push(spec.name.clone());
        }
    }

    let output = json!({
        "contractVersion": contract.contract_version,
        "tier": tier,
        "features": {
            "included": included,
            "grandfatheredAboveEnvelope": grandfathered,
            "excluded": excluded,
        },
        "verification": {
            "required": items(&plan.required),
            "recommended": items(&plan.recommended),
            "optional": items(&plan.optional),
            "excluded": items(&plan.excluded),
        },
    });
    println!("{}", serde_json::to_string_pretty(&output).expect("plan serializes"));
}
