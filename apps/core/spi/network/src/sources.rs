// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

/// Selects the source policy for a buffered request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    Metadata,
    File,
    /// Installer metadata with no independently trusted digest.
    UnverifiedMetadata,
    /// An artifact with no independently trusted digest.
    UnverifiedFile,
}

/// An alternate URL that serves the same resource as the supplied URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadMirror {
    pub url: String,
    pub preferred: bool,
}

/// A DNS hostname supplied for an HTTPS mirror or proxy.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct ServiceHost(String);

impl TryFrom<String> for ServiceHost {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or("invalid service hostname")
    }
}

impl ServiceHost {
    /// Accepts a bare, qualified ASCII hostname with no URL components.
    pub fn parse(value: &str) -> Option<Self> {
        let (_, suffix) = value.rsplit_once('.')?;
        if value.len() > 253 || !suffix.as_bytes().iter().any(u8::is_ascii_alphabetic) {
            return None;
        }
        value
            .split('.')
            .all(valid_label)
            .then(|| Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    bytes.len() <= 63
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}
