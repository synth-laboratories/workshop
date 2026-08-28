//! Import a Trace V5 bundle into the canonical Desktop store.
//!
//! This is the headless equivalent of Inventory -> Import Trace V5 and is
//! useful for dogfood setup and automation. The Desktop UI sees the imported
//! rows on its next refresh.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use synth_desktop_lib::{trace_ingest::TraceBundleIngestRequest, CoreRuntime};

#[tokio::main]
async fn main() -> Result<()> {
    let source = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: synth-trace-import BUNDLE [TITLE]")?;
    if !source.exists() {
        bail!("trace input does not exist: {}", source.display());
    }
    let title = std::env::args_os()
        .nth(2)
        .map(|value| value.to_string_lossy().into_owned());
    let runtime = CoreRuntime::open_default().context("open Synth Desktop store")?;
    let (result, _) = runtime
        .data()
        .ingest_trace_bundle(TraceBundleIngestRequest {
            source_path: source.display().to_string(),
            source_kind: Some("desktop_cli".into()),
            title,
            source_uri: None,
        })
        .await
        .context("import Trace V5 bundle")?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
