use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ForgeUpdates {
    pub homepage: String,
    pub promos: BTreeMap<String, String>,
    #[serde(flatten)]
    pub versions: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Statistics {
    pub projects: u64,
    pub versions: u64,
    pub authors: u64,
    pub files: u64,
}

/// Public incident notices, not creator analytics.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub id: String,
    pub title: String,
    pub announcement_url: Option<String>,
    pub for_metric_kind: Vec<String>,
    pub starts: String,
    pub ends: String,
}

/// Anonymous payment-method availability, not a user payout account.
#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentMethod {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub category: Option<String>,
    pub currency_code: Option<String>,
    pub exchange_rate: Option<Amount>,
    pub interval: PaymentInterval,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentInterval {
    Standard { min: Amount, max: Amount },
    Fixed { values: Vec<Amount> },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Amount {
    Number(f64),
    Decimal(String),
}

/// Platform-wide aggregates as decimal strings, without account or transaction details.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformRevenue {
    pub all_time: String,
    pub all_time_available: String,
    pub data: Vec<RevenuePoint>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevenuePoint {
    pub time: u64,
    pub revenue: String,
    pub creator_revenue: String,
}

/// A verified Modrinth API or CDN location. No automatic download is started.
#[derive(Debug, Serialize, Deserialize)]
pub struct Redirect {
    pub status: u16,
    pub location: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MavenResult<T> {
    Value(T),
    Redirect(Redirect),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenMetadata {
    pub group_id: String,
    pub artifact_id: String,
    pub versioning: MavenVersioning,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenVersioning {
    pub latest: Option<String>,
    pub release: Option<String>,
    pub versions: MavenVersions,
    pub last_updated: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MavenVersions {
    #[serde(rename = "version", default)]
    pub values: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenPom {
    pub model_version: String,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub name: String,
    pub description: String,
}
