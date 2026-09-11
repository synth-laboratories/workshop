//! Candidate wire document for backend 5f9166a30 (and compatible follow-ups).
//! Receiving this document does not activate account storage or grant access.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IdentityDocument {
    pub schema_version: String,
    pub backend_origin: String,
    pub backend_id: String,
    pub account_id: String,
    pub org_id: String,
    pub profile_id: String,
    pub verified_at: String,
    pub valid_until: String,
    pub credential_expiry: Option<String>,
    pub revalidate_before_remote_operation: bool,
    pub revocation_contract: String,
}
