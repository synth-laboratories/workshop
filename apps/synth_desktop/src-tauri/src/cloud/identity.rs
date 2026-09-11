//! Candidate identity observation validation. This is not an authorization lease
//! and does not register storage or enable a live adapter.
use super::storage::CloudScopeIdentity;
use anyhow::{bail, Result};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct IdentityObservation {
    pub schema_version: String,
    pub backend_origin: String,
    pub backend_id: String,
    pub account_id: String,
    pub org_id: String,
    pub profile_id: String,
    pub verified_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub credential_expiry: Option<DateTime<Utc>>,
    pub revalidate_before_remote_operation: bool,
    pub revocation_contract: String,
}

impl IdentityObservation {
    /// Validate a response against the transport's expected origin and local
    /// clock. The caller must still fetch afresh before every remote operation.
    /// A cache hit, valid timestamp, or matching tuple is never authorization.
    pub fn validate(
        &self,
        expected_origin: &str,
        now: DateTime<Utc>,
    ) -> Result<CloudScopeIdentity> {
        if self.schema_version != "synth.desktop-cloud-identity.v1"
            || !self.revalidate_before_remote_operation
            || self.revocation_contract != "fresh_database_key_and_membership_check"
        {
            bail!("unsupported cloud identity authority contract");
        }
        let origin = reqwest::Url::parse(&self.backend_origin)?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.port().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || origin.origin().ascii_serialization() != self.backend_origin
            || self.backend_origin != expected_origin
        {
            bail!("cloud identity origin mismatch or noncanonical origin");
        }
        if self.verified_at > now
            || self.valid_until <= now
            || self.valid_until <= self.verified_at
            || self.valid_until - self.verified_at > Duration::seconds(60)
            || self.credential_expiry.is_some_and(|expiry| expiry <= now)
        {
            bail!("cloud identity observation is stale or invalid");
        }
        for value in [
            &self.backend_id,
            &self.account_id,
            &self.org_id,
            &self.profile_id,
        ] {
            let id = uuid::Uuid::parse_str(value)?;
            if id.is_nil() || id.to_string() != *value {
                bail!("cloud identity requires canonical nonempty UUIDs");
            }
        }
        Ok(CloudScopeIdentity {
            backend_origin: self.backend_origin.clone(),
            backend_id: self.backend_id.clone(),
            account_id: self.account_id.clone(),
            org_id: self.org_id.clone(),
            profile_id: self.profile_id.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> IdentityObservation {
        serde_json::from_value(serde_json::json!({
            "schema_version":"synth.desktop-cloud-identity.v1",
            "backend_origin":"https://cloud.example.test",
            "backend_id":"00000000-0000-4000-8000-000000000001",
            "account_id":"00000000-0000-4000-8000-000000000002",
            "org_id":"00000000-0000-4000-8000-000000000003",
            "profile_id":"00000000-0000-4000-8000-000000000004",
            "verified_at":"2026-09-11T12:00:00Z",
            "valid_until":"2026-09-11T12:01:00Z",
            "credential_expiry":null,
            "revalidate_before_remote_operation":true,
            "revocation_contract":"fresh_database_key_and_membership_check"
        }))
        .unwrap()
    }
    #[test]
    fn observation_is_origin_bound_and_expires_at_deadline() {
        let observation = fixture();
        let now = observation.verified_at + Duration::seconds(1);
        assert!(observation
            .validate(&observation.backend_origin, now)
            .is_ok());
        assert!(observation
            .validate("https://other.example.test", now)
            .is_err());
        assert!(observation
            .validate(&observation.backend_origin, observation.valid_until)
            .is_err());
        assert!(observation
            .validate(
                &observation.backend_origin,
                observation.verified_at - Duration::seconds(1)
            )
            .is_err());
    }
    #[test]
    fn authority_contract_cannot_be_downgraded() {
        let mut observation = fixture();
        let now = observation.verified_at;
        observation.revalidate_before_remote_operation = false;
        assert!(observation
            .validate(&observation.backend_origin, now)
            .is_err());
        observation.revalidate_before_remote_operation = true;
        observation.valid_until = now + Duration::seconds(61);
        assert!(observation
            .validate(&observation.backend_origin, now)
            .is_err());
        observation.valid_until = now + Duration::seconds(60);
        observation.account_id = "organization-fallback".into();
        assert!(observation
            .validate(&observation.backend_origin, now)
            .is_err());
    }
}
