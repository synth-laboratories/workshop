//! Newtypes for every identifier, revision, digest, seed, and bounded count
//! that reaches an execution specification.
//!
//! The point is not ceremony. Inline admission derives a specification from a
//! conversational request, and a bare `String` lets a policy revision be passed
//! where a container registration was meant, or an empty string stand in for a
//! value nobody actually supplied. Each type here refuses to construct itself
//! from a blank, a placeholder, or a zero, so "missing" cannot be spelled as a
//! legal value further down the pipeline.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};

/// A non-empty, trimmed identifier. Constructing one is the only way to get a
/// value into a specification field, so an empty or whitespace-only string is
/// rejected at the edge rather than becoming a silent placeholder.
macro_rules! identifier_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Reject blanks. A caller that has nothing to supply must say so
            /// with `None`, never with `""`.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(IdentifierError {
                        type_name: stringify!($name),
                        reason: IdentifierRejection::Empty,
                    });
                }
                if is_placeholder(trimmed) {
                    return Err(IdentifierError {
                        type_name: stringify!($name),
                        reason: IdentifierRejection::Placeholder(trimmed.to_string()),
                    });
                }
                Ok(Self(trimmed.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::new(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// Values that look like a caller filled a required field with a stand-in.
/// These are refused by name: a specification carrying `"unknown"` as its model
/// id is not a specification, and discovering that at execution time is exactly
/// the failure mode inline admission exists to prevent.
const PLACEHOLDER_VALUES: [&str; 10] = [
    "unknown",
    "none",
    "null",
    "nil",
    "n/a",
    "na",
    "todo",
    "tbd",
    "placeholder",
    "undefined",
];

fn is_placeholder(value: &str) -> bool {
    let folded = value.to_ascii_lowercase();
    PLACEHOLDER_VALUES.contains(&folded.as_str())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentifierRejection {
    Empty,
    Placeholder(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentifierError {
    pub type_name: &'static str,
    pub reason: IdentifierRejection,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            IdentifierRejection::Empty => write!(
                formatter,
                "{} must be a non-empty value; a field with nothing to supply must be omitted, not blank",
                self.type_name
            ),
            IdentifierRejection::Placeholder(value) => write!(
                formatter,
                "{} rejected placeholder value `{}`; supply the real identifier or omit the field",
                self.type_name, value
            ),
        }
    }
}

impl std::error::Error for IdentifierError {}

identifier_newtype!(
    /// Registered container identity, as returned by container discovery.
    ContainerId
);
identifier_newtype!(
    /// The specific registration record a container pin was read from. Two
    /// registrations of the same image are different pins.
    ContainerRegistrationId
);
identifier_newtype!(
    /// The container's own source revision (image digest, commit, or tag the
    /// service itself advertises). Never inferred from the registration.
    SourceRevision
);
identifier_newtype!(
    /// Digest over the container's declared live-eval capability block.
    DeclarationDigest
);
identifier_newtype!(
    /// Catalog recipe identity. Only ever resolved on an explicit request.
    RecipeId
);
identifier_newtype!(
    /// Digest a caller may pin a catalog recipe to.
    RecipeDigest
);
identifier_newtype!(
    /// Resolved, immutable policy revision. A mutable policy name without one
    /// of these is `policy_revision_unresolved`.
    PolicyRevision
);
identifier_newtype!(
    /// Inference provider identity, e.g. `openrouter`.
    ProviderId
);
identifier_newtype!(
    /// Provider-qualified model identity, e.g. `z-ai/glm-5.3-flash`.
    ModelId
);
identifier_newtype!(
    /// Evaluator identity, as declared by the container or named explicitly.
    EvaluatorId
);
identifier_newtype!(
    /// The approval receipt an admissible specification was bound to.
    ApprovalReceiptId
);
identifier_newtype!(
    /// One rollout's durable identity, minted by the container.
    RolloutId
);

/// A content digest. Rendered as `sha256:<64 lowercase hex>` so that a digest
/// is never confused with the content it covers, and so a truncated or
/// re-encoded digest fails to parse instead of comparing unequal in silence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Digest(String);

impl Digest {
    pub const PREFIX: &'static str = "sha256:";

    /// Wrap raw sha256 bytes.
    pub fn from_sha256(bytes: [u8; 32]) -> Self {
        let mut rendered = String::with_capacity(Self::PREFIX.len() + 64);
        rendered.push_str(Self::PREFIX);
        for byte in bytes {
            rendered.push_str(&format!("{byte:02x}"));
        }
        Self(rendered)
    }

    /// Parse a rendered digest. Anything that is not exactly the prefix plus 64
    /// lowercase hex characters is refused; a digest that "almost" parses is a
    /// corruption, not a value to normalize.
    pub fn parse(value: impl Into<String>) -> Result<Self, DigestError> {
        let value = value.into();
        let trimmed = value.trim();
        let Some(hex) = trimmed.strip_prefix(Self::PREFIX) else {
            return Err(DigestError::MissingPrefix(trimmed.to_string()));
        };
        if hex.len() != 64 {
            return Err(DigestError::WrongLength {
                expected: 64,
                actual: hex.len(),
            });
        }
        if !hex
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
        {
            return Err(DigestError::NotLowercaseHex(hex.to_string()));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The hex body without the algorithm prefix, for display truncation only.
    pub fn short(&self) -> &str {
        &self.0[Self::PREFIX.len()..Self::PREFIX.len() + 12]
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DigestError {
    MissingPrefix(String),
    WrongLength { expected: usize, actual: usize },
    NotLowercaseHex(String),
}

impl fmt::Display for DigestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrefix(value) => write!(
                formatter,
                "digest `{value}` is not prefixed with `{}`",
                Digest::PREFIX
            ),
            Self::WrongLength { expected, actual } => write!(
                formatter,
                "digest body must be {expected} hex characters, found {actual}"
            ),
            Self::NotLowercaseHex(value) => {
                write!(formatter, "digest body `{value}` is not lowercase hex")
            }
        }
    }
}

impl std::error::Error for DigestError {}

/// One rollout seed. Seeds are explicit request data; Workshop never invents,
/// reorders, or extends them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seed(pub i64);

impl fmt::Display for Seed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A count of model calls. `NonZeroU32` because a limit of zero is not a limit,
/// it is a specification that can never produce a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelCallCount(pub NonZeroU32);

/// A count of environment steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepCount(pub NonZeroU32);

/// A count of rollouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RolloutCount(pub NonZeroU32);

/// Currency in micros (1 USD = 1_000_000). Integer so that a ceiling never
/// drifts through floating-point accumulation, and `NonZeroU64` so that a
/// "free" paid-compute ceiling cannot be expressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CostMicros(pub NonZeroU64);

impl CostMicros {
    /// Convert a US-dollar figure supplied by a human request. Rounds to the
    /// nearest micro and refuses anything that is not a positive, finite
    /// amount — a NaN or negative ceiling must not become `0`.
    pub fn from_usd(value: f64) -> Result<Self, CostError> {
        if !value.is_finite() {
            return Err(CostError::NotFinite);
        }
        if value <= 0.0 {
            return Err(CostError::NotPositive(value));
        }
        let micros = (value * 1_000_000.0).round();
        if micros > u64::MAX as f64 {
            return Err(CostError::Overflow(value));
        }
        NonZeroU64::new(micros as u64)
            .map(Self)
            .ok_or(CostError::NotPositive(value))
    }

    pub fn as_micros(self) -> u64 {
        self.0.get()
    }

    /// Render for display. Never used to compare ceilings — comparison is
    /// always on the integer micros.
    pub fn render_usd(self) -> String {
        format!("${:.2}", self.0.get() as f64 / 1_000_000.0)
    }
}

impl fmt::Display for CostMicros {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render_usd())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CostError {
    NotFinite,
    NotPositive(f64),
    Overflow(f64),
}

impl fmt::Display for CostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite => formatter.write_str("cost ceiling must be a finite amount"),
            Self::NotPositive(value) => write!(
                formatter,
                "cost ceiling must be greater than zero, found {value}"
            ),
            Self::Overflow(value) => write!(formatter, "cost ceiling {value} is out of range"),
        }
    }
}

impl std::error::Error for CostError {}

/// Helper for the common `NonZeroU32` construction from a request field.
pub fn non_zero_u32(value: u32, field: &'static str) -> Result<NonZeroU32, BoundError> {
    NonZeroU32::new(value).ok_or(BoundError { field })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundError {
    pub field: &'static str,
}

impl fmt::Display for BoundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} must be greater than zero; a zero bound is not a bound",
            self.field
        )
    }
}

impl std::error::Error for BoundError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identifier_refuses_blanks_and_placeholders() {
        assert!(ContainerId::new("nanohorizon-craftax").is_ok());
        assert!(ContainerId::new("   ").is_err());
        assert!(ContainerId::new("").is_err());
        // A placeholder is exactly the "missing value spelled as a legal
        // value" case the newtypes exist to stop.
        assert!(ContainerId::new("unknown").is_err());
        assert!(ContainerId::new("TBD").is_err());
    }

    #[test]
    fn an_identifier_trims_but_does_not_otherwise_rewrite() {
        let identifier = ModelId::new("  z-ai/glm-5.3-flash  ").unwrap();
        assert_eq!(identifier.as_str(), "z-ai/glm-5.3-flash");
    }

    #[test]
    fn a_digest_round_trips_and_refuses_near_misses() {
        let digest = Digest::from_sha256([0xab; 32]);
        assert_eq!(digest.as_str(), format!("sha256:{}", "ab".repeat(32)));
        assert_eq!(Digest::parse(digest.as_str()).unwrap(), digest);
        assert!(Digest::parse("ab".repeat(32)).is_err(), "prefix required");
        assert!(
            Digest::parse(format!("sha256:{}", "AB".repeat(32))).is_err(),
            "uppercase hex is a re-encoding, not the same digest"
        );
        assert!(Digest::parse("sha256:abc").is_err(), "truncated");
    }

    #[test]
    fn a_cost_ceiling_never_rounds_down_to_free() {
        assert_eq!(CostMicros::from_usd(2.45).unwrap().as_micros(), 2_450_000);
        assert_eq!(CostMicros::from_usd(2.45).unwrap().render_usd(), "$2.45");
        assert!(CostMicros::from_usd(0.0).is_err());
        assert!(CostMicros::from_usd(-1.0).is_err());
        assert!(CostMicros::from_usd(f64::NAN).is_err());
        // Rounding a sub-micro amount must fail rather than produce a zero
        // ceiling that would read as "no spend allowed" or, worse, "unbounded".
        assert!(CostMicros::from_usd(1e-9).is_err());
    }

    #[test]
    fn a_zero_bound_is_refused() {
        assert!(non_zero_u32(0, "maximum_rollouts").is_err());
        assert_eq!(non_zero_u32(5, "maximum_rollouts").unwrap().get(), 5);
    }
}
