//! Local MCP connection, diagnostics, and host packaging. Operation semantics
//! and tool schemas come from the connected runtime, never from this CLI.

mod config;
mod transport;
mod lifecycle;
#[path = "../session/acp/config.rs"]
mod backend_config;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use transport::RuntimeClient;

pub const SKILL: &str =
    include_str!("../../../../../integrations/workshop/skills/workshop/SKILL.md");
const INSTRUCTIONS: &str = "Workshop exposes the explicitly connected local instance's product operations, hosted agents, and shared visual library. Use runtime_status for attachment state, core_events_after for durable event cursors, and agent_backends_list before starting an ACP session. Human decisions and renderer evidence cannot be supplied by agents. Inspect existing visuals and registered template contracts before creating. Use returned visual IDs. Presenting requests display; observe the rendered revision and inspect a capture before claiming visual quality. Creation is not idempotent. Shared visual operations need no hosted session or provider credentials. The connected runtime's tool catalogue is authoritative; use only advertised operations.";

const HELP: &str = "Workshop local agent integration (experimental)\n\n\
  workshop doctor --data-root PATH\n\
  workshop connect codex|claude --data-root PATH [--config-root PATH]\n\
  workshop disconnect codex|claude --data-root PATH [--config-root PATH]\n\
  workshop mcp --data-root PATH\n\
  workshop backends --data-root PATH\n\
  workshop runtime start|status|attach|detach|stop --data-root PATH\n\
  workshop plugin --data-root PATH --output PATH/workshop\n\n\
Select the data root from Workshop Settings > About > Data root.\n\
This grants access to the whole selected local instance, not a single chat.\n\
Start the runtime or Desktop. Closing a window retains the runtime.\n\
Connect verifies runtime compatibility before changing\n\
client configuration. Restart the client after configuration changes.\n\
Plugin exports bundle the same MCP connection and optional shared skill.\n\
Use either a plugin or direct registration, not both. No credentials are exported.";

struct Args {
    command: String,
    action: Option<String>,
    client: Option<String>,
    root: PathBuf,
    config_root: Option<PathBuf>,
    output: Option<PathBuf>,
}

fn parse(args: Vec<String>) -> Result<Args> {
    let mut iter = args.into_iter();
    let command = iter
        .next()
        .context("a command is required; run workshop --help")?;
    anyhow::ensure!(
        ["doctor", "connect", "disconnect", "mcp", "plugin", "runtime", "backends"].contains(&command.as_str()),
        "unknown command; run workshop --help"
    );
    let action = if command == "runtime" {
        let value = iter.next().context("choose start, status, attach, detach, or stop")?;
        anyhow::ensure!(["start", "status", "attach", "detach", "stop"].contains(&value.as_str()), "unknown runtime action");
        Some(value)
    } else { None };
    let client = if command == "connect" || command == "disconnect" {
        let value = iter.next().context("choose codex or claude")?;
        anyhow::ensure!(
            ["codex", "claude"].contains(&value.as_str()),
            "choose codex or claude"
        );
        Some(value)
    } else {
        None
    };
    let mut root = None;
    let mut config_root = None;
    let mut output = None;
    while let Some(flag) = iter.next() {
        let destination = match flag.as_str() {
            "--data-root" => &mut root,
            "--config-root" if client.is_some() => &mut config_root,
            "--output" if command == "plugin" => &mut output,
            _ => anyhow::bail!("unknown option: {flag}"),
        };
        anyhow::ensure!(destination.is_none(), "duplicate option: {flag}");
        let value = PathBuf::from(
            iter.next()
                .with_context(|| format!("{flag} requires a path"))?,
        );
        anyhow::ensure!(value.is_absolute(), "{flag} requires an absolute path");
        *destination = Some(value);
    }
    let root: PathBuf =
        root.context("--data-root is required; select an explicit Workshop instance")?;
    // Disconnect remains possible after the instance has been deleted.
    let root = root.canonicalize().unwrap_or(root);
    Ok(Args {
        command,
        action,
        client,
        root,
        config_root,
        output,
    })
}

pub fn run(args: Vec<String>) -> Result<()> {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("{HELP}");
        return Ok(());
    }
    let args = parse(args)?;
    let executable = std::env::current_exe()?.canonicalize()?;
    let client = RuntimeClient::new(args.root.clone())?;
    match args.command.as_str() {
        "backends" => {
            println!("{}", serde_json::to_string_pretty(&backend_config::read(&args.root)?)?);
        }
        "runtime" => {
            let action = args.action.as_deref().unwrap();
            let result = match action {
                "start" => lifecycle::start(&client, &executable, &args.root)?,
                "status" => client.call("runtime_status", &json!({}))?,
                other => client.call("runtime_control", &json!({"action": other}))?,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "doctor" => {
            let description = client.describe()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "connected":true, "runtimeVersion": description["runtimeVersion"],
                "dataRoot": description["dataRoot"], "workspaceId": description["workspaceId"], "scope": description["scope"],
                    "tools": description["tools"].as_array().unwrap().iter().map(|tool| tool["name"].clone()).collect::<Vec<_>>(),
                    "features": description["features"], "renderingVerified": false
                }))?
            );
        }
        "connect" | "disconnect" => {
            let connecting = args.command == "connect";
            if connecting {
                client.describe()?;
            }
            let host = args.client.as_deref().unwrap();
            let config_root = args
                .config_root
                .unwrap_or_else(|| default_config_root(host));
            let path = config::configure(host, &config_root, &executable, &args.root, connecting)?;
            println!(
                "{} Workshop in {}. Restart {host} to apply. Scope: local instance {}.",
                if connecting { "Registered" } else { "Removed" },
                path.display(),
                args.root.display()
            );
        }
        "plugin" => {
            client.describe()?;
            let output = args.output.context("--output is required")?;
            export_plugin(&output, &executable, &args.root)?;
            println!("Exported {}. Install it through your client's plugin manager. Do not also register Workshop manually.", output.display());
        }
        "mcp" => {
            let description = client.describe()?;
            let tools = json!({"tools": description["tools"]});
            crate::mcp_stdio::run_stdio_server_with_instructions(
                crate::mcp_stdio::McpServerInfo {
                    name: "workshop",
                    version: env!("CARGO_PKG_VERSION"),
                },
                || tools.clone(),
                |name, arguments| {
                    client
                        .call(name, arguments)
                        .map_err(|error| error.to_string())
                },
                |_, arguments| client.breaker_arguments(arguments),
                INSTRUCTIONS,
            );
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn default_config_root(host: &str) -> PathBuf {
    if host == "codex" {
        std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
    } else {
        // Claude user MCP configuration lives in ~/.claude.json, not ~/.claude/.
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
    }
}

pub fn server_config(executable: &Path, root: &Path) -> Value {
    json!({"command": executable, "args": ["mcp", "--data-root", root]})
}

fn export_plugin(output: &Path, executable: &Path, root: &Path) -> Result<()> {
    anyhow::ensure!(
        output.file_name().and_then(|value| value.to_str()) == Some("workshop"),
        "plugin directory must be named workshop"
    );
    anyhow::ensure!(
        !output.exists(),
        "output already exists; export to a new directory"
    );
    let parent = output
        .parent()
        .context("output requires a parent directory")?;
    std::fs::create_dir_all(parent)?;
    let stage = parent.join(format!(".workshop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        for dir in [".codex-plugin", ".claude-plugin", "skills/workshop"] {
            std::fs::create_dir_all(stage.join(dir))?;
        }
        let common = json!({"name":"workshop", "version": env!("CARGO_PKG_VERSION"),
            "author":{"name":"Synth Laboratories"},
            "description":"Use a connected local Workshop instance and its visual panel."});
        let mut codex = common.clone();
        codex["interface"] = json!({
            "displayName":"Workshop", "shortDescription":"Work with local Workshop visuals",
            "longDescription":"Inspect, create, update, and present visuals in a connected local Workshop instance.",
            "developerName":"Synth Laboratories", "category":"Productivity", "capabilities":["Interactive","Write"],
            "defaultPrompt":["Use Workshop to create a diagram, present it, and inspect a native capture."]
        });
        codex["skills"] = json!("./skills/");
        codex["mcpServers"] = json!("./.mcp.json");
        std::fs::write(
            stage.join(".codex-plugin/plugin.json"),
            serde_json::to_vec_pretty(&codex)?,
        )?;
        std::fs::write(
            stage.join(".claude-plugin/plugin.json"),
            serde_json::to_vec_pretty(&common)?,
        )?;
        std::fs::write(
            stage.join(".mcp.json"),
            serde_json::to_vec_pretty(
                &json!({"mcpServers":{"workshop": server_config(executable, root)}}),
            )?,
        )?;
        std::fs::write(stage.join("skills/workshop/SKILL.md"), SKILL)?;
        std::fs::rename(&stage, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}

#[cfg(test)]
mod tests;
