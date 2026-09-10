//! Values accepted at the Git HTTP trust boundary.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A stable, URL-safe account or workspace identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Slug(String);

impl Slug {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<Slug> for String {
    fn from(value: Slug) -> Self {
        value.0
    }
}

impl TryFrom<String> for Slug {
    type Error = InvalidSlug;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

impl FromStr for Slug {
    type Err = InvalidSlug;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid_length = !value.is_empty() && value.len() <= 63;
        let valid_edges = value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric);
        let valid_characters = value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');

        if valid_length && valid_edges && valid_characters {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidSlug)
        }
    }
}

/// Rustly's stable identity for a coding repository.
///
/// Storage paths and clone URLs are derived from this value by a repository
/// provider. They are locations, not identities, and may change when a learner
/// links or unlinks an external provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId {
    owner: Slug,
    name: Slug,
}

impl RepositoryId {
    pub fn new(owner: Slug, name: Slug) -> Self {
        Self { owner, name }
    }

    pub fn owner(&self) -> &Slug {
        &self.owner
    }

    pub fn name(&self) -> &Slug {
        &self.name
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("must be 1-63 lowercase ASCII letters, digits, or interior hyphens")]
pub struct InvalidSlug;

/// The only RPCs Git Smart HTTP may dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitService {
    UploadPack,
    ReceivePack,
}

impl GitService {
    pub const fn command(self) -> &'static str {
        match self {
            Self::UploadPack => "git-upload-pack",
            Self::ReceivePack => "git-receive-pack",
        }
    }

    pub const fn is_write(self) -> bool {
        matches!(self, Self::ReceivePack)
    }

    pub fn parse(value: &str) -> Result<Self, InvalidService> {
        match value {
            "git-upload-pack" => Ok(Self::UploadPack),
            "git-receive-pack" => Ok(Self::ReceivePack),
            _ => Err(InvalidService),
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("only git-upload-pack and git-receive-pack are allowed")]
pub struct InvalidService;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_cannot_escape_or_smuggle_paths() {
        for bad in [
            "", "../owner", "a/b", ".git", "Upper", "-edge", "edge-", "a_b",
        ] {
            assert!(bad.parse::<Slug>().is_err(), "accepted {bad:?}");
        }
        assert_eq!(
            "ownership-101".parse::<Slug>().unwrap().as_str(),
            "ownership-101"
        );
    }

    #[test]
    fn only_the_two_smart_http_services_are_dispatchable() {
        assert_eq!(
            GitService::parse("git-upload-pack").unwrap(),
            GitService::UploadPack
        );
        assert_eq!(
            GitService::parse("git-receive-pack").unwrap(),
            GitService::ReceivePack
        );
        assert!(GitService::parse("upload-archive").is_err());
        assert!(GitService::parse("sh -c anything").is_err());
    }

    #[test]
    fn repository_identity_contains_no_location() {
        let id = RepositoryId::new("learner".parse().unwrap(), "ownership".parse().unwrap());
        let encoded = serde_json::to_string(&id).unwrap();
        assert_eq!(encoded, r#"{"owner":"learner","name":"ownership"}"#);
        assert!(!encoded.contains('/') && !encoded.contains("http"));
    }
}
