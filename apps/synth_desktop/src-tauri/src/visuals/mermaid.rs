//! Isolated Mermaid → SVG rendering for `diagram.mermaid.v1`.
//!
//! Canonical source is UTF-8 Mermaid. Layout is provided by the pinned,
//! vendored Grok Build pure-Rust renderer (`xai-org/grok-build@8a14c91d`).

use anyhow::{anyhow, bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const TEMPLATE_ID: &str = "diagram.mermaid.v1";
pub const HIDDEN_MODE: &str = "__render-mermaid";
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(3);
pub const RENDERER_VERSION: &str = "grok-mermaid-8a14c91d-workshop.2";
pub const MEDIA_TYPE_SOURCE: &str = "text/vnd.mermaid";
pub const MEDIA_TYPE_SVG: &str = "image/svg+xml";

const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_AXIS_PX: u32 = 16_384;
const MAX_MEGAPIXELS: u64 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
    Flowchart,
    Sequence,
    Class,
    State,
    Er,
    C4,
    Sankey,
    Gantt,
    Git,
    Mindmap,
    Pie,
    Timeline,
    Journey,
    Kanban,
    Requirement,
    Xy,
    Quadrant,
    Block,
    Packet,
    Radar,
    Info,
}

impl DiagramKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flowchart => "flowchart",
            Self::Sequence => "sequence",
            Self::Class => "class",
            Self::State => "state",
            Self::Er => "er",
            Self::C4 => "c4",
            Self::Sankey => "sankey",
            Self::Gantt => "gantt",
            Self::Git => "git",
            Self::Mindmap => "mindmap",
            Self::Pie => "pie",
            Self::Timeline => "timeline",
            Self::Journey => "journey",
            Self::Kanban => "kanban",
            Self::Requirement => "requirement",
            Self::Xy => "xychart",
            Self::Quadrant => "quadrant",
            Self::Block => "block",
            Self::Packet => "packet",
            Self::Radar => "radar",
            Self::Info => "info",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("dark") {
            Self::Dark
        } else {
            Self::Light
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderedDiagram {
    pub kind: DiagramKind,
    pub svg: String,
    pub width: u32,
    pub height: u32,
}

pub fn is_mermaid_template(template_id: &str) -> bool {
    template_id == TEMPLATE_ID
}

pub fn hidden_mode_requested() -> bool {
    std::env::args().nth(1).as_deref() == Some(HIDDEN_MODE)
}

pub fn run_hidden_mode() -> i32 {
    match run_hidden_mode_inner() {
        Ok(()) => 0,
        Err(error) => {
            crate::platform::logging::report("visuals", "eprintln", format!("mermaid render failed: {error:#}"));
            1
        }
    }
}

fn run_hidden_mode_inner() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut input = None;
    let mut output = None;
    let mut format = "svg".to_string();
    let mut theme = Theme::Light;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => {
                input = Some(PathBuf::from(
                    args.get(index + 1).context("missing --input")?,
                ));
                index += 2;
            }
            "--output" => {
                output = Some(PathBuf::from(
                    args.get(index + 1).context("missing --output")?,
                ));
                index += 2;
            }
            "--format" => {
                format = args.get(index + 1).context("missing --format")?.clone();
                index += 2;
            }
            "--theme" => {
                theme = Theme::parse(args.get(index + 1).context("missing --theme")?);
                index += 2;
            }
            "--size" => index += 2,
            other => bail!("unknown mermaid render flag: {other}"),
        }
    }
    let input = input.context("require --input")?;
    let output = output.context("require --output")?;
    assert_temp_path(&input)?;
    assert_temp_path(&output)?;
    if format != "svg" {
        bail!("Grok renderer emits svg only (requested {format})");
    }
    let source = fs::read_to_string(&input).context("read mermaid source")?;
    let rendered = render_svg(&source, theme)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, rendered.svg.as_bytes()).context("write mermaid svg")?;
    Ok(())
}

pub fn validate_source(source: &str) -> Result<DiagramKind> {
    if source.len() > MAX_SOURCE_BYTES {
        bail!(
            "mermaid source exceeds {MAX_SOURCE_BYTES} bytes (got {})",
            source.len()
        );
    }
    if source.as_bytes().contains(&0) {
        bail!("mermaid source must be UTF-8 without NUL");
    }
    if source.trim().is_empty() {
        bail!("mermaid source is empty");
    }
    detect_kind(source)
}

pub fn detect_kind(source: &str) -> Result<DiagramKind> {
    let header =
        first_significant_line(source).ok_or_else(|| anyhow!("mermaid source is empty"))?;
    let token = header
        .split_whitespace()
        .next()
        .unwrap_or(header)
        .trim_end_matches(|c: char| c == ';' || c == ':');
    let lower = token.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "flowchart" | "graph" => DiagramKind::Flowchart,
        "sequencediagram" => DiagramKind::Sequence,
        "classdiagram" => DiagramKind::Class,
        "statediagram" | "statediagram-v2" => DiagramKind::State,
        "erdiagram" => DiagramKind::Er,
        "c4context" | "c4container" | "c4component" | "c4dynamic" | "c4deployment" => {
            DiagramKind::C4
        }
        "sankey-beta" => DiagramKind::Sankey,
        "gantt" => DiagramKind::Gantt,
        "gitgraph" => DiagramKind::Git,
        "mindmap" => DiagramKind::Mindmap,
        "pie" => DiagramKind::Pie,
        "timeline" => DiagramKind::Timeline,
        "journey" => DiagramKind::Journey,
        "kanban" => DiagramKind::Kanban,
        "requirementdiagram" => DiagramKind::Requirement,
        "xychart-beta" => DiagramKind::Xy,
        "quadrantchart" => DiagramKind::Quadrant,
        "block-beta" => DiagramKind::Block,
        "packet-beta" => DiagramKind::Packet,
        "radar-beta" => DiagramKind::Radar,
        "info" => DiagramKind::Info,
        _ => bail!("unrecognized or unsupported Mermaid diagram prefix '{token}'"),
    })
}

pub fn render_svg(source: &str, theme: Theme) -> Result<RenderedDiagram> {
    let kind = validate_source(source)?;
    let engine_theme = match theme {
        Theme::Light => mermaid_to_svg::MermaidTheme {
            background: "#ffffff".into(),
            node_fill: "#f8f9fc".into(),
            node_stroke: "#6172f3".into(),
            text_color: "#182230".into(),
            edge_color: "#667085".into(),
            subgraph_fill: "#f9fafb".into(),
            subgraph_stroke: "#d0d5dd".into(),
        },
        Theme::Dark => mermaid_to_svg::MermaidTheme {
            background: "#101828".into(),
            node_fill: "#1d2939".into(),
            node_stroke: "#8098f9".into(),
            text_color: "#f9fafb".into(),
            edge_color: "#98a2b3".into(),
            subgraph_fill: "#1d2939".into(),
            subgraph_stroke: "#475467".into(),
        },
    };
    let svg = mermaid_to_svg::render_mermaid_to_svg(source, Some(&engine_theme))
        .map_err(|error| anyhow!("Grok Mermaid renderer: {error}"))?;
    validate_svg(&svg)?;
    let (width, height) = svg_dimensions(&svg);
    if svg.len() > MAX_OUTPUT_BYTES {
        bail!("mermaid svg exceeds {MAX_OUTPUT_BYTES} bytes");
    }
    if width > MAX_AXIS_PX || height > MAX_AXIS_PX {
        bail!("mermaid svg exceeds {MAX_AXIS_PX}px axis limit");
    }
    if u64::from(width) * u64::from(height) > MAX_MEGAPIXELS * 1_000_000 {
        bail!("mermaid svg exceeds {MAX_MEGAPIXELS} megapixels");
    }
    Ok(RenderedDiagram {
        kind,
        svg,
        width,
        height,
    })
}

pub fn render_isolated(source: &str, theme: Theme) -> Result<RenderedDiagram> {
    validate_source(source)?;
    if cfg!(test)
        || std::env::var("SYNTH_MERMAID_IN_PROCESS")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    {
        return render_svg(source, theme);
    }
    match spawn_render_child(source, theme) {
        Ok(rendered) => Ok(rendered),
        Err(error) if cfg!(debug_assertions) => render_svg(source, theme).context(error),
        Err(error) => Err(error),
    }
}

fn spawn_render_child(source: &str, theme: Theme) -> Result<RenderedDiagram> {
    let stamp = format!(
        "synth-mermaid-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    );
    let dir = std::env::temp_dir().join(stamp);
    fs::create_dir_all(&dir)?;
    let input = dir.join("source.mmd");
    let output = dir.join("diagram.svg");
    fs::write(&input, source.as_bytes())?;
    let exe = std::env::current_exe().context("current_exe for mermaid child")?;
    let mut command = Command::new(exe);
    command
        .arg(HIDDEN_MODE)
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--format")
        .arg("svg")
        .arg("--theme")
        .arg(theme.as_str())
        .arg("--size")
        .arg("pane")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let status = wait_child_timeout(command, RENDER_TIMEOUT);
    let svg = fs::read_to_string(&output).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    let status = status?;
    if !status.success() {
        bail!("mermaid child exited {}", status.code().unwrap_or(-1));
    }
    if svg.is_empty() {
        bail!("mermaid child wrote no svg");
    }
    validate_svg(&svg)?;
    let kind = detect_kind(source)?;
    let (width, height) = svg_dimensions(&svg);
    Ok(RenderedDiagram {
        kind,
        svg,
        width,
        height,
    })
}

fn wait_child_timeout(mut command: Command, timeout: Duration) -> Result<std::process::ExitStatus> {
    let mut child = command.spawn().context("spawn mermaid child")?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("mermaid render timed out after {}ms", timeout.as_millis());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                return Err(error.into());
            }
        }
    }
}

fn assert_temp_path(path: &Path) -> Result<()> {
    let temp = std::env::temp_dir()
        .canonicalize()
        .unwrap_or(std::env::temp_dir());
    let candidate = if path.exists() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else if let Some(parent) = path.parent() {
        parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(path.file_name().unwrap_or_default())
    } else {
        path.to_path_buf()
    };
    if !(candidate.starts_with(&temp) || path.starts_with(std::env::temp_dir())) {
        bail!("mermaid render paths must stay under the process temp directory");
    }
    Ok(())
}

fn first_significant_line(source: &str) -> Option<&str> {
    source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%") && !line.starts_with("---"))
}

fn validate_svg(svg: &str) -> Result<()> {
    let trimmed = svg.trim_start();
    if !(trimmed.starts_with("<svg") || (trimmed.starts_with("<?xml") && trimmed.contains("<svg")))
    {
        bail!("renderer produced non-svg output");
    }
    let lower = svg.to_ascii_lowercase();
    for forbidden in [
        "<script",
        "foreignobject",
        "xlink:href",
        "href=\"http",
        "href='http",
        "href=\"file:",
        "href='file:",
        "url(http",
        "@import",
    ] {
        if lower.contains(forbidden) {
            bail!("generated svg contains forbidden construct {forbidden}");
        }
    }
    Ok(())
}

fn svg_dimensions(svg: &str) -> (u32, u32) {
    if let (Some(width), Some(height)) = (attr_number(svg, "width="), attr_number(svg, "height=")) {
        return (width, height);
    }
    if let Some(start) = svg.find("viewBox=") {
        let rest = svg[start + "viewBox=".len()..].trim_start_matches(['"', '\'']);
        let end = rest.find(['"', '\'']).unwrap_or(rest.len());
        let values: Vec<f64> = rest[..end]
            .split_whitespace()
            .filter_map(|value| value.parse().ok())
            .collect();
        if values.len() == 4 {
            return (values[2].ceil() as u32, values[3].ceil() as u32);
        }
    }
    (640, 480)
}

fn attr_number(svg: &str, key: &str) -> Option<u32> {
    let start = svg.find(key)? + key.len();
    let rest = svg[start..].trim_start_matches(['"', '\'']);
    let end = rest
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(rest.len());
    rest[..end]
        .parse::<f64>()
        .ok()
        .map(|value| value.ceil() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn digest(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn rejects_oversized_source_before_layout() {
        let source = format!("flowchart TD\nA[{}]", "x".repeat(MAX_SOURCE_BYTES));
        assert!(validate_source(&source)
            .unwrap_err()
            .to_string()
            .contains("exceeds"));
    }

    #[test]
    fn rejects_unknown_family() {
        assert!(validate_source("notADiagram\nA,B,1").is_err());
    }

    #[test]
    fn renders_core_families_with_grok() {
        for source in [
            "flowchart LR\nAgent[Agent] --> MCP[MCP] --> Registry[Registry]",
            "sequenceDiagram\nparticipant A as Agent\nA->>B: policy_ref",
            "classDiagram\nclass PolicyRef {\n+harness\n+config\n}\nPolicyRef --> Rollout",
            "stateDiagram-v2\n[*] --> Prepared\nPrepared --> Running: start\nRunning --> [*]",
            "erDiagram\nPOLICY ||--o{ ROLLOUT : pins\nPOLICY {\nstring harness\n}",
            "C4Context\nPerson(agent, \"Agent\")\nSystem(desktop, \"Desktop\")\nRel(agent, desktop, \"MCP\")",
        ] {
            let rendered = render_svg(source, Theme::Light).expect(source);
            assert!(rendered.svg.contains("<svg"));
            assert!(rendered.width > 0 && rendered.height > 0);
        }
    }

    #[test]
    fn sequence_normalizes_mermaid_breaks() {
        let rendered = render_svg(
            "sequenceDiagram\nautonumber\nparticipant A as Chat\nparticipant B as Container\nA->>B: POST /rollouts<br/>policy_ref\nNote over A,B: Immutable identity",
            Theme::Light,
        )
        .unwrap();
        assert!(rendered.svg.contains("POST /rollouts"));
        assert!(rendered.svg.contains("policy_ref"));
        assert!(!rendered.svg.contains("&lt;br/&gt;"));
        assert!(rendered.svg.contains("Immutable identity"));
    }

    #[test]
    fn identical_input_is_deterministic() {
        let source = "flowchart LR\nA[Agent] --> B[MCP]";
        let a = render_svg(source, Theme::Light).unwrap();
        let b = render_svg(source, Theme::Light).unwrap();
        assert_eq!(digest(a.svg.as_bytes()), digest(b.svg.as_bytes()));
    }

    #[test]
    fn reaps_hung_child() {
        let started = Instant::now();
        let mut command = Command::new("sleep");
        command.arg("30");
        assert!(wait_child_timeout(command, Duration::from_millis(250))
            .unwrap_err()
            .to_string()
            .contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
