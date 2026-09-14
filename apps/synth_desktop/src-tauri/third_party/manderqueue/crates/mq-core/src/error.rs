use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("forbidden: {0}")]
    Forbidden(&'static str),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("conflict: {0}")]
    Conflict(&'static str),
    #[error("invalid: {0}")]
    Invalid(&'static str),
}
