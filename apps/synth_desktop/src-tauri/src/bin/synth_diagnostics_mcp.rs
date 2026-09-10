//! Compatibility stdio entry point; operation definitions live in the shared adapter.
fn main() {
    synth_desktop_lib::adapters::mcp::operations::diagnostics::run();
}
