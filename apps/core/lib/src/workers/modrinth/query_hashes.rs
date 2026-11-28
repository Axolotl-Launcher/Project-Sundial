use serde::{Deserialize, Serialize};

use super::{Batch, CatalogError, VersionId, VersionType, filters::validate_filters};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha1,
    Sha512,
}

impl HashAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha512 => "sha512",
        }
    }

    pub fn accepts(self, hash: &str) -> bool {
        let length = match self {
            Self::Sha1 => 40,
            Self::Sha512 => 128,
        };
        hash.len() == length && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn validate(self, hash: &str) -> Result<(), CatalogError> {
        self.accepts(hash)
            .then_some(())
            .ok_or(CatalogError::InvalidRequest)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HashLookup {
    pub algorithm: HashAlgorithm,
    pub hash: String,
    pub version: Option<VersionId>,
}

impl HashLookup {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        self.algorithm.validate(&self.hash)?;
        if let Some(version) = &self.version {
            version.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HashBatch {
    pub algorithm: HashAlgorithm,
    pub hashes: Batch<String>,
}

impl HashBatch {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        self.hashes.validate(|hash| self.algorithm.validate(hash))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_versions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loaders: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_types: Option<Vec<VersionType>>,
}

impl UpdateFilters {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        validate_filters(
            self.loaders.as_deref().unwrap_or_default(),
            self.game_versions.as_deref().unwrap_or_default(),
        )?;
        if self
            .version_types
            .as_ref()
            .is_some_and(|types| types.len() > 3)
        {
            return Err(CatalogError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BulkUpdate {
    pub hashes: HashBatch,
    pub filters: UpdateFilters,
}

impl BulkUpdate {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        self.hashes.validate()?;
        self.filters.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndividualUpdate {
    pub hash: String,
    pub filters: UpdateFilters,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndividualUpdates {
    pub algorithm: HashAlgorithm,
    pub hashes: Batch<IndividualUpdate>,
}

impl IndividualUpdates {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        self.hashes.validate(|entry| {
            self.algorithm.validate(&entry.hash)?;
            entry.filters.validate()
        })
    }
}
