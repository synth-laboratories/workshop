//! Packaged local client; no Tauri linkage or second database owner.
#[path = "../agent_integration/mod.rs"]
mod agent_integration;
#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

fn main() {
    if let Err(error) = agent_integration::run(std::env::args().skip(1).collect()) {
        eprintln!("Workshop: {error}");
        std::process::exit(1);
    }
}
