//! The gate on persisting visual template code.
//!
//! Writing `shell.tsx` or `renderer.html` under `<state root>/visuals/templates`
//! is a different act from rendering TSX in the pane. The pane compiles a string
//! that dies with the view; this leaves a file the registry scans on every
//! launch, for every future session, until someone deletes it.
//! `internal_readme.md` fixes the precedent for that class — entering an API key
//! "creates persistent access and must follow the Computer Use confirmation
//! policy" — and part V trap 2 names the mechanism: a new [`ApprovalKind`]
//! through the existing [`ApprovalBroker`], modeled on `PluginLifecycle`.
//!
//! **[`PersistConsent`] has private fields and exactly one public constructor,
//! [`authorize`].** That is the whole enforcement design: a writer cannot be
//! called without one, and one cannot be produced without a settled approval.
//! The predecessor was a single-variant enum documented as "a speed bump, not a
//! gate" — it type-checked a promise nobody had to keep, because the variant was
//! constructible anywhere in the crate. Privacy is what turns the signature into
//! a check.
//!
//! **Both write routes go through here, and one of them was already open.**
//! `visual_save_template` is new; `visual_import_template` is not — it has been
//! reachable from the agent HTTP seam since the managed registry shipped, and it
//! writes a `renderer.html` package into the instance state root with no
//! confirmation at all. A gate on the new writers alone would have been theatre.

use super::approval::{ApprovalBroker, ApprovalKind};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tauri::{AppHandle, Manager};

/// What this write does to an id, and where its code came from.
///
/// Two independent facts rather than one enum, because they are independent:
/// forking a shipped family into a brand-new id and forking it over a user
/// template you wrote last week are both forks, and only one of them destroys
/// something. Collapsing them would make the card say "fork" for a write that
/// replaces reviewed code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PersistDisposition {
    /// A template already owns this id and its source is being replaced.
    pub overwrites: bool,
    /// Template this one was copied from. Fork, never shadow.
    pub forked_from: Option<String>,
}

/// Everything the card must state, gathered before a single byte is written.
#[derive(Debug)]
pub(crate) struct PersistRequest {
    /// Template id, which is also the directory name under the root.
    pub template_id: String,
    /// `user` (`shell.tsx`, compiled in the pane) or `managed`
    /// (`renderer.html`, sandboxed iframe). The two grant different
    /// capabilities, so the tier is part of the question.
    pub source_kind: String,
    /// `save`, `fork`, or `import`.
    pub action: String,
    /// Bytes of code being persisted.
    pub byte_size: u64,
    pub disposition: PersistDisposition,
    /// Directory that will be written.
    pub destination: String,
    /// SHA-256 of the source bytes, so the card and the receipt name the same
    /// code. Prefixed `sha256:` to match how digests are spelled on every other
    /// approval card.
    pub source_digest: String,
}

impl PersistRequest {
    /// Build a request from the bytes about to be written.
    ///
    /// `source` is the renderer code itself — `shell.tsx` or `renderer.html` —
    /// not the manifest. The manifest is metadata the registry re-validates
    /// after the write; the source is what will run.
    pub(crate) fn new(
        template_id: &str,
        source_kind: &str,
        action: &str,
        source: &[u8],
        disposition: PersistDisposition,
        destination: &Path,
    ) -> Self {
        Self {
            template_id: template_id.to_owned(),
            source_kind: source_kind.to_owned(),
            action: action.to_owned(),
            byte_size: source.len() as u64,
            disposition,
            destination: destination.display().to_string(),
            source_digest: format!("sha256:{:x}", Sha256::digest(source)),
        }
    }

    fn kind(&self) -> ApprovalKind {
        ApprovalKind::VisualTemplatePersist {
            template_id: self.template_id.clone(),
            source_kind: self.source_kind.clone(),
            action: self.action.clone(),
            byte_size: self.byte_size,
            overwrites: self.disposition.overwrites,
            forked_from: self.disposition.forked_from.clone(),
            destination: self.destination.clone(),
            source_digest: self.source_digest.clone(),
        }
    }
}

/// Proof that a person settled an approval describing one specific write.
///
/// Deliberately not `Clone` and not `Copy`: a consent is spent by the write it
/// authorized. Copying one would be how "approve this template" quietly becomes
/// "approve every template written afterwards".
pub(crate) struct PersistConsent {
    approval_id: String,
    template_id: String,
    source_digest: String,
}

impl PersistConsent {
    /// Refuse a consent that was granted for a different template or different
    /// bytes than the one now being written.
    ///
    /// The broker settles a request; nothing in the broker knows which write
    /// then happens. Binding here is what stops one approval from being used to
    /// authorize a second, unreviewed write in the same call chain — the same
    /// reason `ComputerUse` binds consent to `payload` rather than to the app.
    pub(crate) fn bind(&self, request: &PersistRequest) -> Result<()> {
        if self.template_id != request.template_id || self.source_digest != request.source_digest {
            return Err(anyhow!(
                "visual template approval {} was granted for {} ({}), not for {} ({})",
                self.approval_id,
                self.template_id,
                self.source_digest,
                request.template_id,
                request.source_digest,
            ));
        }
        Ok(())
    }

}

/// Put the write in front of a person and return the consent if they allow it.
///
/// `session_id` is required in practice rather than by the signature:
/// `authorize_host` refuses a `requires_human` kind that arrives with no session
/// context, because there is no card to show and no policy that may settle it.
/// The parameter stays `Option` to match `authorize_host`, so the refusal has
/// one owner and reads the same here as everywhere else.
pub(crate) async fn authorize<R: tauri::Runtime>(
    app: &AppHandle<R>,
    session_id: Option<&str>,
    request: &PersistRequest,
) -> Result<PersistConsent> {
    let broker = app
        .try_state::<Arc<ApprovalBroker>>()
        .ok_or_else(|| anyhow!("approval broker unavailable"))?;
    let approval_id = broker
        .authorize_host(app, session_id, request.kind())
        .await?;
    Ok(PersistConsent {
        approval_id,
        template_id: request.template_id.clone(),
        source_digest: request.source_digest.clone(),
    })
}

/// The refusal an ungated seam returns.
///
/// Kept here rather than spelled out at each call site so that every route that
/// has not been given an approval says the same thing, and so that grepping this
/// function finds all of them.
pub(crate) fn unapproved(action: &str, subject: &str) -> anyhow::Error {
    anyhow!(
        "persisting visual template code is not permitted without confirmation: {action} \
         {subject}. This route has no approval broker or session to raise a card with; call the \
         approved entry point on VisualRegistry, which requires a session id and settles a \
         visual_template_persist approval first."
    )
}

