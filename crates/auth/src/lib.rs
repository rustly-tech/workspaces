//! Authentication remains independent of the HTTP and storage implementations.

use std::{collections::HashMap, sync::Arc};

use rustly_git_protocol::Slug;
use thiserror::Error;

/// An authenticated account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub account: Slug,
}

/// Provider-neutral authentication boundary.
pub trait Authenticator: Send + Sync {
    fn authenticate(&self, bearer: &str) -> Result<Principal, AuthError>;
}

pub type DynAuthenticator = Arc<dyn Authenticator>;

/// Development and self-hosting authenticator backed by hashed static tokens.
///
/// Raw tokens are hashed during construction and are never retained.
#[derive(Debug, Default)]
pub struct StaticTokens {
    tokens: HashMap<blake3::Hash, Slug>,
}

impl StaticTokens {
    pub fn new(credentials: impl IntoIterator<Item = (Slug, String)>) -> Result<Self, AuthError> {
        let mut tokens = HashMap::new();
        for (account, token) in credentials {
            if token.len() < 32 {
                return Err(AuthError::WeakToken);
            }
            if tokens
                .insert(blake3::hash(token.as_bytes()), account)
                .is_some()
            {
                return Err(AuthError::DuplicateToken);
            }
        }
        if tokens.is_empty() {
            return Err(AuthError::NoCredentials);
        }
        Ok(Self { tokens })
    }

    /// Parse `RUSTLY_GIT_CREDENTIALS`, a JSON object from account names to tokens.
    pub fn from_json(json: &str) -> Result<Self, AuthError> {
        let raw: HashMap<String, String> = serde_json::from_str(json)?;
        let credentials = raw
            .into_iter()
            .map(|(account, token)| account.parse().map(|account| (account, token)))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(credentials)
    }
}

impl Authenticator for StaticTokens {
    fn authenticate(&self, bearer: &str) -> Result<Principal, AuthError> {
        self.tokens
            .get(&blake3::hash(bearer.as_bytes()))
            .cloned()
            .map(|account| Principal { account })
            .ok_or(AuthError::InvalidCredentials)
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no credentials configured")]
    NoCredentials,
    #[error("tokens must contain at least 32 bytes")]
    WeakToken,
    #[error("one token cannot identify two accounts")]
    DuplicateToken,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("invalid account identifier: {0}")]
    InvalidAccount(#[from] rustly_git_protocol::InvalidSlug),
    #[error("invalid credential JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_scoped_to_one_account_and_not_retained_as_text() {
        let secret = "correct horse battery staple 1234";
        let auth = StaticTokens::new([("learner".parse().unwrap(), secret.to_owned())]).unwrap();
        assert_eq!(
            auth.authenticate(secret).unwrap().account.as_str(),
            "learner"
        );
        assert!(matches!(
            auth.authenticate("wrong wrong wrong wrong wrong wrong"),
            Err(AuthError::InvalidCredentials)
        ));
        assert!(!format!("{auth:?}").contains(secret));
    }

    #[test]
    fn configuration_rejects_weak_duplicate_and_malformed_credentials() {
        assert!(matches!(
            StaticTokens::new([("user".parse().unwrap(), "short".to_owned())]),
            Err(AuthError::WeakToken)
        ));
        assert!(StaticTokens::from_json(r#"{"Bad":"abcdefghijklmnopqrstuvwxyz123456"}"#).is_err());
        assert!(StaticTokens::from_json("not json").is_err());
    }
}
