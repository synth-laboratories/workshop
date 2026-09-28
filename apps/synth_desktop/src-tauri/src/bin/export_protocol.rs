//! Generate the desktop TypeScript boundary without launching the app or tests.
fn main() -> Result<(), String> {
    synth_desktop_lib::contract::specta::export_typescript_bindings()
}
