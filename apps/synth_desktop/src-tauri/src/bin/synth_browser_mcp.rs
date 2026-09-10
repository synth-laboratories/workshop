//! Compatibility stdio transport for the runtime-owned managed browser.
fn main() {
    if let Err(error) = synth_desktop_lib::adapters::mcp::run_browser_stdio() {
        eprintln!("Workshop browser: {error}");
        std::process::exit(1);
    }
}
