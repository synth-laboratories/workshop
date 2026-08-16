//! Lists the Codex skills Synth Desktop bundles and installs into each
//! session's Codex home (see `codex.rs::ensure_home`).
//!
//! Reads from the same `include_str!`-embedded `SKILL.md` sources
//! `ensure_home` writes, rather than scanning a session's Codex home on disk:
//! a session home only exists once a Codex session has started, so scanning
//! it would make the list empty (or stale) most of the time. The embedded
//! copies are always present, in the packaged app and in development.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillHit {
    pub id: String,
    pub name: String,
    pub description: String,
}

struct BundledSkill {
    id: &'static str,
    content: &'static str,
}

const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        id: "use-synth-containers",
        content: include_str!("../../skills/use-synth-containers/SKILL.md"),
    },
    BundledSkill {
        id: "use-synth-visuals",
        content: include_str!("../../skills/use-synth-visuals/SKILL.md"),
    },
    BundledSkill {
        id: "use-synth-optimizers",
        content: include_str!("../../skills/use-synth-optimizers/SKILL.md"),
    },
    BundledSkill {
        id: "use-synth-plugins",
        content: include_str!("../../skills/use-synth-plugins/SKILL.md"),
    },
    BundledSkill {
        id: "use-synth-session",
        content: include_str!("../../skills/use-synth-session/SKILL.md"),
    },
    BundledSkill {
        id: "use-synth-diagnostics",
        content: include_str!("../../skills/use-synth-diagnostics/SKILL.md"),
    },
    BundledSkill {
        id: "run-live-container-evals",
        content: include_str!("../../skills/run-live-container-evals/SKILL.md"),
    },
    BundledSkill {
        id: "author-synth-diagrams",
        content: include_str!("../../skills/author-synth-diagrams/SKILL.md"),
    },
];

pub(crate) fn bundled_skill_content(id: &str) -> Option<&'static str> {
    BUNDLED_SKILLS
        .iter()
        .find(|skill| skill.id == id)
        .map(|skill| skill.content)
}

/// Reads a single `key: value` field out of a SKILL.md's leading `---`
/// YAML frontmatter block. Deliberately minimal: these files only ever use
/// flat scalar fields (`name`, `description`), never nested YAML.
fn frontmatter_field(content: &str, key: &str) -> Option<String> {
    let body = content.strip_prefix("---")?;
    let body = body.strip_prefix('\n').unwrap_or(body);
    let end = body.find("\n---")?;
    let frontmatter = &body[..end];
    let prefix = format!("{key}:");
    for line in frontmatter.lines() {
        if let Some(rest) = line.strip_prefix(prefix.as_str()) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

pub fn list_skills() -> Vec<SkillHit> {
    BUNDLED_SKILLS
        .iter()
        .map(|skill| SkillHit {
            id: skill.id.into(),
            name: frontmatter_field(skill.content, "name").unwrap_or_else(|| skill.id.into()),
            description: frontmatter_field(skill.content, "description").unwrap_or_default(),
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn skills_list() -> Vec<SkillHit> {
    list_skills()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract drift guard: the container skills are the only place the agent
    /// is told to stop instead of wandering to a raw engine, another port, or
    /// an old rollout stream. Softening this text silently re-opens the
    /// twenty-minute failure it replaced.
    #[test]
    fn container_skills_keep_the_fail_fast_and_no_prior_evidence_language() {
        const REQUIRED: [&str; 12] = [
            "compatible_runtime_unavailable",
            "container_unhealthy",
            "container_capabilities_stale",
            "container_capability_mismatch",
            "do not try raw engines, alternate ports, archived rollouts, or prior traces.",
            "evidence must match the current invocation's rollout ids and requested seeds.",
            "missing sealed trace v5 means the requested task is incomplete.",
            "proves liveness, not workflow compatibility",
            "sse support does not imply prepared-rollout support",
            "never fall back from a selected policy pool to raw gold",
            "do not perform shell or repository archaeology",
            "prior evidence may be reported as prior evidence only",
        ];
        for id in ["use-synth-containers", "run-live-container-evals"] {
            let content = bundled_skill_content(id).expect("bundled skill");
            let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let normalized = normalized.to_ascii_lowercase();
            for phrase in REQUIRED {
                assert!(
                    normalized.contains(phrase),
                    "{id} lost required capability-gating language: {phrase}"
                );
            }
        }
    }

    #[test]
    fn parses_name_and_description_from_every_bundled_skill() {
        let hits = list_skills();
        assert_eq!(hits.len(), BUNDLED_SKILLS.len());
        for hit in &hits {
            assert!(!hit.name.is_empty(), "{} missing a name", hit.id);
            assert!(
                !hit.description.is_empty(),
                "{} missing a description",
                hit.id
            );
        }
    }
}
