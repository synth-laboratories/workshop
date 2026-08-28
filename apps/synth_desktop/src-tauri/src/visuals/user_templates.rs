//! Read the `shell.tsx` of a user-authored visual template.
//!
//! `templates.rs` decides *what* a user template is: a directory under
//! `<state root>/visuals/templates` holding `template.json` plus `shell.tsx`,
//! indexed with `source_kind: "user"` and `shell_path` set. This module answers
//! the single question the pane then asks — "give me that file's text" — and
//! nothing else.
//!
//! It deliberately does not parse or lint the TSX. The import allowlist, the
//! forbidden-token scan and the 256 KiB pane cap live once, in
//! `visuals/runtime/sourcedValidate.ts`, which fails closed and renders
//! `sourcedInvalidShell` with the exact message a human editing the file sees.
//! A second copy of that rule here would be a second thing to drift.
//!
//! **The structural gate runs before any byte is read.** `resolve_template`
//! rebuilds the whole index on every call — there is no cache — and the user
//! tier runs `templates.rs::checked_template_file` over `template.json` and
//! `shell.tsx` as it scans. A symlink, a non-regular file, or a file above
//! `MANAGED_TEMPLATE_MAX_BYTES` therefore fails this command with templates.rs's
//! own message, from templates.rs's own check, before `read_to_string` is
//! reached. Calling that gate a second time directly would be the clearer
//! spelling; it is private to `templates.rs`, which this change may not edit.

use anyhow::{anyhow, bail, Context, Result};
use std::{fs, path::Path};

/// `source_kind` tag `templates.rs` puts on a `template.json` + `shell.tsx`
/// directory. The pane branches on this, never on a template id.
pub const USER_SOURCE_KIND: &str = "user";

/// TSX source of one user-authored template's shell.
///
/// Refuses anything that is not `source_kind: "user"` — a bundled family
/// resolves its shell through Vite's static graph, and a `managed` package is
/// `renderer.html` rendered in a sandboxed iframe under a CSP. Handing either
/// one's source to `compileSourcedModule` would run it under a capability model
/// its author never agreed to.
pub fn shell_source(template_id: &str) -> Result<String> {
    // Also the id gate: `resolve_template` refuses an empty id, a separator and
    // `..` before it looks anything up, and an id it does not know.
    let meta = super::templates::resolve_template(template_id)?;

    let source_kind = meta.source_kind.as_deref().unwrap_or("bundled");
    if source_kind != USER_SOURCE_KIND {
        bail!(
            "visual template {template_id} has source kind {source_kind:?}; \
             only a user-authored template exposes shell source"
        );
    }

    let shell_path = meta.shell_path.as_deref().ok_or_else(|| {
        anyhow!("user visual template {template_id} declares no shell.tsx")
    })?;
    let shell = Path::new(shell_path);

    // Containment against the one rule that owns this tier's location, rather
    // than against a property it happens to imply. `user_templates_root()` is
    // `templates.rs`'s and is now `pub(super)` precisely so this check does not
    // re-derive the path — re-deriving it is what `template_root_join` in
    // `scripts/conform-desktop.sh` counts, and what item 23 was.
    let root = super::templates::user_templates_root();
    if !shell.starts_with(&root) {
        bail!(
            "user visual template shell is outside the user template root: {}",
            shell.display()
        );
    }

    // The scan gated this file microseconds ago; re-check the one property an
    // attacker could flip in between, because the swap costs nothing and the
    // consequence is reading a file outside the root. This is the same refusal
    // with the same words, not a second policy: the size cap already ran in the
    // scan, and the pane re-caps at 256 KiB.
    let metadata = fs::symlink_metadata(shell)
        .with_context(|| format!("reading {}", shell.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "user visual template registry refuses symlink: {}",
            shell.display()
        );
    }
    if !metadata.is_file() {
        bail!(
            "user visual template entry must be a regular file: {}",
            shell.display()
        );
    }

    fs::read_to_string(shell)
        .with_context(|| format!("user visual template shell must be UTF-8: {}", shell.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A checkout without staged bundled families has no bundled template to
    /// assert against. The user tier itself is independent of it.
    fn bundled_families_present() -> bool {
        super::super::templates::visuals_root()
            .join("families")
            .exists()
    }

    fn write_user_template(id: &str, shell: Option<&str>) -> PathBuf {
        let path = crate::instance::state_root()
            .join("visuals")
            .join("templates")
            .join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("template.json"),
            format!(
                r#"{{"schemaVersion":"synth.visual-template.v1","id":"{id}","version":"1.0.0"}}"#
            ),
        )
        .unwrap();
        if let Some(source) = shell {
            fs::write(path.join("shell.tsx"), source).unwrap();
        }
        path
    }

    #[test]
    fn reads_user_shell_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-source");
        let source = "export default function Shell() { return null; }\n";
        write_user_template("user.readable.v1", Some(source));
        assert_eq!(shell_source("user.readable.v1").unwrap(), source);
    }

    #[test]
    fn unknown_template_id_fails_closed() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-unknown");
        let error = shell_source("user.absent.v1").unwrap_err().to_string();
        assert!(error.contains("unknown visual template"), "{error}");
    }

    #[test]
    fn bundled_template_has_no_shell_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-bundled");
        if !bundled_families_present() {
            return;
        }
        let error = shell_source("analysis.visual.v1").unwrap_err().to_string();
        assert!(error.contains("source kind"), "{error}");
    }

    #[test]
    fn scaffold_without_shell_is_not_readable() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-scaffold");
        // A manifest with no source is skipped by the scan, so the id never
        // enters the index -- the same refusal an unknown id gets.
        write_user_template("user.scaffold.v1", None);
        let error = shell_source("user.scaffold.v1").unwrap_err().to_string();
        assert!(error.contains("unknown visual template"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_shell_fails_closed_before_reading() {
        use std::os::unix::fs::symlink;
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-symlink");
        let outside = tempfile::tempdir().unwrap();
        let planted = outside.path().join("shell.tsx");
        fs::write(&planted, "export default () => null;\n").unwrap();
        let path = write_user_template("user.linked.v1", None);
        symlink(&planted, path.join("shell.tsx")).unwrap();
        let error = shell_source("user.linked.v1").unwrap_err().to_string();
        assert!(error.contains("refuses symlink"), "{error}");
    }
}
