use serde::{Deserialize, Serialize};

use super::CatalogError;

pub(super) const MAX_BATCH: usize = 100;

/// A bounded batch of IDs or hashes sent as one API query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Batch<T>(Vec<T>);

impl<T> Batch<T> {
    pub fn new(items: Vec<T>) -> Result<Self, CatalogError> {
        check_batch(&items)?;
        Ok(Self(items))
    }

    pub fn items(&self) -> &[T] {
        &self.0
    }

    pub(super) fn validate(
        &self,
        validate: impl Fn(&T) -> Result<(), CatalogError>,
    ) -> Result<(), CatalogError> {
        check_batch(&self.0)?;
        self.0.iter().try_for_each(validate)
    }
}

fn check_batch<T>(items: &[T]) -> Result<(), CatalogError> {
    if items.is_empty() || items.len() > MAX_BATCH {
        return Err(CatalogError::InvalidRequest);
    }
    Ok(())
}

pub(super) fn valid_key(value: &str, extras: &[u8], max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || extras.contains(&byte))
}

macro_rules! keys {
    ($($key:ident: $extra:literal, $max:literal;)+) => {$(
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $key(String);

        impl $key {
            pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
                let key = Self(value.into());
                key.validate()?;
                Ok(key)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub(super) fn validate(&self) -> Result<(), CatalogError> {
                valid_key(&self.0, $extra, $max)
                    .then_some(())
                    .ok_or(CatalogError::InvalidRequest)
            }
        }
    )+};
}

keys! {
    UserKey: b"._-", 64;
    VersionId: b"", 11;
    TeamId: b"", 11;
    CollectionId: b"", 11;
    VersionKey: b"!@$()`.+,_\"-", 128;
    LicenseId: b"._+-", 128;
}
