use std::borrow::Cow;
mod ast;
mod block_diagram;
mod c4_diagram;
mod class_diagram;
pub mod config;
mod er_diagram;
mod error;
mod gantt_diagram;
mod gitgraph_diagram;
mod info_diagram;
mod journey_diagram;
mod kanban_diagram;
mod layout;
mod mermaid_port;
mod mindmap_diagram;
mod packet_diagram;
mod parser;
mod pie_diagram;
mod quadrant_diagram;
mod radar_diagram;
mod requirement_diagram;
mod sankey_diagram;
mod sequence_diagram;
mod state_diagram;
mod svg_renderer;
mod text_wrap;
mod theme;
mod timeline_diagram;
mod xychart_diagram;

pub use config::{parse_mermaid_frontmatter, FlowchartConfig, ParsedMermaidSource, RenderConfig};
pub use error::MermaidError;
pub use theme::{MermaidTheme, MermaidThemePreset, MermaidThemeVariables};

pub fn render_mermaid_to_svg(
    mermaid_source: &str,
    theme: Option<&MermaidTheme>,
) -> Result<String, MermaidError> {
    let parsed_source = parse_mermaid_frontmatter(mermaid_source);
    let default_theme = MermaidTheme::default();
    let configured_theme = parsed_source.config.to_mermaid_theme();
    let theme = match theme {
        Some(theme) => theme,
        None => configured_theme.as_ref().unwrap_or(&default_theme),
    };
    let mermaid_source = parsed_source.body.as_ref();

    let diagram_type = first_diagram_type_token(mermaid_source);

    if diagram_type == Some("erDiagram") {
        return er_diagram::render_er_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("classDiagram") {
        return class_diagram::render_class_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("mindmap") {
        return mindmap_diagram::render_mindmap_to_svg(mermaid_source, theme);
    }

    if matches!(diagram_type, Some("stateDiagram") | Some("stateDiagram-v2")) {
        let graph = state_diagram::parse_state_diagram(mermaid_source)?;
        let layout_result = layout::compute_layout(&graph);
        let svg = svg_renderer::render(&layout_result, theme);
        return Ok(svg);
    }

    if diagram_type == Some("pie") {
        return pie_diagram::render_pie_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("gantt") {
        return gantt_diagram::render_gantt_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("requirementDiagram") {
        return requirement_diagram::render_requirement_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("info") {
        return info_diagram::render_info_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("packet-beta") {
        return packet_diagram::render_packet_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("block-beta") {
        return block_diagram::render_block_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("radar-beta") {
        return radar_diagram::render_radar_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("sankey-beta") {
        return sankey_diagram::render_sankey_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("sequenceDiagram") {
        return sequence_diagram::render_sequence_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("gitGraph") {
        return gitgraph_diagram::render_gitgraph_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("timeline") {
        return timeline_diagram::render_timeline_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("journey") {
        return journey_diagram::render_journey_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("kanban") {
        return kanban_diagram::render_kanban_diagram_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("quadrantChart") {
        return quadrant_diagram::render_quadrant_chart_to_svg(mermaid_source, theme);
    }

    if diagram_type == Some("xychart-beta") {
        return xychart_diagram::render_xychart_diagram_to_svg(mermaid_source, theme);
    }

    if matches!(
        diagram_type,
        Some("C4Context")
            | Some("C4Container")
            | Some("C4Component")
            | Some("C4Dynamic")
            | Some("C4Deployment")
    ) {
        return c4_diagram::render_c4_diagram_to_svg(mermaid_source, theme);
    }

    let is_flowchart = matches!(diagram_type, Some("graph") | Some("flowchart"));
    if is_flowchart && mermaid_port::is_enabled() {
        return mermaid_port::render_mermaid_to_svg_ported(
            mermaid_source,
            theme,
            &parsed_source.config,
        );
    }

    let graph = parser::parse_mermaid(mermaid_source)?;
    let layout_result = if is_flowchart {
        layout::compute_layout_with_config(&graph, &parsed_source.config)
    } else {
        layout::compute_layout(&graph)
    };
    let svg = if is_flowchart {
        svg_renderer::render_with_config(&layout_result, theme, &parsed_source.config)
    } else {
        svg_renderer::render(&layout_result, theme)
    };

    Ok(svg)
}

/// Strip a leading Mermaid YAML frontmatter block delimited by `---` lines.
///
/// Mermaid supports frontmatter at the start of a diagram for per-diagram
/// metadata and configuration. Use `parse_mermaid_frontmatter` to access parsed
/// metadata and config values.
pub fn strip_mermaid_frontmatter(source: &str) -> Cow<'_, str> {
    parse_mermaid_frontmatter(source).body
}

fn first_diagram_type_token(input: &str) -> Option<&str> {
    input
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .and_then(|l| l.split_whitespace().next())
}

pub fn is_mermaid_diagram(lang: &str) -> bool {
    let lang_lower = lang.to_lowercase();
    lang_lower == "mermaid" || lang_lower.starts_with("mermaid ")
}

