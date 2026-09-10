//! Ad-hoc agent-authored data charts: bounded JSON spec -> deterministic SVG.
//!
//! Same contract as `systems.rs`: the canonical source is the spec, the
//! rendition is produced here in-process, and the pane displays that exact
//! rendition. One implementation means an agent capture and a human pane are
//! the same pixels, and review never needs a live window.
//!
//! Absence is not zero. Every value channel is `Option<f64>`; a `null` opens a
//! gap in a line, omits a bar, and hatches a heatmap cell. Nothing is imputed.

use super::chart_data::{
    BarsFrom, HeatmapFrom, HistogramFrom, MetricsFrom, ScatterFrom, SeriesFrom, TableFrom,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const TEMPLATE_ID: &str = "analysis.chart.v1";
pub const MAX_SOURCE_BYTES: usize = 512 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub const RENDERER_VERSION: &str = "workshop-charts-svg.1";
pub const MEDIA_TYPE_SOURCE: &str = "application/vnd.synth.chart-spec+json";
pub const MEDIA_TYPE_SVG: &str = "image/svg+xml";
pub const SCHEMA_VERSION: &str = "synth.visual.chart-spec.v1";

const MAX_PANELS: usize = 16;
const MAX_SERIES: usize = 12;
const MAX_POINTS: usize = 20_000;
const MAX_CATEGORIES: usize = 200;
const MAX_TABLE_ROWS: usize = 400;
const MAX_TABLE_COLUMNS: usize = 12;
const MAX_HEATMAP_CELLS: usize = 4_000;
const MAX_LABEL_CHARS: usize = 240;

const MIN_WIDTH: f64 = 480.0;
const MAX_WIDTH: f64 = 2_000.0;
const MAX_HEIGHT: f64 = 16_384.0;

pub fn is_chart_template(id: &str) -> bool {
    id == TEMPLATE_ID
}

// ── Spec ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartSpec {
    pub version: u8,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_width")]
    pub width: f64,
    pub panels: Vec<Panel>,
}

fn default_theme() -> String {
    "light".into()
}
fn default_width() -> f64 {
    960.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Panel {
    Metrics(MetricsPanel),
    Series(SeriesPanel),
    Bars(BarsPanel),
    Scatter(ScatterPanel),
    Histogram(HistogramPanel),
    Heatmap(HeatmapPanel),
    Table(TablePanel),
    Note(NotePanel),
}

impl Panel {
    fn head(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::Metrics(p) => (p.title.as_deref(), p.subtitle.as_deref()),
            Self::Series(p) => (Some(p.title.as_str()), p.subtitle.as_deref()),
            Self::Bars(p) => (Some(p.title.as_str()), p.subtitle.as_deref()),
            Self::Scatter(p) => (Some(p.title.as_str()), p.subtitle.as_deref()),
            Self::Histogram(p) => (Some(p.title.as_str()), p.subtitle.as_deref()),
            Self::Heatmap(p) => (Some(p.title.as_str()), p.subtitle.as_deref()),
            Self::Table(p) => (Some(p.title.as_str()), p.subtitle.as_deref()),
            Self::Note(p) => (p.title.as_deref(), None),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricsPanel {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub items: Vec<MetricItem>,
    #[serde(default)]
    pub from: Option<MetricsFrom>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricItem {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub tone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Axis {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
}

impl Default for Axis {
    fn default() -> Self {
        Self {
            label: None,
            min: None,
            max: None,
            unit: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesPanel {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub x: Axis,
    #[serde(default)]
    pub y: Axis,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default)]
    pub from: Option<SeriesFrom>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Series {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_series_style")]
    pub style: String,
    #[serde(default)]
    pub points: Vec<Point>,
    /// Optional uncertainty envelope drawn behind the line.
    #[serde(default)]
    pub band: Vec<Band>,
}

fn default_series_style() -> String {
    "line".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    /// `null` is an explicit gap, never zero.
    #[serde(default)]
    pub y: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Band {
    pub x: f64,
    pub lo: f64,
    pub hi: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BarsPanel {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub series: Vec<BarSeries>,
    #[serde(default)]
    pub from: Option<BarsFrom>,
    #[serde(default)]
    pub stacked: bool,
    #[serde(default = "default_orientation")]
    pub orientation: String,
    #[serde(default)]
    pub y: Axis,
    #[serde(default)]
    pub height: Option<f64>,
}

fn default_orientation() -> String {
    "vertical".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BarSeries {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    /// One entry per category; `null` means the category was not measured.
    pub values: Vec<Option<f64>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScatterPanel {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub x: Axis,
    #[serde(default)]
    pub y: Axis,
    #[serde(default)]
    pub points: Vec<ScatterPoint>,
    #[serde(default)]
    pub from: Option<ScatterFrom>,
    #[serde(default)]
    pub frontier: Option<Frontier>,
    #[serde(default)]
    pub height: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScatterPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frontier {
    /// "max" keeps the highest x among ties; "min" keeps the lowest (cost axes).
    #[serde(default = "default_prefers_max")]
    pub x_prefers: String,
    #[serde(default = "default_prefers_max")]
    pub y_prefers: String,
}

fn default_prefers_max() -> String {
    "max".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistogramPanel {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub values: Vec<f64>,
    #[serde(default)]
    pub from: Option<HistogramFrom>,
    #[serde(default = "default_bins")]
    pub bins: u32,
    #[serde(default)]
    pub x: Axis,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub height: Option<f64>,
}

fn default_bins() -> u32 {
    20
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeatmapPanel {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub rows: Vec<String>,
    #[serde(default)]
    pub columns: Vec<String>,
    /// `values[row][column]`; `null` renders as an unreached cell.
    #[serde(default)]
    pub values: Vec<Vec<Option<f64>>>,
    #[serde(default)]
    pub from: Option<HeatmapFrom>,
    #[serde(default)]
    pub scale: Option<Axis>,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TablePanel {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Vec<Cell>>,
    #[serde(default)]
    pub from: Option<TableFrom>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Cell {
    Null,
    Number(f64),
    Text(String),
}

impl Cell {
    fn display(&self) -> String {
        match self {
            Self::Null => "—".into(),
            Self::Number(value) => n(*value),
            Self::Text(value) => value.clone(),
        }
    }
    fn numeric(&self) -> bool {
        matches!(self, Self::Number(_) | Self::Null)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotePanel {
    #[serde(default)]
    pub title: Option<String>,
    pub body: String,
    #[serde(default)]
    pub tone: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RenderedChart {
    pub svg: String,
    pub width: u32,
    pub height: u32,
}

// ── Parse + validate ────────────────────────────────────────────────────────

pub fn parse_and_validate(source: &str) -> Result<ChartSpec> {
    if source.len() > MAX_SOURCE_BYTES {
        bail!("chart spec exceeds {MAX_SOURCE_BYTES} bytes");
    }
    if source.as_bytes().contains(&0) {
        bail!("chart spec must not contain NUL");
    }
    let spec: ChartSpec =
        serde_json::from_str(source).context("chart spec must be valid bounded JSON")?;
    validate_spec(&spec)?;
    Ok(spec)
}

pub fn validate_source(source: &str) -> Result<()> {
    parse_and_validate(source).map(|_| ())
}

/// The same bounds, applied to a spec that has already been resolved. A
/// derivation can produce a shape no author typed, so it is checked too.
pub fn validate_spec(spec: &ChartSpec) -> Result<()> {
    if spec.version != 1 {
        bail!("unsupported chart spec version {}", spec.version);
    }
    if !matches!(spec.theme.as_str(), "light" | "dark") {
        bail!("theme must be light or dark");
    }
    if !spec.width.is_finite() || spec.width < MIN_WIDTH || spec.width > MAX_WIDTH {
        bail!("width must be between {MIN_WIDTH} and {MAX_WIDTH}");
    }
    if spec.panels.is_empty() {
        bail!("chart spec needs at least one panel");
    }
    if spec.panels.len() > MAX_PANELS {
        bail!("chart spec exceeds {MAX_PANELS} panels");
    }
    safe_text_opt(spec.title.as_deref())?;
    safe_text_opt(spec.subtitle.as_deref())?;
    for panel in &spec.panels {
        validate_panel(panel)?;
    }
    Ok(())
}

fn validate_panel(panel: &Panel) -> Result<()> {
    let (title, subtitle) = panel.head();
    safe_text_opt(title)?;
    safe_text_opt(subtitle)?;
    match panel {
        Panel::Metrics(p) => {
            declares_one_source(p.from.is_some(), !p.items.is_empty(), "metrics", "items")?;
            if p.items.len() > 12 {
                bail!(
                    "metrics panel has {} items, above 12; add a limit transform",
                    p.items.len()
                );
            }
            for item in &p.items {
                safe_text(&item.label)?;
                safe_text(&item.value)?;
                safe_text_opt(item.detail.as_deref())?;
                valid_tone(item.tone.as_deref())?;
            }
        }
        Panel::Series(p) => {
            declares_one_source(p.from.is_some(), !p.series.is_empty(), "series", "series")?;
            if p.series.len() > MAX_SERIES {
                bail!(
                    "series panel derived {} series, above {MAX_SERIES}; filter the split column",
                    p.series.len()
                );
            }
            let mut points = 0usize;
            for series in &p.series {
                safe_text(&series.name)?;
                valid_color(series.color.as_deref())?;
                if !matches!(series.style.as_str(), "line" | "stepped" | "area") {
                    bail!("series style must be line, stepped, or area");
                }
                points += series.points.len() + series.band.len();
                for point in &series.points {
                    finite("series x", point.x)?;
                    if let Some(y) = point.y {
                        finite("series y", y)?;
                    }
                }
                for band in &series.band {
                    finite("band x", band.x)?;
                    finite("band lo", band.lo)?;
                    finite("band hi", band.hi)?;
                    if band.lo > band.hi {
                        bail!("band lo must not exceed hi");
                    }
                }
            }
            if points > MAX_POINTS {
                bail!("series panel exceeds {MAX_POINTS} points");
            }
            validate_axis(&p.x, "x")?;
            validate_axis(&p.y, "y")?;
            validate_height(p.height)?;
        }
        Panel::Bars(p) => {
            declares_one_source(
                p.from.is_some(),
                !p.categories.is_empty() || !p.series.is_empty(),
                "bars",
                "categories and series",
            )?;
            if p.categories.len() > MAX_CATEGORIES {
                bail!(
                    "bars panel has {} categories, above {MAX_CATEGORIES}; aggregate or limit first",
                    p.categories.len()
                );
            }
            if p.series.len() > MAX_SERIES {
                bail!(
                    "bars panel derived {} series, above {MAX_SERIES}; filter the split column",
                    p.series.len()
                );
            }
            if !matches!(p.orientation.as_str(), "vertical" | "horizontal") {
                bail!("orientation must be vertical or horizontal");
            }
            for category in &p.categories {
                safe_text(category)?;
            }
            for series in &p.series {
                safe_text(&series.name)?;
                valid_color(series.color.as_deref())?;
                if series.values.len() != p.categories.len() {
                    bail!(
                        "bar series {} has {} values for {} categories",
                        series.name,
                        series.values.len(),
                        p.categories.len()
                    );
                }
                for value in series.values.iter().flatten() {
                    finite("bar value", *value)?;
                }
            }
            if p.stacked
                && p.series
                    .iter()
                    .flat_map(|s| &s.values)
                    .flatten()
                    .any(|v| *v < 0.0)
            {
                bail!("stacked bars must not mix negative values");
            }
            validate_axis(&p.y, "y")?;
            validate_height(p.height)?;
        }
        Panel::Scatter(p) => {
            declares_one_source(p.from.is_some(), !p.points.is_empty(), "scatter", "points")?;
            if p.points.len() > MAX_POINTS {
                bail!("scatter panel exceeds {MAX_POINTS} points");
            }
            for point in &p.points {
                finite("scatter x", point.x)?;
                finite("scatter y", point.y)?;
                safe_text_opt(point.label.as_deref())?;
                safe_text_opt(point.group.as_deref())?;
                valid_color(point.color.as_deref())?;
            }
            if let Some(frontier) = &p.frontier {
                for value in [&frontier.x_prefers, &frontier.y_prefers] {
                    if !matches!(value.as_str(), "min" | "max") {
                        bail!("frontier preference must be min or max");
                    }
                }
            }
            validate_axis(&p.x, "x")?;
            validate_axis(&p.y, "y")?;
            validate_height(p.height)?;
        }
        Panel::Histogram(p) => {
            declares_one_source(
                p.from.is_some(),
                !p.values.is_empty(),
                "histogram",
                "values",
            )?;
            if p.values.len() > MAX_POINTS {
                bail!("histogram exceeds {MAX_POINTS} values");
            }
            if p.bins < 2 || p.bins > 200 {
                bail!("histogram bins must be 2..200");
            }
            for value in &p.values {
                finite("histogram value", *value)?;
            }
            valid_color(p.color.as_deref())?;
            validate_axis(&p.x, "x")?;
            validate_height(p.height)?;
        }
        Panel::Heatmap(p) => {
            declares_one_source(
                p.from.is_some(),
                !p.rows.is_empty() || !p.columns.is_empty(),
                "heatmap",
                "rows and columns",
            )?;
            if p.rows.len() * p.columns.len() > MAX_HEATMAP_CELLS {
                bail!("heatmap exceeds {MAX_HEATMAP_CELLS} cells");
            }
            if p.from.is_none() && p.values.len() != p.rows.len() {
                bail!(
                    "heatmap has {} value rows for {} row labels",
                    p.values.len(),
                    p.rows.len()
                );
            }
            for (index, row) in p.values.iter().enumerate() {
                if row.len() != p.columns.len() {
                    bail!(
                        "heatmap row {index} has {} values for {} columns",
                        row.len(),
                        p.columns.len()
                    );
                }
                for value in row.iter().flatten() {
                    finite("heatmap value", *value)?;
                }
            }
            for label in p.rows.iter().chain(p.columns.iter()) {
                safe_text(label)?;
            }
            if let Some(scale) = &p.scale {
                validate_axis(scale, "scale")?;
            }
        }
        Panel::Table(p) => {
            declares_one_source(p.from.is_some(), !p.columns.is_empty(), "table", "columns")?;
            if p.columns.len() > MAX_TABLE_COLUMNS {
                bail!(
                    "table has {} columns, above {MAX_TABLE_COLUMNS}",
                    p.columns.len()
                );
            }
            if p.rows.len() > MAX_TABLE_ROWS {
                bail!("table exceeds {MAX_TABLE_ROWS} rows");
            }
            for column in &p.columns {
                safe_text(column)?;
            }
            for (index, row) in p.rows.iter().enumerate() {
                if row.len() != p.columns.len() {
                    bail!(
                        "table row {index} has {} cells for {} columns",
                        row.len(),
                        p.columns.len()
                    );
                }
                for cell in row {
                    match cell {
                        Cell::Text(value) => safe_text(value)?,
                        Cell::Number(value) => finite("table cell", *value)?,
                        Cell::Null => {}
                    }
                }
            }
        }
        Panel::Note(p) => {
            safe_text(&p.body)?;
            if p.body.len() > 2_000 {
                bail!("note body exceeds 2000 bytes");
            }
            valid_tone(p.tone.as_deref())?;
        }
    }
    Ok(())
}

/// A panel takes its values from literals or from a `from` block, never both
/// and never neither. Both would leave two answers to "what is plotted"; the
/// author would not be able to tell which one the image came from.
fn declares_one_source(
    has_from: bool,
    has_literals: bool,
    panel: &str,
    literal_field: &str,
) -> Result<()> {
    match (has_from, has_literals) {
        (true, true) => {
            bail!("{panel} panel declares both `from` and literal {literal_field}; keep one")
        }
        (false, false) => {
            bail!("{panel} panel has no {literal_field} and no `from` block naming a bound slot")
        }
        _ => Ok(()),
    }
}

fn validate_axis(axis: &Axis, name: &str) -> Result<()> {
    safe_text_opt(axis.label.as_deref())?;
    safe_text_opt(axis.unit.as_deref())?;
    for value in [axis.min, axis.max].into_iter().flatten() {
        finite(name, value)?;
    }
    if let (Some(min), Some(max)) = (axis.min, axis.max) {
        if min >= max {
            bail!("{name} axis min must be below max");
        }
    }
    Ok(())
}

fn validate_height(height: Option<f64>) -> Result<()> {
    if let Some(value) = height {
        if !value.is_finite() || !(120.0..=1_200.0).contains(&value) {
            bail!("panel height must be 120..1200");
        }
    }
    Ok(())
}

fn finite(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() {
        bail!("{name} must be finite");
    }
    Ok(())
}

fn safe_text(value: &str) -> Result<()> {
    if value.chars().count() > MAX_LABEL_CHARS {
        bail!("text exceeds {MAX_LABEL_CHARS} characters");
    }
    if value.chars().any(|c| c.is_control() && c != '\n') {
        bail!("text must not contain control characters");
    }
    Ok(())
}

fn safe_text_opt(value: Option<&str>) -> Result<()> {
    value.map(safe_text).unwrap_or(Ok(()))
}

fn valid_tone(tone: Option<&str>) -> Result<()> {
    match tone {
        None | Some("neutral") | Some("ok") | Some("caution") | Some("bad") => Ok(()),
        Some(other) => bail!("unknown tone {other}"),
    }
}

/// Only literal hex colors: a spec is agent-authored, and `url(...)` or
/// `expression(...)` in a paint attribute is a script vector.
fn valid_color(color: Option<&str>) -> Result<()> {
    let Some(value) = color else { return Ok(()) };
    let hex = value.strip_prefix('#').unwrap_or("");
    if !(hex.len() == 6 || hex.len() == 3) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("color must be a #rgb or #rrggbb literal");
    }
    Ok(())
}

/// Schema-valid but commonly illegible compositions. Feedback for the author,
/// not a parse failure — the same visual can be revised and re-reviewed.
pub fn authoring_findings(source: &str) -> Result<Vec<String>> {
    Ok(authoring_findings_for(&parse_and_validate(source)?))
}

/// The same diagnostics, applied to a spec already in hand — the resolved one,
/// where a derivation's real width is finally known.
pub fn authoring_findings_for(spec: &ChartSpec) -> Vec<String> {
    let mut findings = Vec::new();
    for panel in &spec.panels {
        match panel {
            Panel::Series(p) => {
                if p.series.len() > 6 {
                    findings.push(format!(
                        "{}: {} series in one panel; above six the legend outruns the eye",
                        p.title,
                        p.series.len()
                    ));
                }
                if p.series
                    .iter()
                    .all(|s| s.points.iter().all(|q| q.y.is_none()))
                {
                    findings.push(format!(
                        "{}: every point is null; nothing will plot",
                        p.title
                    ));
                }
            }
            Panel::Bars(p) => {
                if p.orientation == "vertical" && p.categories.len() > 18 {
                    findings.push(format!(
                        "{}: {} vertical categories collide; use orientation \"horizontal\"",
                        p.title,
                        p.categories.len()
                    ));
                }
            }
            Panel::Scatter(p) => {
                let labelled = p.points.iter().filter(|q| q.label.is_some()).count();
                if labelled > 24 {
                    findings.push(format!(
                        "{}: {labelled} point labels overlap at pane width; label the frontier only",
                        p.title
                    ));
                }
            }
            Panel::Heatmap(p) => {
                if p.columns.len() > 40 {
                    findings.push(format!(
                        "{}: {} columns render below one label width",
                        p.title,
                        p.columns.len()
                    ));
                }
            }
            Panel::Table(p) => {
                if p.rows.len() > 40 {
                    findings.push(format!(
                        "{}: {} rows exceed a reviewable pane; aggregate or rank first",
                        p.title,
                        p.rows.len()
                    ));
                }
            }
            Panel::Metrics(_) | Panel::Histogram(_) | Panel::Note(_) => {}
        }
    }
    findings
}

// ── Theme ───────────────────────────────────────────────────────────────────

struct Palette {
    bg: &'static str,
    surface: &'static str,
    border: &'static str,
    grid: &'static str,
    text: &'static str,
    muted: &'static str,
    faint: &'static str,
    accent: &'static str,
    absent: &'static str,
    series: [&'static str; 8],
}

/// Values are the visual-chrome tokens (`visuals/chrome/tokens.css`); the dark
/// set mirrors the systems-map technical dark so the two families agree.
fn palette(theme: &str) -> Palette {
    if theme == "dark" {
        Palette {
            bg: "#0D0F13",
            surface: "#14171C",
            border: "#262A31",
            grid: "#20242B",
            text: "#F4F4F5",
            muted: "#A1A1AA",
            faint: "#7C818B",
            accent: "#F05F22",
            absent: "#2A2F37",
            series: [
                "#F05F22", "#9AA3B0", "#3FB27F", "#D8A030", "#E06A62", "#5B8FD1", "#9A7BD1",
                "#3FA9A0",
            ],
        }
    } else {
        Palette {
            bg: "#FFFFFF",
            surface: "#F6F7F9",
            border: "#E8EAEE",
            grid: "#EEF0F3",
            text: "#1A1D23",
            muted: "#5C6573",
            faint: "#687180",
            accent: "#B94712",
            absent: "#E4E7EC",
            series: [
                "#B94712", "#5C6573", "#1E7A43", "#92660C", "#B23830", "#3F6EA8", "#6B4E9B",
                "#0F766E",
            ],
        }
    }
}

fn tone_color<'a>(tone: Option<&str>, palette: &'a Palette) -> &'a str {
    match tone {
        Some("ok") => palette.series[2],
        Some("caution") => palette.series[3],
        Some("bad") => palette.series[4],
        _ => palette.text,
    }
}

// ── Layout constants ────────────────────────────────────────────────────────

const MARGIN: f64 = 26.0;
const PANEL_GAP: f64 = 20.0;
const PANEL_PAD: f64 = 14.0;
const PLOT_LEFT: f64 = 58.0;
const PLOT_RIGHT: f64 = 16.0;
const PLOT_BOTTOM: f64 = 34.0;
const LEGEND_H: f64 = 22.0;
const MAX_BAR_WIDTH: f64 = 46.0;

const FS_TITLE: f64 = 18.0;
const FS_PANEL: f64 = 13.0;
const FS_BODY: f64 = 12.0;
const FS_META: f64 = 11.0;
const FS_MICRO: f64 = 10.0;

fn text_width(value: &str, size: f64) -> f64 {
    value.chars().count() as f64 * size * 0.56
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let head: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{head}…")
}

fn wrap(value: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in value.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if candidate.chars().count() > max_chars && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            } else {
                current = candidate;
            }
        }
        lines.push(current);
    }
    lines
}

fn panel_head_height(panel: &Panel) -> f64 {
    let (title, subtitle) = panel.head();
    let mut height = 0.0;
    if title.is_some() {
        height += 20.0;
    }
    if subtitle.is_some() {
        height += 15.0;
    }
    if height > 0.0 {
        height += 6.0;
    }
    height
}

fn panel_body_height(panel: &Panel, width: f64) -> f64 {
    match panel {
        Panel::Metrics(p) => {
            if p.items.is_empty() {
                return 60.0;
            }
            let columns = metric_columns(p.items.len());
            let rows = (p.items.len() + columns - 1) / columns;
            rows as f64 * 60.0 + (rows.saturating_sub(1)) as f64 * 8.0
        }
        Panel::Series(p) => {
            let legend = if p.series.len() > 1 { LEGEND_H } else { 0.0 };
            p.height.unwrap_or(250.0) + legend
        }
        Panel::Bars(p) => {
            if p.categories.is_empty() {
                return 60.0;
            }
            let legend = if p.series.len() > 1 { LEGEND_H } else { 0.0 };
            let base = if p.orientation == "horizontal" {
                (p.categories.len() as f64 * bar_row_height(p) + PLOT_BOTTOM).max(160.0)
            } else {
                250.0
            };
            p.height.unwrap_or(base) + legend
        }
        Panel::Scatter(p) => p.height.unwrap_or(290.0),
        Panel::Histogram(p) => p.height.unwrap_or(230.0),
        Panel::Heatmap(p) => {
            if p.rows.is_empty() || p.columns.is_empty() {
                return 60.0;
            }
            let cell = heatmap_cell(p, width);
            heatmap_column_band(p) + p.rows.len() as f64 * cell.1 + 10.0
        }
        Panel::Table(p) => {
            if p.columns.is_empty() {
                return 60.0;
            }
            30.0 + p.rows.len() as f64 * 24.0 + 6.0
        }
        Panel::Note(p) => {
            let max_chars = ((width - 2.0 * PANEL_PAD) / (FS_BODY * 0.56)).max(20.0) as usize;
            wrap(&p.body, max_chars).len() as f64 * 17.0 + 8.0
        }
    }
}

fn metric_columns(count: usize) -> usize {
    match count {
        1 => 1,
        2 => 2,
        3 => 3,
        4 | 7 | 8 => 4,
        5 | 6 => 3,
        _ => 4,
    }
}

fn bar_row_height(panel: &BarsPanel) -> f64 {
    let per_series = if panel.stacked { 1 } else { panel.series.len() };
    (per_series as f64 * 13.0 + 10.0).max(22.0)
}

fn heatmap_cell(panel: &HeatmapPanel, width: f64) -> (f64, f64) {
    let gutter = heatmap_gutter(panel);
    let available = (width - 2.0 * PANEL_PAD - gutter - 46.0).max(80.0);
    let cell_w = (available / panel.columns.len() as f64).clamp(8.0, 96.0);
    let cell_h = cell_w.clamp(14.0, 30.0);
    (cell_w, cell_h)
}

fn heatmap_gutter(panel: &HeatmapPanel) -> f64 {
    let longest = panel
        .rows
        .iter()
        .map(|label| text_width(&truncate(label, 22), FS_META))
        .fold(0.0_f64, f64::max);
    (longest + 12.0).clamp(48.0, 190.0)
}

fn heatmap_column_band(panel: &HeatmapPanel) -> f64 {
    let longest = panel
        .columns
        .iter()
        .map(|label| text_width(&truncate(label, 18), FS_MICRO))
        .fold(0.0_f64, f64::max);
    (longest * 0.72 + 14.0).clamp(26.0, 110.0)
}

// ── Scales ──────────────────────────────────────────────────────────────────

/// Deterministic 1/2/5 tick selection; returns the padded domain and its ticks.
fn nice_ticks(lo: f64, hi: f64, target: usize, axis: &Axis) -> (f64, f64, Vec<f64>) {
    let mut lo = axis.min.unwrap_or(lo);
    let mut hi = axis.max.unwrap_or(hi);
    if !lo.is_finite() || !hi.is_finite() {
        lo = 0.0;
        hi = 1.0;
    }
    if (hi - lo).abs() < f64::EPSILON {
        let pad = if lo.abs() < f64::EPSILON {
            1.0
        } else {
            lo.abs() * 0.1
        };
        lo -= pad;
        hi += pad;
    }
    let rough = (hi - lo) / target.max(1) as f64;
    let magnitude = 10f64.powf(rough.abs().log10().floor());
    let normalized = rough / magnitude;
    let step = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    } * magnitude;
    let start = if axis.min.is_some() {
        lo
    } else {
        (lo / step).floor() * step
    };
    let end = if axis.max.is_some() {
        hi
    } else {
        (hi / step).ceil() * step
    };
    let mut ticks = Vec::new();
    let count = (((end - start) / step).round() as i64).clamp(1, 24);
    for index in 0..=count {
        ticks.push(start + step * index as f64);
    }
    (start, end, ticks)
}

fn scale(value: f64, lo: f64, hi: f64, out_lo: f64, out_hi: f64) -> f64 {
    if (hi - lo).abs() < f64::EPSILON {
        return (out_lo + out_hi) / 2.0;
    }
    out_lo + (value - lo) / (hi - lo) * (out_hi - out_lo)
}

fn format_tick(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 100_000.0 {
        format!("{:.0}k", value / 1000.0)
    } else if magnitude >= 1000.0 {
        let thousands = value / 1000.0;
        if (thousands.fract()).abs() < 0.05 {
            format!("{thousands:.0}k")
        } else {
            format!("{thousands:.1}k")
        }
    } else if value.fract().abs() < 1e-9 {
        n(value)
    } else if magnitude >= 1.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

// ── Render ──────────────────────────────────────────────────────────────────

/// Render a literal spec. Every `from` block must already be resolved — the
/// renderer has no way to reach evidence and must not pretend a panel is empty
/// when the truth is that nobody resolved it.
pub fn render_svg(source: &str) -> Result<RenderedChart> {
    render_spec(&parse_and_validate(source)?)
}

pub fn render_spec(spec: &ChartSpec) -> Result<RenderedChart> {
    if let Some(slot) = unresolved_slot(spec) {
        bail!("panel still reads input {slot}; resolve bindings before rendering");
    }
    let spec = spec.clone();
    let palette = palette(&spec.theme);
    let width = spec.width;
    let inner = width - 2.0 * MARGIN;

    let mut header = 0.0;
    if spec.title.is_some() {
        header += 26.0;
    }
    if spec.subtitle.is_some() {
        header += 18.0;
    }
    if header > 0.0 {
        header += 10.0;
    }

    let bodies: Vec<f64> = spec
        .panels
        .iter()
        .map(|panel| panel_body_height(panel, inner))
        .collect();
    let heights: Vec<f64> = spec
        .panels
        .iter()
        .zip(&bodies)
        .map(|(panel, body)| panel_head_height(panel) + body + 2.0 * PANEL_PAD)
        .collect();
    let height = MARGIN * 2.0
        + header
        + heights.iter().sum::<f64>()
        + PANEL_GAP * (heights.len().saturating_sub(1)) as f64;
    if height > MAX_HEIGHT {
        bail!("chart renders {height}px tall, above the {MAX_HEIGHT}px ceiling");
    }

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-labelledby=\"title desc\"><title id=\"title\">{title}</title><desc id=\"desc\">{desc}</desc><defs><pattern id=\"absent\" width=\"6\" height=\"6\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(45)\"><rect width=\"6\" height=\"6\" fill=\"{absent}\"/><line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"6\" stroke=\"{border}\" stroke-width=\"2\"/></pattern></defs><rect width=\"100%\" height=\"100%\" fill=\"{bg}\"/><g font-family=\"'IBM Plex Sans',system-ui,-apple-system,'Segoe UI',sans-serif\">",
        w = n(width),
        h = n(height),
        title = escape(spec.title.as_deref().unwrap_or("Chart")),
        desc = escape(&describe(&spec)),
        absent = palette.absent,
        border = palette.border,
        bg = palette.bg,
    );

    let mut y = MARGIN;
    if let Some(title) = &spec.title {
        y += 20.0;
        out.push_str(&text_at(
            MARGIN,
            y,
            &truncate(title, 96),
            "start",
            palette.text,
            FS_TITLE,
            600,
        ));
        y += 6.0;
    }
    if let Some(subtitle) = &spec.subtitle {
        y += 13.0;
        out.push_str(&text_at(
            MARGIN,
            y,
            &truncate(subtitle, 140),
            "start",
            palette.muted,
            FS_BODY,
            400,
        ));
        y += 5.0;
    }
    if header > 0.0 {
        y = MARGIN + header;
    }

    for ((panel, body_height), card_height) in spec.panels.iter().zip(&bodies).zip(&heights) {
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"10\" fill=\"{}\" stroke=\"{}\"/>",
            n(MARGIN),
            n(y),
            n(inner),
            n(*card_height),
            palette.surface,
            palette.border
        ));
        let mut cursor = y + PANEL_PAD;
        let (title, subtitle) = panel.head();
        if let Some(title) = title {
            cursor += 14.0;
            out.push_str(&text_at(
                MARGIN + PANEL_PAD,
                cursor,
                &truncate(title, 90),
                "start",
                palette.text,
                FS_PANEL,
                600,
            ));
            cursor += 6.0;
        }
        if let Some(subtitle) = subtitle {
            cursor += 11.0;
            out.push_str(&text_at(
                MARGIN + PANEL_PAD,
                cursor,
                &truncate(subtitle, 130),
                "start",
                palette.faint,
                FS_META,
                400,
            ));
            cursor += 4.0;
        }
        let rect = Rect {
            x: MARGIN + PANEL_PAD,
            y: cursor,
            w: inner - 2.0 * PANEL_PAD,
            h: *body_height,
        };
        render_panel(&mut out, panel, rect, &palette);
        y += card_height + PANEL_GAP;
    }

    out.push_str("</g></svg>");
    if out.len() > MAX_OUTPUT_BYTES {
        bail!("chart SVG exceeds {MAX_OUTPUT_BYTES} bytes");
    }
    Ok(RenderedChart {
        svg: out,
        width: width.round() as u32,
        height: height.round() as u32,
    })
}

fn unresolved_slot(spec: &ChartSpec) -> Option<String> {
    spec.panels.iter().find_map(|panel| match panel {
        Panel::Metrics(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Series(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Bars(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Scatter(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Histogram(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Heatmap(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Table(p) => p.from.as_ref().map(|from| from.source.input.clone()),
        Panel::Note(_) => None,
    })
}

fn describe(spec: &ChartSpec) -> String {
    let kinds: Vec<&str> = spec
        .panels
        .iter()
        .map(|panel| match panel {
            Panel::Metrics(_) => "metrics",
            Panel::Series(_) => "series",
            Panel::Bars(_) => "bars",
            Panel::Scatter(_) => "scatter",
            Panel::Histogram(_) => "histogram",
            Panel::Heatmap(_) => "heatmap",
            Panel::Table(_) => "table",
            Panel::Note(_) => "note",
        })
        .collect();
    kinds.join(", ")
}

#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn render_panel(out: &mut String, panel: &Panel, rect: Rect, palette: &Palette) {
    match panel {
        Panel::Metrics(p) => render_metrics(out, p, rect, palette),
        Panel::Series(p) => render_series(out, p, rect, palette),
        Panel::Bars(p) => render_bars(out, p, rect, palette),
        Panel::Scatter(p) => render_scatter(out, p, rect, palette),
        Panel::Histogram(p) => render_histogram(out, p, rect, palette),
        Panel::Heatmap(p) => render_heatmap(out, p, rect, palette),
        Panel::Table(p) => render_table(out, p, rect, palette),
        Panel::Note(p) => render_note(out, p, rect, palette),
    }
}

fn series_color<'a>(index: usize, explicit: Option<&'a str>, palette: &'a Palette) -> &'a str {
    explicit.unwrap_or(palette.series[index % palette.series.len()])
}

fn legend(out: &mut String, entries: &[(String, String)], rect: Rect, palette: &Palette) {
    let mut x = rect.x;
    let baseline = rect.y + 12.0;
    for (name, color) in entries {
        let label = truncate(name, 28);
        let advance = text_width(&label, FS_META) + 26.0;
        if x + advance > rect.x + rect.w {
            break;
        }
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"9\" height=\"9\" rx=\"2\" fill=\"{}\"/>",
            n(x),
            n(baseline - 8.0),
            color
        ));
        out.push_str(&text_at(
            x + 14.0,
            baseline,
            &label,
            "start",
            palette.muted,
            FS_META,
            400,
        ));
        x += advance;
    }
}

fn empty_state(out: &mut String, rect: Rect, palette: &Palette, message: &str) {
    out.push_str(&text_at(
        rect.x + rect.w / 2.0,
        rect.y + rect.h / 2.0,
        message,
        "middle",
        palette.faint,
        FS_META,
        400,
    ));
}

// ── Panels ──────────────────────────────────────────────────────────────────

fn render_metrics(out: &mut String, panel: &MetricsPanel, rect: Rect, palette: &Palette) {
    if panel.items.is_empty() {
        empty_state(out, rect, palette, "no metrics resolved");
        return;
    }
    let columns = metric_columns(panel.items.len());
    let gap = 10.0;
    let cell_w = (rect.w - gap * (columns.saturating_sub(1)) as f64) / columns as f64;
    for (index, item) in panel.items.iter().enumerate() {
        let column = index % columns;
        let row = index / columns;
        let x = rect.x + column as f64 * (cell_w + gap);
        let y = rect.y + row as f64 * 68.0;
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"60\" rx=\"8\" fill=\"{}\" stroke=\"{}\"/>",
            n(x),
            n(y),
            n(cell_w),
            palette.bg,
            palette.border
        ));
        let chars = (cell_w / (FS_META * 0.56)) as usize;
        out.push_str(&text_at(
            x + 11.0,
            y + 18.0,
            &truncate(&item.label, chars.saturating_sub(2)),
            "start",
            palette.faint,
            FS_META,
            400,
        ));
        out.push_str(&text_at(
            x + 11.0,
            y + 39.0,
            &truncate(&item.value, ((cell_w / (17.0 * 0.6)) as usize).max(4)),
            "start",
            tone_color(item.tone.as_deref(), palette),
            17.0,
            600,
        ));
        if let Some(detail) = &item.detail {
            out.push_str(&text_at(
                x + 11.0,
                y + 52.0,
                &truncate(detail, chars.saturating_sub(2)),
                "start",
                palette.faint,
                FS_MICRO,
                400,
            ));
        }
    }
}

/// Grid, ticks, and axis labels shared by every cartesian panel.
#[allow(clippy::too_many_arguments)]
fn cartesian_frame(
    out: &mut String,
    plot: Rect,
    x_domain: (f64, f64),
    x_ticks: &[f64],
    y_domain: (f64, f64),
    y_ticks: &[f64],
    x_axis: &Axis,
    y_axis: &Axis,
    palette: &Palette,
    x_labels: Option<&[String]>,
) {
    for tick in y_ticks {
        if *tick < y_domain.0 - f64::EPSILON || *tick > y_domain.1 + f64::EPSILON {
            continue;
        }
        let y = scale(*tick, y_domain.0, y_domain.1, plot.y + plot.h, plot.y);
        out.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\"/>",
            n(plot.x),
            n(y),
            n(plot.x + plot.w),
            n(y),
            palette.grid
        ));
        out.push_str(&text_at(
            plot.x - 8.0,
            y + 3.5,
            &format_tick(*tick),
            "end",
            palette.faint,
            FS_MICRO,
            400,
        ));
    }
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\"/>",
        n(plot.x),
        n(plot.y + plot.h),
        n(plot.x + plot.w),
        n(plot.y + plot.h),
        palette.border
    ));
    match x_labels {
        Some(labels) => {
            let step = (labels.len() as f64 * 62.0 / plot.w).ceil().max(1.0) as usize;
            for (index, label) in labels.iter().enumerate() {
                if index % step != 0 {
                    continue;
                }
                let x = scale(
                    index as f64 + 0.5,
                    0.0,
                    labels.len() as f64,
                    plot.x,
                    plot.x + plot.w,
                );
                out.push_str(&text_at(
                    x,
                    plot.y + plot.h + 15.0,
                    &truncate(label, 14),
                    "middle",
                    palette.faint,
                    FS_MICRO,
                    400,
                ));
            }
        }
        None => {
            for tick in x_ticks {
                if *tick < x_domain.0 - f64::EPSILON || *tick > x_domain.1 + f64::EPSILON {
                    continue;
                }
                let x = scale(*tick, x_domain.0, x_domain.1, plot.x, plot.x + plot.w);
                out.push_str(&text_at(
                    x,
                    plot.y + plot.h + 15.0,
                    &format_tick(*tick),
                    "middle",
                    palette.faint,
                    FS_MICRO,
                    400,
                ));
            }
        }
    }
    if let Some(label) = axis_caption(x_axis) {
        out.push_str(&text_at(
            plot.x + plot.w / 2.0,
            plot.y + plot.h + 30.0,
            &truncate(&label, 60),
            "middle",
            palette.muted,
            FS_META,
            400,
        ));
    }
    if let Some(label) = axis_caption(y_axis) {
        let cx = plot.x - 44.0;
        let cy = plot.y + plot.h / 2.0;
        out.push_str(&format!(
            "<g transform=\"rotate(-90 {} {})\">{}</g>",
            n(cx),
            n(cy),
            text_at(
                cx,
                cy,
                &truncate(&label, 40),
                "middle",
                palette.muted,
                FS_META,
                400
            )
        ));
    }
}

fn axis_caption(axis: &Axis) -> Option<String> {
    let label = axis.label.as_deref()?;
    Some(match axis.unit.as_deref() {
        Some(unit) => format!("{label} ({unit})"),
        None => label.to_string(),
    })
}

fn render_series(out: &mut String, panel: &SeriesPanel, rect: Rect, palette: &Palette) {
    let mut body = rect;
    if panel.series.len() > 1 {
        let entries: Vec<(String, String)> = panel
            .series
            .iter()
            .enumerate()
            .map(|(index, series)| {
                (
                    series.name.clone(),
                    series_color(index, series.color.as_deref(), palette).to_string(),
                )
            })
            .collect();
        legend(out, &entries, body, palette);
        body.y += LEGEND_H;
        body.h -= LEGEND_H;
    }
    let plot = Rect {
        x: body.x + PLOT_LEFT,
        y: body.y + 6.0,
        w: (body.w - PLOT_LEFT - PLOT_RIGHT).max(40.0),
        h: (body.h - 6.0 - PLOT_BOTTOM).max(40.0),
    };
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for series in &panel.series {
        for point in &series.points {
            if let Some(y) = point.y {
                xs.push(point.x);
                ys.push(y);
            }
        }
        for band in &series.band {
            xs.push(band.x);
            ys.push(band.lo);
            ys.push(band.hi);
        }
    }
    if xs.is_empty() {
        empty_state(out, body, palette, "no plotted values");
        return;
    }
    let (x_lo, x_hi, x_ticks) = nice_ticks(min_of(&xs), max_of(&xs), 6, &panel.x);
    let (y_lo, y_hi, y_ticks) = nice_ticks(min_of(&ys), max_of(&ys), 5, &panel.y);
    cartesian_frame(
        out,
        plot,
        (x_lo, x_hi),
        &x_ticks,
        (y_lo, y_hi),
        &y_ticks,
        &panel.x,
        &panel.y,
        palette,
        None,
    );
    for (index, series) in panel.series.iter().enumerate() {
        let color = series_color(index, series.color.as_deref(), palette);
        if !series.band.is_empty() {
            let mut band = series.band.clone();
            band.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            let mut path = String::new();
            for (position, item) in band.iter().enumerate() {
                let x = scale(item.x, x_lo, x_hi, plot.x, plot.x + plot.w);
                let y = scale(item.hi, y_lo, y_hi, plot.y + plot.h, plot.y);
                path.push_str(&format!(
                    "{}{} {}",
                    if position == 0 { "M" } else { "L" },
                    n(x),
                    n(y)
                ));
                path.push(' ');
            }
            for item in band.iter().rev() {
                let x = scale(item.x, x_lo, x_hi, plot.x, plot.x + plot.w);
                let y = scale(item.lo, y_lo, y_hi, plot.y + plot.h, plot.y);
                path.push_str(&format!("L{} {} ", n(x), n(y)));
            }
            out.push_str(&format!(
                "<path d=\"{path}Z\" fill=\"{color}\" fill-opacity=\"0.14\" stroke=\"none\"/>"
            ));
        }
        let mut sorted = series.points.clone();
        sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        let mut path = String::new();
        let mut open = false;
        let mut previous: Option<(f64, f64)> = None;
        for point in &sorted {
            let Some(value) = point.y else {
                open = false;
                previous = None;
                continue;
            };
            let x = scale(point.x, x_lo, x_hi, plot.x, plot.x + plot.w);
            let y = scale(value, y_lo, y_hi, plot.y + plot.h, plot.y);
            if !open {
                path.push_str(&format!("M{} {} ", n(x), n(y)));
                open = true;
            } else if series.style == "stepped" {
                let (_, prev_y) = previous.unwrap_or((x, y));
                path.push_str(&format!("L{} {} L{} {} ", n(x), n(prev_y), n(x), n(y)));
            } else {
                path.push_str(&format!("L{} {} ", n(x), n(y)));
            }
            previous = Some((x, y));
        }
        if series.style == "area" && !path.is_empty() {
            let baseline = scale(y_lo.max(0.0), y_lo, y_hi, plot.y + plot.h, plot.y);
            let first = sorted.iter().find(|p| p.y.is_some());
            let last = sorted.iter().rev().find(|p| p.y.is_some());
            if let (Some(first), Some(last)) = (first, last) {
                let x0 = scale(first.x, x_lo, x_hi, plot.x, plot.x + plot.w);
                let x1 = scale(last.x, x_lo, x_hi, plot.x, plot.x + plot.w);
                out.push_str(&format!(
                    "<path d=\"{path}L{} {} L{} {} Z\" fill=\"{color}\" fill-opacity=\"0.16\" stroke=\"none\"/>",
                    n(x1),
                    n(baseline),
                    n(x0),
                    n(baseline)
                ));
            }
        }
        out.push_str(&format!(
            "<path d=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.8\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>"
        ));
        if sorted.iter().filter(|p| p.y.is_some()).count() <= 40 {
            for point in &sorted {
                let Some(value) = point.y else { continue };
                let x = scale(point.x, x_lo, x_hi, plot.x, plot.x + plot.w);
                let y = scale(value, y_lo, y_hi, plot.y + plot.h, plot.y);
                out.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"2.6\" fill=\"{color}\"/>",
                    n(x),
                    n(y)
                ));
            }
        }
    }
}

fn render_bars(out: &mut String, panel: &BarsPanel, rect: Rect, palette: &Palette) {
    if panel.categories.is_empty() {
        empty_state(out, rect, palette, "no categories resolved");
        return;
    }
    let mut body = rect;
    if panel.series.len() > 1 {
        let entries: Vec<(String, String)> = panel
            .series
            .iter()
            .enumerate()
            .map(|(index, series)| {
                (
                    series.name.clone(),
                    series_color(index, series.color.as_deref(), palette).to_string(),
                )
            })
            .collect();
        legend(out, &entries, body, palette);
        body.y += LEGEND_H;
        body.h -= LEGEND_H;
    }
    let horizontal = panel.orientation == "horizontal";
    let gutter = if horizontal {
        panel
            .categories
            .iter()
            .map(|label| text_width(&truncate(label, 24), FS_META))
            .fold(0.0_f64, f64::max)
            .clamp(60.0, 220.0)
            + 10.0
    } else {
        PLOT_LEFT
    };
    let plot = Rect {
        x: body.x + gutter,
        y: body.y + 6.0,
        w: (body.w - gutter - PLOT_RIGHT).max(40.0),
        h: (body.h - 6.0 - PLOT_BOTTOM).max(40.0),
    };
    let mut totals: Vec<f64> = Vec::new();
    for index in 0..panel.categories.len() {
        if panel.stacked {
            let total: f64 = panel
                .series
                .iter()
                .filter_map(|series| series.values.get(index).copied().flatten())
                .sum();
            totals.push(total);
        } else {
            for series in &panel.series {
                if let Some(Some(value)) = series.values.get(index) {
                    totals.push(*value);
                }
            }
        }
    }
    if totals.is_empty() {
        empty_state(out, body, palette, "no measured categories");
        return;
    }
    let (v_lo, v_hi, v_ticks) = nice_ticks(min_of(&totals).min(0.0), max_of(&totals), 5, &panel.y);
    if horizontal {
        let row = plot.h / panel.categories.len() as f64;
        for tick in &v_ticks {
            if *tick < v_lo || *tick > v_hi {
                continue;
            }
            let x = scale(*tick, v_lo, v_hi, plot.x, plot.x + plot.w);
            out.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\"/>",
                n(x),
                n(plot.y),
                n(x),
                n(plot.y + plot.h),
                palette.grid
            ));
            out.push_str(&text_at(
                x,
                plot.y + plot.h + 15.0,
                &format_tick(*tick),
                "middle",
                palette.faint,
                FS_MICRO,
                400,
            ));
        }
        for (index, category) in panel.categories.iter().enumerate() {
            let top = plot.y + index as f64 * row;
            out.push_str(&text_at(
                plot.x - 8.0,
                top + row / 2.0 + 3.5,
                &truncate(category, 24),
                "end",
                palette.muted,
                FS_META,
                400,
            ));
            let lanes = if panel.stacked { 1 } else { panel.series.len() };
            // Same cap as the vertical case: a bar thicker than the eye needs
            // reads as a block. Centre the group in its row instead.
            let lane = ((row - 6.0) / lanes as f64).clamp(3.0, MAX_BAR_WIDTH);
            let group_top = top + (row - lane * lanes as f64) / 2.0;
            let mut cursor = scale(0.0_f64.max(v_lo), v_lo, v_hi, plot.x, plot.x + plot.w);
            for (order, series) in panel.series.iter().enumerate() {
                let color = series_color(order, series.color.as_deref(), palette);
                let Some(Some(value)) = series.values.get(index) else {
                    if !panel.stacked {
                        let y = group_top + order as f64 * lane;
                        out.push_str(&format!(
                            "<rect x=\"{}\" y=\"{}\" width=\"9\" height=\"{}\" fill=\"url(#absent)\" stroke=\"{}\" stroke-width=\"0.75\"/>",
                            n(cursor),
                            n(y),
                            n(lane - 2.0),
                            palette.border
                        ));
                    }
                    continue;
                };
                let end = scale(*value, v_lo, v_hi, plot.x, plot.x + plot.w);
                let (x0, x1) = if panel.stacked {
                    let start = cursor;
                    cursor += end - scale(0.0_f64.max(v_lo), v_lo, v_hi, plot.x, plot.x + plot.w);
                    (start.min(cursor), start.max(cursor))
                } else {
                    let zero = scale(0.0_f64.max(v_lo), v_lo, v_hi, plot.x, plot.x + plot.w);
                    (zero.min(end), zero.max(end))
                };
                let y = if panel.stacked {
                    group_top
                } else {
                    group_top + order as f64 * lane
                };
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{color}\"/>",
                    n(x0),
                    n(y),
                    n((x1 - x0).max(1.0)),
                    n((lane - 2.0).max(3.0))
                ));
            }
        }
        if let Some(label) = axis_caption(&panel.y) {
            out.push_str(&text_at(
                plot.x + plot.w / 2.0,
                plot.y + plot.h + 30.0,
                &truncate(&label, 60),
                "middle",
                palette.muted,
                FS_META,
                400,
            ));
        }
        return;
    }
    cartesian_frame(
        out,
        plot,
        (0.0, panel.categories.len() as f64),
        &[],
        (v_lo, v_hi),
        &v_ticks,
        &Axis::default(),
        &panel.y,
        palette,
        Some(&panel.categories),
    );
    let column = plot.w / panel.categories.len() as f64;
    let zero = scale(0.0_f64.max(v_lo), v_lo, v_hi, plot.y + plot.h, plot.y);
    let lanes = if panel.stacked { 1 } else { panel.series.len() };
    // A bar wider than the eye needs reads as a block, not a measurement: cap
    // the lane and centre the group in its slot rather than filling it.
    let lane = ((column - 8.0) / lanes as f64).clamp(2.0, MAX_BAR_WIDTH);
    let group = lane * lanes as f64;
    for index in 0..panel.categories.len() {
        let left = plot.x + index as f64 * column + (column - group) / 2.0;
        let mut cursor = zero;
        for (order, series) in panel.series.iter().enumerate() {
            let color = series_color(order, series.color.as_deref(), palette);
            let x = if panel.stacked {
                left
            } else {
                left + order as f64 * lane
            };
            let Some(Some(value)) = series.values.get(index) else {
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"9\" fill=\"url(#absent)\" stroke=\"{}\" stroke-width=\"0.75\"/>",
                    n(x),
                    n(zero - 9.0),
                    n((lane - 2.0).max(3.0)),
                    palette.border
                ));
                continue;
            };
            let end = scale(*value, v_lo, v_hi, plot.y + plot.h, plot.y);
            let (y0, y1) = if panel.stacked {
                let start = cursor;
                cursor -= zero - end;
                (start.min(cursor), start.max(cursor))
            } else {
                (zero.min(end), zero.max(end))
            };
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{color}\"/>",
                n(x),
                n(y0),
                n((lane - 2.0).max(2.0)),
                n((y1 - y0).max(1.0))
            ));
        }
    }
}

fn render_scatter(out: &mut String, panel: &ScatterPanel, rect: Rect, palette: &Palette) {
    let plot = Rect {
        x: rect.x + PLOT_LEFT,
        y: rect.y + 6.0,
        w: (rect.w - PLOT_LEFT - PLOT_RIGHT).max(40.0),
        h: (rect.h - 6.0 - PLOT_BOTTOM).max(40.0),
    };
    if panel.points.is_empty() {
        empty_state(out, rect, palette, "no points resolved");
        return;
    }
    let xs: Vec<f64> = panel.points.iter().map(|point| point.x).collect();
    let ys: Vec<f64> = panel.points.iter().map(|point| point.y).collect();
    let (x_lo, x_hi, x_ticks) = nice_ticks(min_of(&xs), max_of(&xs), 6, &panel.x);
    let (y_lo, y_hi, y_ticks) = nice_ticks(min_of(&ys), max_of(&ys), 5, &panel.y);
    cartesian_frame(
        out,
        plot,
        (x_lo, x_hi),
        &x_ticks,
        (y_lo, y_hi),
        &y_ticks,
        &panel.x,
        &panel.y,
        palette,
        None,
    );
    if let Some(frontier) = &panel.frontier {
        let set = frontier_points(&panel.points, frontier);
        if set.len() > 1 {
            let mut path = String::new();
            for (position, (x, y)) in set.iter().enumerate() {
                let px = scale(*x, x_lo, x_hi, plot.x, plot.x + plot.w);
                let py = scale(*y, y_lo, y_hi, plot.y + plot.h, plot.y);
                if position == 0 {
                    path.push_str(&format!("M{} {} ", n(px), n(py)));
                } else {
                    path.push_str(&format!("L{} {} ", n(px), n(py)));
                }
            }
            out.push_str(&format!(
                "<path d=\"{path}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.4\" stroke-dasharray=\"5 4\"/>",
                palette.accent
            ));
        }
    }
    let mut groups: Vec<&str> = Vec::new();
    for point in &panel.points {
        if let Some(group) = point.group.as_deref() {
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
    }
    let label_all = panel.points.iter().filter(|p| p.label.is_some()).count() <= 24;
    for point in &panel.points {
        let color = match (point.color.as_deref(), point.group.as_deref()) {
            (Some(color), _) => color.to_string(),
            (None, Some(group)) => {
                let index = groups.iter().position(|item| *item == group).unwrap_or(0);
                palette.series[index % palette.series.len()].to_string()
            }
            (None, None) => palette.series[0].to_string(),
        };
        let x = scale(point.x, x_lo, x_hi, plot.x, plot.x + plot.w);
        let y = scale(point.y, y_lo, y_hi, plot.y + plot.h, plot.y);
        out.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"5\" fill=\"{color}\" fill-opacity=\"0.85\" stroke=\"{}\" stroke-width=\"1.2\"/>",
            n(x),
            n(y),
            palette.bg
        ));
        if label_all {
            if let Some(label) = &point.label {
                out.push_str(&text_at(
                    x,
                    y - 10.0,
                    &truncate(label, 22),
                    "middle",
                    palette.muted,
                    FS_MICRO,
                    400,
                ));
            }
        }
    }
    if !groups.is_empty() {
        let entries: Vec<(String, String)> = groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                (
                    (*group).to_string(),
                    palette.series[index % palette.series.len()].to_string(),
                )
            })
            .collect();
        legend(
            out,
            &entries,
            Rect {
                x: plot.x,
                y: rect.y + rect.h - 16.0,
                w: plot.w,
                h: 16.0,
            },
            palette,
        );
    }
}

fn frontier_points(points: &[ScatterPoint], frontier: &Frontier) -> Vec<(f64, f64)> {
    let x_max = frontier.x_prefers == "max";
    let y_max = frontier.y_prefers == "max";
    let dominates = |a: &ScatterPoint, b: &ScatterPoint| {
        let x_better = if x_max { a.x >= b.x } else { a.x <= b.x };
        let y_better = if y_max { a.y >= b.y } else { a.y <= b.y };
        let strict =
            if x_max { a.x > b.x } else { a.x < b.x } || if y_max { a.y > b.y } else { a.y < b.y };
        x_better && y_better && strict
    };
    let mut set: Vec<(f64, f64)> = points
        .iter()
        .filter(|candidate| !points.iter().any(|other| dominates(other, candidate)))
        .map(|point| (point.x, point.y))
        .collect();
    set.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    set.dedup();
    set
}

fn render_histogram(out: &mut String, panel: &HistogramPanel, rect: Rect, palette: &Palette) {
    let plot = Rect {
        x: rect.x + PLOT_LEFT,
        y: rect.y + 6.0,
        w: (rect.w - PLOT_LEFT - PLOT_RIGHT).max(40.0),
        h: (rect.h - 6.0 - PLOT_BOTTOM).max(40.0),
    };
    let lo = panel.x.min.unwrap_or_else(|| min_of(&panel.values));
    let hi = panel.x.max.unwrap_or_else(|| max_of(&panel.values));
    let (lo, hi) = if (hi - lo).abs() < f64::EPSILON {
        (lo - 0.5, hi + 0.5)
    } else {
        (lo, hi)
    };
    let bins = panel.bins as usize;
    let width = (hi - lo) / bins as f64;
    let mut counts = vec![0u64; bins];
    for value in &panel.values {
        if *value < lo || *value > hi {
            continue;
        }
        let index = (((value - lo) / width).floor() as usize).min(bins - 1);
        counts[index] += 1;
    }
    let peak = counts.iter().copied().max().unwrap_or(0);
    if peak == 0 {
        empty_state(out, rect, palette, "no values inside the axis range");
        return;
    }
    let counts_f: Vec<f64> = counts.iter().map(|value| *value as f64).collect();
    let (c_lo, c_hi, c_ticks) = nice_ticks(0.0, max_of(&counts_f), 4, &Axis::default());
    let x_axis = panel.x.clone();
    let (x_lo, x_hi, x_ticks) = nice_ticks(lo, hi, 6, &x_axis);
    cartesian_frame(
        out,
        plot,
        (x_lo, x_hi),
        &x_ticks,
        (c_lo, c_hi),
        &c_ticks,
        &x_axis,
        &Axis {
            label: Some("count".into()),
            ..Axis::default()
        },
        palette,
        None,
    );
    let color = panel.color.as_deref().unwrap_or(palette.series[0]);
    for (index, count) in counts.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        let left = scale(
            lo + index as f64 * width,
            x_lo,
            x_hi,
            plot.x,
            plot.x + plot.w,
        );
        let right = scale(
            lo + (index + 1) as f64 * width,
            x_lo,
            x_hi,
            plot.x,
            plot.x + plot.w,
        );
        let top = scale(*count as f64, c_lo, c_hi, plot.y + plot.h, plot.y);
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{color}\" fill-opacity=\"0.9\"/>",
            n(left + 0.5),
            n(top),
            n((right - left - 1.0).max(1.0)),
            n(plot.y + plot.h - top)
        ));
    }
}

fn render_heatmap(out: &mut String, panel: &HeatmapPanel, rect: Rect, palette: &Palette) {
    if panel.rows.is_empty() || panel.columns.is_empty() {
        empty_state(out, rect, palette, "no cells resolved");
        return;
    }
    let gutter = heatmap_gutter(panel);
    let band = heatmap_column_band(panel);
    let (cell_w, cell_h) = heatmap_cell(panel, rect.w + 2.0 * PANEL_PAD);
    let grid_w = gutter + panel.columns.len() as f64 * cell_w + 46.0;
    let origin_x = rect.x + gutter + ((rect.w - grid_w) / 2.0).max(0.0);
    let origin_y = rect.y + band;
    let values: Vec<f64> = panel.values.iter().flatten().flatten().copied().collect();
    if values.is_empty() {
        empty_state(out, rect, palette, "every cell is unreached");
        return;
    }
    let lo = panel
        .scale
        .as_ref()
        .and_then(|axis| axis.min)
        .unwrap_or_else(|| min_of(&values));
    let hi = panel
        .scale
        .as_ref()
        .and_then(|axis| axis.max)
        .unwrap_or_else(|| max_of(&values));
    for (column, label) in panel.columns.iter().enumerate() {
        let x = origin_x + column as f64 * cell_w + cell_w / 2.0;
        let y = origin_y - 6.0;
        out.push_str(&format!(
            "<g transform=\"rotate(-52 {} {})\">{}</g>",
            n(x),
            n(y),
            text_at(
                x,
                y,
                &truncate(label, 18),
                "start",
                palette.faint,
                FS_MICRO,
                400
            )
        ));
    }
    for (row, label) in panel.rows.iter().enumerate() {
        let y = origin_y + row as f64 * cell_h;
        out.push_str(&text_at(
            origin_x - 8.0,
            y + cell_h / 2.0 + 3.0,
            &truncate(label, 22),
            "end",
            palette.muted,
            FS_META,
            400,
        ));
        for (column, _) in panel.columns.iter().enumerate() {
            let x = origin_x + column as f64 * cell_w;
            let value = panel.values[row][column];
            let fill = match value {
                Some(value) => mix(palette.bg, palette.accent, ramp_t(value, lo, hi)),
                None => "url(#absent)".to_string(),
            };
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"0.5\"/>",
                n(x + 0.5),
                n(y + 0.5),
                n(cell_w - 1.0),
                n(cell_h - 1.0),
                palette.border
            ));
        }
    }
    let legend_x = origin_x + panel.columns.len() as f64 * cell_w + 12.0;
    let legend_h = (panel.rows.len() as f64 * cell_h).min(120.0);
    if legend_x + 34.0 < rect.x + rect.w {
        for step in 0..24 {
            let t = step as f64 / 23.0;
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"10\" height=\"{}\" fill=\"{}\"/>",
                n(legend_x),
                n(origin_y + legend_h - (t + 1.0 / 23.0) * legend_h),
                n(legend_h / 23.0 + 0.6),
                mix(palette.bg, palette.accent, t)
            ));
        }
        let caption = |value: f64| match panel.unit.as_deref() {
            Some(unit) => format!("{}{unit}", format_tick(value)),
            None => format_tick(value),
        };
        out.push_str(&text_at(
            legend_x + 14.0,
            origin_y + 8.0,
            &caption(hi),
            "start",
            palette.faint,
            FS_MICRO,
            400,
        ));
        out.push_str(&text_at(
            legend_x + 14.0,
            origin_y + legend_h,
            &caption(lo),
            "start",
            palette.faint,
            FS_MICRO,
            400,
        ));
    }
}

fn ramp_t(value: f64, lo: f64, hi: f64) -> f64 {
    if (hi - lo).abs() < f64::EPSILON {
        return 1.0;
    }
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn render_table(out: &mut String, panel: &TablePanel, rect: Rect, palette: &Palette) {
    if panel.columns.is_empty() {
        empty_state(out, rect, palette, "no columns resolved");
        return;
    }
    let columns = panel.columns.len();
    let column_w = rect.w / columns as f64;
    let numeric: Vec<bool> = (0..columns)
        .map(|index| {
            !panel.rows.is_empty()
                && panel
                    .rows
                    .iter()
                    .all(|row| row.get(index).map(Cell::numeric).unwrap_or(false))
        })
        .collect();
    for (index, column) in panel.columns.iter().enumerate() {
        let right = numeric[index];
        let x = if right {
            rect.x + (index as f64 + 1.0) * column_w - 8.0
        } else {
            rect.x + index as f64 * column_w + 2.0
        };
        out.push_str(&text_at(
            x,
            rect.y + 14.0,
            &truncate(column, (column_w / (FS_META * 0.56)) as usize),
            if right { "end" } else { "start" },
            palette.faint,
            FS_META,
            500,
        ));
    }
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\"/>",
        n(rect.x),
        n(rect.y + 22.0),
        n(rect.x + rect.w),
        n(rect.y + 22.0),
        palette.border
    ));
    for (index, row) in panel.rows.iter().enumerate() {
        let y = rect.y + 30.0 + index as f64 * 24.0 + 10.0;
        if index > 0 {
            out.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\"/>",
                n(rect.x),
                n(y - 17.0),
                n(rect.x + rect.w),
                n(y - 17.0),
                palette.grid
            ));
        }
        for (column, cell) in row.iter().enumerate() {
            let right = numeric[column];
            let x = if right {
                rect.x + (column as f64 + 1.0) * column_w - 8.0
            } else {
                rect.x + column as f64 * column_w + 2.0
            };
            let color = if matches!(cell, Cell::Null) {
                palette.faint
            } else {
                palette.text
            };
            out.push_str(&text_at(
                x,
                y,
                &truncate(&cell.display(), (column_w / (FS_BODY * 0.56)) as usize),
                if right { "end" } else { "start" },
                color,
                FS_BODY,
                400,
            ));
        }
    }
}

fn render_note(out: &mut String, panel: &NotePanel, rect: Rect, palette: &Palette) {
    let color = tone_color(panel.tone.as_deref(), palette);
    out.push_str(&format!(
        "<rect x=\"{}\" y=\"{}\" width=\"3\" height=\"{}\" rx=\"2\" fill=\"{color}\"/>",
        n(rect.x),
        n(rect.y),
        n(rect.h)
    ));
    let max_chars = ((rect.w - 14.0) / (FS_BODY * 0.56)).max(20.0) as usize;
    for (index, line) in wrap(&panel.body, max_chars).iter().enumerate() {
        out.push_str(&text_at(
            rect.x + 12.0,
            rect.y + 13.0 + index as f64 * 17.0,
            line,
            "start",
            palette.muted,
            FS_BODY,
            400,
        ));
    }
}

// ── Primitives ──────────────────────────────────────────────────────────────

fn text_at(
    x: f64,
    y: f64,
    value: &str,
    anchor: &str,
    color: &str,
    size: f64,
    weight: u16,
) -> String {
    format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" fill=\"{color}\" font-size=\"{}\" font-weight=\"{weight}\">{}</text>",
        n(x),
        n(y),
        n(size),
        escape(value)
    )
}

fn min_of(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

fn max_of(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn mix(from: &str, to: &str, t: f64) -> String {
    let parse = |value: &str| {
        let hex = value.trim_start_matches('#');
        let expand = |slice: &str| u8::from_str_radix(slice, 16).unwrap_or(0) as f64;
        if hex.len() == 6 {
            (expand(&hex[0..2]), expand(&hex[2..4]), expand(&hex[4..6]))
        } else {
            (0.0, 0.0, 0.0)
        }
    };
    let (r1, g1, b1) = parse(from);
    let (r2, g2, b2) = parse(to);
    let t = t.clamp(0.0, 1.0);
    format!(
        "#{:02X}{:02X}{:02X}",
        (r1 + (r2 - r1) * t).round() as u8,
        (g1 + (g2 - g1) * t).round() as u8,
        (b1 + (b2 - b1) * t).round() as u8
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn n(value: f64) -> String {
    if value.fract().abs() < 0.000_001 {
        format!("{}", value as i64)
    } else {
        let rendered = format!("{value:.3}");
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn source() -> &'static str {
        r#"{
          "version": 1,
          "title": "Craftax cohort survival",
          "subtitle": "craftax-fixture.v1",
          "panels": [
            {"kind":"metrics","items":[
              {"label":"rollouts","value":"200"},
              {"label":"mean achievements","value":"3.04","detail":"+1.40 vs base","tone":"ok"}
            ]},
            {"kind":"series","title":"Survival envelope","x":{"label":"turn"},"y":{"label":"alive"},
             "series":[{"name":"cohort A","points":[{"x":0,"y":1},{"x":10,"y":0.82},{"x":20,"y":null},{"x":30,"y":0.4}],
                        "band":[{"x":0,"lo":0.95,"hi":1},{"x":10,"lo":0.7,"hi":0.9}]},
                       {"name":"cohort B","style":"stepped","points":[{"x":0,"y":1},{"x":10,"y":0.6},{"x":30,"y":0.2}]}]},
            {"kind":"bars","title":"First unlock","categories":["wood","stone","coal"],
             "series":[{"name":"base","values":[12,28,null]},{"name":"tuned","values":[9,21,44]}]},
            {"kind":"scatter","title":"Cost vs score","x":{"label":"usd"},"y":{"label":"score"},
             "frontier":{"xPrefers":"min"},
             "points":[{"x":1.2,"y":0.4,"label":"a"},{"x":2.4,"y":0.7,"label":"b"},{"x":3.9,"y":0.65}]},
            {"kind":"histogram","title":"Turn count","values":[3,4,4,5,9,12,12,13,20],"bins":6},
            {"kind":"heatmap","title":"Achievements","rows":["seed 0","seed 1"],"columns":["wood","stone"],
             "values":[[1,0.5],[null,0.25]]},
            {"kind":"table","title":"Top rollouts","columns":["rollout","score"],
             "rows":[["r-01",0.82],["r-02",null]]},
            {"kind":"note","body":"Cohort B never reached stone."}
          ]
        }"#
    }

    /// Developer utility, not a gate — the renderer is judged by eye, so keep a
    /// supported way to produce something to look at:
    ///
    ///   cargo test -p synth-desktop --lib dump_reference_chart -- --ignored
    ///
    /// writes `target/chart-reference.svg` from the fixture spec below, or from
    /// the spec file named by `SYNTH_CHART_SPEC_FILE`.
    #[test]
    #[ignore]
    fn dump_reference_chart() {
        let spec = match std::env::var("SYNTH_CHART_SPEC_FILE") {
            Ok(path) => std::fs::read_to_string(path).expect("spec file"),
            Err(_) => source().to_string(),
        };
        let rendered = render_svg(&spec).expect("render");
        std::fs::write("target/chart-reference.svg", rendered.svg).expect("write");
    }

    #[test]
    fn renders_every_panel_deterministically() {
        let first = render_svg(source()).expect("render");
        let second = render_svg(source()).expect("render");
        assert_eq!(first.svg, second.svg);
        assert_eq!(first.width, 960);
        let digest = format!("{:x}", Sha256::digest(first.svg.as_bytes()));
        assert_eq!(digest.len(), 64);
        assert!(first.svg.starts_with("<svg"));
        assert!(first.svg.ends_with("</svg>"));
    }

    #[test]
    fn null_series_value_opens_a_gap_instead_of_plotting_zero() {
        let svg = render_svg(source()).expect("render").svg;
        let cohort_a = svg
            .split("<path")
            .find(|chunk| chunk.contains("stroke-width=\"1.8\""))
            .expect("line path");
        assert!(
            cohort_a.matches('M').count() >= 2,
            "a null y must break the path into separate subpaths: {cohort_a}"
        );
    }

    #[test]
    fn absent_measurements_render_as_absence() {
        let svg = render_svg(source()).expect("render").svg;
        // one unmeasured bar category and one unreached heatmap cell
        assert!(svg.matches("url(#absent)").count() >= 2);
        // the missing table cell shows an em dash, never 0
        assert!(svg.contains("—"));
    }

    #[test]
    fn rejects_unknown_fields_and_unsafe_paint() {
        let unknown = r#"{"version":1,"panels":[{"kind":"note","body":"x","emphasis":true}]}"#;
        assert!(parse_and_validate(unknown).is_err());
        let paint = r#"{"version":1,"panels":[{"kind":"bars","title":"t","categories":["a"],
            "series":[{"name":"s","color":"url(#x)","values":[1]}]}]}"#;
        assert!(parse_and_validate(paint).is_err());
    }

    #[test]
    fn rejects_shape_mismatches() {
        let bars = r#"{"version":1,"panels":[{"kind":"bars","title":"t","categories":["a","b"],
            "series":[{"name":"s","values":[1]}]}]}"#;
        assert!(parse_and_validate(bars).is_err());
        let heatmap = r#"{"version":1,"panels":[{"kind":"heatmap","title":"t","rows":["r"],
            "columns":["c","d"],"values":[[1]]}]}"#;
        assert!(parse_and_validate(heatmap).is_err());
    }

    #[test]
    fn escapes_author_supplied_text() {
        let spec = r#"{"version":1,"title":"<script>alert(1)</script>",
            "panels":[{"kind":"note","body":"a & b"}]}"#;
        let svg = render_svg(spec).expect("render").svg;
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
        assert!(svg.contains("a &amp; b"));
    }

    #[test]
    fn frontier_keeps_the_nondominated_set() {
        let points = vec![
            ScatterPoint {
                x: 1.0,
                y: 0.4,
                label: None,
                color: None,
                group: None,
            },
            ScatterPoint {
                x: 2.0,
                y: 0.7,
                label: None,
                color: None,
                group: None,
            },
            ScatterPoint {
                x: 3.0,
                y: 0.5,
                label: None,
                color: None,
                group: None,
            },
        ];
        let max = frontier_points(
            &points,
            &Frontier {
                x_prefers: "max".into(),
                y_prefers: "max".into(),
            },
        );
        assert_eq!(max, vec![(2.0, 0.7), (3.0, 0.5)]);
        let cheap = frontier_points(
            &points,
            &Frontier {
                x_prefers: "min".into(),
                y_prefers: "max".into(),
            },
        );
        assert_eq!(cheap, vec![(1.0, 0.4), (2.0, 0.7)]);
    }

    #[test]
    fn empty_panels_state_their_emptiness() {
        let spec = r#"{"version":1,"panels":[{"kind":"series","title":"t",
            "series":[{"name":"s","points":[{"x":0,"y":null}]}]}]}"#;
        let svg = render_svg(spec).expect("render").svg;
        assert!(svg.contains("no plotted values"));
    }

    #[test]
    fn authoring_findings_flag_illegible_density() {
        let series: Vec<String> = (0..8)
            .map(|index| format!("{{\"name\":\"s{index}\",\"points\":[{{\"x\":0,\"y\":1}}]}}"))
            .collect();
        let spec = format!(
            r#"{{"version":1,"panels":[{{"kind":"series","title":"dense","series":[{}]}}]}}"#,
            series.join(",")
        );
        let findings = authoring_findings(&spec).expect("findings");
        assert!(findings
            .iter()
            .any(|line| line.contains("eight") || line.contains("8 series")));
    }

    #[test]
    fn width_and_panel_count_are_bounded() {
        let wide = r#"{"version":1,"width":9000,"panels":[{"kind":"note","body":"x"}]}"#;
        assert!(parse_and_validate(wide).is_err());
        let none = r#"{"version":1,"panels":[]}"#;
        assert!(parse_and_validate(none).is_err());
    }

    #[test]
    fn dark_theme_changes_only_the_palette() {
        let light = render_svg(source()).expect("render").svg;
        let dark_source =
            source().replacen(r#""version": 1,"#, r#""version": 1, "theme": "dark","#, 1);
        let dark = render_svg(&dark_source).expect("render").svg;
        assert!(dark.contains("#0D0F13"));
        assert_ne!(light, dark);
        assert_eq!(
            light.matches("<rect").count(),
            dark.matches("<rect").count(),
            "themes must not change the drawn geometry"
        );
    }
}
