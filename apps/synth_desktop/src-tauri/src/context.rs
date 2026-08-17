//! Inspectable, durable context controls used when materializing new Codex homes.

use crate::{instance, skills};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

pub const WORKSHOP_AGENTS: &str = include_str!("../../context/WORKSHOP_AGENTS.md");
const COOKBOOKS_REPO: &str = "https://github.com/synth-laboratories/synth-cookbooks-public.git";
static CANCEL_COOKBOOKS: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextFile {
    pub path: String,
    pub content: String,
    pub state: String,
    pub editable: bool,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub enabled: bool,
    pub editable: bool,
    pub content: String,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpContextGroup {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub servers: Vec<String>,
    pub enabled_tools: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CookbookContext {
    pub enabled: bool,
    pub installed: bool,
    pub phase: String,
    pub pin: Option<String>,
    pub digest: Option<String>,
    pub path: Option<String>,
    pub last_fetch: Option<String>,
    pub detail: Option<String>,
}

impl Default for CookbookContext {
    fn default() -> Self {
        Self {
            enabled: false,
            installed: false,
            phase: "off".into(),
            pin: None,
            digest: None,
            path: None,
            last_fetch: None,
            detail: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub workshop_agents: ContextFile,
    pub workspace_agents: ContextFile,
    pub cookbooks: CookbookContext,
    pub skills: Vec<ContextSkill>,
    pub mcp_groups: Vec<McpContextGroup>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextSettings {
    pub skill_enabled: BTreeMap<String, bool>,
    pub skill_overrides: BTreeMap<String, String>,
    pub mcp_group_enabled: BTreeMap<String, bool>,
    pub cookbooks: CookbookContext,
}

fn root() -> PathBuf {
    instance::state_root().join("context")
}
fn settings_path() -> PathBuf {
    root().join("settings.json")
}

pub fn settings() -> ContextSettings {
    fs::read(settings_path())
        .ok()
        .and_then(|body| serde_json::from_slice(&body).ok())
        .unwrap_or_default()
}

fn save(value: &ContextSettings) -> Result<()> {
    fs::create_dir_all(root())?;
    let temporary = settings_path().with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, settings_path())?;
    Ok(())
}

pub fn skill_enabled(id: &str) -> bool {
    let current = settings();
    if id == "use-synth-cookbooks" {
        return current.cookbooks.enabled
            && current.cookbooks.installed
            && current
                .cookbooks
                .path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_dir());
    }
    current.skill_enabled.get(id).copied().unwrap_or(true)
}

pub fn skill_override(id: &str) -> Option<String> {
    settings().skill_overrides.get(id).cloned()
}
/// Groups that must be switched on deliberately rather than inherited.
///
/// Everything else defaults on, which is right for tools that read and write
/// inside Workshop. Computer Use drives the operator's other applications, so
/// arriving switched on by virtue of existing is the wrong default: it should
/// be a thing someone turned on, not a thing they failed to turn off.
pub const OPT_IN_MCP_GROUPS: [&str; 2] = [COMPUTER_USE_MCP_GROUP, BROWSER_MCP_GROUP];

pub const COMPUTER_USE_MCP_GROUP: &str = "computer-use";
pub const BROWSER_MCP_GROUP: &str = "browser";

pub const MCP_GROUPS: [&str; 5] = [
    "bundled",
    "productivity",
    "development",
    COMPUTER_USE_MCP_GROUP,
    BROWSER_MCP_GROUP,
];

fn mcp_group_default(id: &str) -> bool {
    !OPT_IN_MCP_GROUPS.contains(&id)
}

pub fn mcp_group_enabled(id: &str) -> bool {
    settings()
        .mcp_group_enabled
        .get(id)
        .copied()
        .unwrap_or_else(|| mcp_group_default(id))
}
pub fn cookbook() -> CookbookContext {
    settings().cookbooks
}

fn workspace_file(workspace: &str) -> ContextFile {
    let path = Path::new(workspace).join("AGENTS.md");
    match fs::read_to_string(&path) {
        Ok(content) => ContextFile {
            path: path.display().to_string(),
            state: if content.trim().is_empty() {
                "empty".into()
            } else {
                "overriding".into()
            },
            content,
            editable: true,
            version: None,
        },
        Err(_) => ContextFile {
            path: path.display().to_string(),
            state: "absent".into(),
            content: String::new(),
            editable: true,
            version: None,
        },
    }
}

fn mcp_groups(current: &ContextSettings) -> Vec<McpContextGroup> {
    let group =
        |id: &str, label: &str, servers: &[&str], tools: &[(&str, &[&str])]| McpContextGroup {
            id: id.into(),
            label: label.into(),
            enabled: current
                .mcp_group_enabled
                .get(id)
                .copied()
                .unwrap_or_else(|| mcp_group_default(id)),
            servers: servers.iter().map(|value| (*value).into()).collect(),
            enabled_tools: tools
                .iter()
                .map(|(server, names)| {
                    (
                        (*server).into(),
                        names.iter().map(|name| (*name).into()).collect(),
                    )
                })
                .collect(),
        };
    vec![
        group(
            "bundled",
            "Bundled",
            &["synth_containers", "synth_visuals", "synth_optimizers"],
            &[
                ("synth_containers", &["container_manage"]),
                ("synth_visuals", &["visual_manage"]),
                (
                    "synth_optimizers",
                    &[
                        "optimizer_manage",
                        "optimizer_stage_eval_candidates",
                        "optimizer_start_recipe",
                    ],
                ),
            ],
        ),
        group("productivity", "Productivity", &[], &[]),
        group("development", "Development", &[], &[]),
        group(
            COMPUTER_USE_MCP_GROUP,
            "Computer Use",
            &["synth_computer_use"],
            &[(
                "synth_computer_use",
                &["computer_use", "computer_use_status"],
            )],
        ),
        group(
            BROWSER_MCP_GROUP,
            "Managed Browser",
            &["synth_browser"],
            &[(
                "synth_browser",
                &[
                    "browser_status",
                    "browser_create_session",
                    "browser_claim_chrome",
                    "browser_close_session",
                    "browser_list_tabs",
                    "browser_new_tab",
                    "browser_close_tab",
                    "browser_navigate",
                    "browser_back",
                    "browser_snapshot",
                    "browser_query",
                    "browser_subtree",
                    "browser_click",
                    "browser_fill",
                    "browser_press",
                    "browser_scroll",
                    "browser_screenshot",
                    "browser_upload",
                    "browser_download",
                    "browser_list_dialogs",
                    "browser_handle_dialog",
                    "browser_audit",
                ],
            )],
        ),
    ]
}

pub fn snapshot(workspace: &str) -> ContextSnapshot {
    let current = settings();
    let mut listed = skills::list_skills()
        .into_iter()
        .map(|skill| {
            let bundled = skills::bundled_skill_content(&skill.id).unwrap_or_default();
            let content = current
                .skill_overrides
                .get(&skill.id)
                .cloned()
                .unwrap_or_else(|| bundled.to_string());
            ContextSkill {
                id: skill.id.clone(),
                name: skill.name,
                description: skill.description,
                source: if current.skill_overrides.contains_key(&skill.id) {
                    "yours".into()
                } else {
                    "bundled".into()
                },
                enabled: current
                    .skill_enabled
                    .get(&skill.id)
                    .copied()
                    .unwrap_or(true),
                editable: true,
                content,
                path: current.skill_overrides.contains_key(&skill.id).then(|| {
                    root()
                        .join("skills")
                        .join(&skill.id)
                        .join("SKILL.md")
                        .display()
                        .to_string()
                }),
            }
        })
        .collect::<Vec<_>>();
    let cookbook_skill_enabled = skill_enabled("use-synth-cookbooks");
    listed.push(ContextSkill { id: "use-synth-cookbooks".into(), name: "use-synth-cookbooks".into(), description: "Read the pinned public cookbook checkout without treating run residue or private overlays as recipes.".into(), source: "cookbook".into(), enabled: cookbook_skill_enabled, editable: false, content: cookbook_skill(&current.cookbooks).unwrap_or_default(), path: current.cookbooks.path.clone() });
    ContextSnapshot {
        workshop_agents: ContextFile {
            path: "bundled://WORKSHOP_AGENTS.md".into(),
            content: WORKSHOP_AGENTS.into(),
            state: "bundled".into(),
            editable: false,
            version: Some(env!("CARGO_PKG_VERSION").into()),
        },
        workspace_agents: workspace_file(workspace),
        cookbooks: current.cookbooks.clone(),
        skills: listed,
        mcp_groups: mcp_groups(&current),
    }
}

pub fn cookbook_skill(cookbook: &CookbookContext) -> Option<String> {
    if !(cookbook.enabled && cookbook.installed) {
        return None;
    }
    let path = cookbook.path.as_deref()?;
    Some(format!("---\nname: use-synth-cookbooks\ndescription: Use the digest-pinned public Synth cookbook checkout.\n---\n\nThe public cookbook pin is available read-only at `{path}` (commit `{}`). Read recipes and skills there when relevant. Never read or advertise `runs/`, infer a missing recipe, or treat private overlays as public.\n", cookbook.pin.as_deref().unwrap_or("unknown")))
}

#[tauri::command]
#[specta::specta]
pub fn context_snapshot(workspace: String) -> Result<ContextSnapshot, crate::error::AppError> {
    Ok(snapshot(&workspace))
}

#[tauri::command]
#[specta::specta]
pub fn context_workspace_agents_update(
    workspace: String,
    content: String,
) -> Result<ContextSnapshot, crate::error::AppError> {
    let path = Path::new(&workspace).join("AGENTS.md");
    fs::create_dir_all(Path::new(&workspace)).map_err(crate::error::AppError::from)?;
    fs::write(path, content).map_err(crate::error::AppError::from)?;
    Ok(snapshot(&workspace))
}

#[tauri::command]
#[specta::specta]
pub fn context_skill_update(
    workspace: String,
    skill_id: String,
    enabled: bool,
    content: Option<String>,
) -> Result<ContextSnapshot, crate::error::AppError> {
    if skills::bundled_skill_content(&skill_id).is_none() {
        return Err(crate::error::AppError::from(anyhow!(
            "unknown editable skill"
        )));
    }
    let mut current = settings();
    current.skill_enabled.insert(skill_id.clone(), enabled);
    if let Some(body) = content {
        let directory = root().join("skills").join(&skill_id);
        fs::create_dir_all(&directory).map_err(crate::error::AppError::from)?;
        fs::write(directory.join("SKILL.md"), &body).map_err(crate::error::AppError::from)?;
        current.skill_overrides.insert(skill_id, body);
    }
    save(&current).map_err(crate::error::AppError::from)?;
    Ok(snapshot(&workspace))
}

#[tauri::command]
#[specta::specta]
pub fn context_mcp_group_update(
    workspace: String,
    group_id: String,
    enabled: bool,
) -> Result<ContextSnapshot, crate::error::AppError> {
    if !MCP_GROUPS.contains(&group_id.as_str()) {
        return Err(crate::error::AppError::from(anyhow!("unknown MCP group")));
    }
    let mut current = settings();
    current.mcp_group_enabled.insert(group_id, enabled);
    save(&current).map_err(crate::error::AppError::from)?;
    Ok(snapshot(&workspace))
}

fn git(directory: Option<&Path>, arguments: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    if let Some(path) = directory {
        command.arg("-C").arg(path);
    }
    let output = command
        .args(arguments)
        .output()
        .context("could not run git")?;
    if !output.status.success() {
        return Err(anyhow!(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn clone_cancellable(destination: &Path) -> Result<()> {
    let mut child = Command::new("git")
        .args([
            "clone",
            "--filter=blob:none",
            "--depth",
            "1",
            "--no-checkout",
            COOKBOOKS_REPO,
        ])
        .arg(destination)
        .spawn()
        .context("could not start git clone")?;
    loop {
        if CANCEL_COOKBOOKS.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("Cookbook installation cancelled"));
        }
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(anyhow!("git clone failed"))
            };
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[tauri::command]
#[specta::specta]
pub async fn context_cookbooks_install(
    workspace: String,
) -> Result<ContextSnapshot, crate::error::AppError> {
    CANCEL_COOKBOOKS.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<()> {
        let mut current = settings();
        current.cookbooks.phase = "cloning".into();
        current.cookbooks.detail = Some("Fetching the public cookbook pin".into());
        save(&current)?;
        let cookbooks_root = instance::state_root().join("cookbooks");
        fs::create_dir_all(&cookbooks_root)?;
        let staging = cookbooks_root.join("installing");
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        clone_cancellable(&staging)?;
        git(
            Some(&staging),
            &[
                "sparse-checkout",
                "set",
                "--no-cone",
                "/skills/",
                "/cookbooks/",
                "!/**/runs/",
            ],
        )?;
        git(Some(&staging), &["checkout"])?;
        let pin = git(Some(&staging), &["rev-parse", "HEAD"])?;
        let destination = cookbooks_root.join(&pin);
        if destination.exists() {
            fs::remove_dir_all(&destination)?;
        }
        fs::rename(&staging, &destination)?;
        let mut hasher = Sha256::new();
        hasher.update(pin.as_bytes());
        let digest = format!("sha256:{:x}", hasher.finalize());
        current = settings();
        current.cookbooks = CookbookContext {
            enabled: true,
            installed: true,
            phase: "ready".into(),
            pin: Some(pin),
            digest: Some(digest),
            path: Some(destination.display().to_string()),
            last_fetch: Some(Utc::now().to_rfc3339()),
            detail: None,
        };
        save(&current)
    })
    .await
    .map_err(|error| crate::error::AppError::from(anyhow!(error.to_string())))?;
    if let Err(error) = result {
        let staging = instance::state_root().join("cookbooks").join("installing");
        if staging.is_dir() {
            let _ = fs::remove_dir_all(staging);
        }
        let mut current = settings();
        let cancelled = error.to_string() == "Cookbook installation cancelled";
        current.cookbooks.phase = if cancelled {
            if current.cookbooks.installed {
                "installed"
            } else {
                "off"
            }
        } else {
            "error"
        }
        .into();
        current.cookbooks.detail = Some(if cancelled {
            "Cookbook installation cancelled".into()
        } else {
            error.to_string()
        });
        let _ = save(&current);
        return Err(crate::error::AppError::from(error));
    }
    Ok(snapshot(&workspace))
}

#[tauri::command]
#[specta::specta]
pub fn context_cookbooks_cancel(
    workspace: String,
) -> Result<ContextSnapshot, crate::error::AppError> {
    CANCEL_COOKBOOKS.store(true, Ordering::Relaxed);
    let mut current = settings();
    current.cookbooks.phase = if current.cookbooks.installed {
        "installed".into()
    } else {
        "off".into()
    };
    current.cookbooks.detail = Some("Cancellation requested".into());
    save(&current).map_err(crate::error::AppError::from)?;
    Ok(snapshot(&workspace))
}

#[tauri::command]
#[specta::specta]
pub fn context_cookbooks_set_enabled(
    workspace: String,
    enabled: bool,
) -> Result<ContextSnapshot, crate::error::AppError> {
    let mut current = settings();
    if enabled
        && !(current.cookbooks.installed
            && current
                .cookbooks
                .path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_dir()))
    {
        return Err(crate::error::AppError::from(anyhow!(
            "cookbook pin is not ready"
        )));
    }
    current.cookbooks.enabled = enabled;
    current.cookbooks.phase = if enabled {
        "ready".into()
    } else if current.cookbooks.installed {
        "installed".into()
    } else {
        "off".into()
    };
    save(&current).map_err(crate::error::AppError::from)?;
    Ok(snapshot(&workspace))
}

#[tauri::command]
#[specta::specta]
pub fn context_cookbooks_uninstall(
    workspace: String,
) -> Result<ContextSnapshot, crate::error::AppError> {
    let mut current = settings();
    if let Some(path) = current.cookbooks.path.as_deref() {
        let target = PathBuf::from(path);
        let allowed = instance::state_root().join("cookbooks");
        if target.starts_with(&allowed) && target.is_dir() {
            fs::remove_dir_all(target).map_err(crate::error::AppError::from)?;
        }
    }
    current.cookbooks = CookbookContext::default();
    save(&current).map_err(crate::error::AppError::from)?;
    Ok(snapshot(&workspace))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workshop_instructions_name_overlay_and_missing_truth() {
        assert!(WORKSHOP_AGENTS.contains("missing"));
        assert!(WORKSHOP_AGENTS.contains("overlay"));
        assert!(WORKSHOP_AGENTS.contains(".env"));
        assert!(WORKSHOP_AGENTS.contains("workspace_roots_list"));
        assert!(WORKSHOP_AGENTS.contains("source_request"));
        assert!(!WORKSHOP_AGENTS.contains("request_env_import"));
        assert!(WORKSHOP_AGENTS.contains("$run-banking77-gepa"));
        assert!(WORKSHOP_AGENTS.contains("Never use the Keychain-backed"));
        assert!(WORKSHOP_AGENTS.contains("macOS Keychain"));
        assert!(WORKSHOP_AGENTS.contains("no read-denylist field"));
    }
    #[test]
    fn mcp_group_uses_declared_tool_names() {
        let groups = mcp_groups(&ContextSettings::default());
        assert_eq!(
            groups[0].enabled_tools["synth_containers"],
            vec!["container_manage"]
        );
        assert_eq!(
            groups[0].enabled_tools["synth_visuals"],
            vec!["visual_manage"]
        );
    }
}
