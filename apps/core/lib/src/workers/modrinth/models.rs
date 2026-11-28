use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Project, SearchPage, Version};

#[path = "auxiliary.rs"]
mod auxiliary;
pub use auxiliary::*;

#[derive(Debug)]
pub enum SearchResult {
    Offset(SearchPage),
    Pages(ModernSearchPage),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModernSearchPage {
    pub hits: Vec<ModernHit>,
    pub page: u32,
    pub hits_per_page: u8,
    pub total_hits: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModernHit {
    pub project_id: String,
    pub version_id: Option<String>,
    pub name: String,
    pub summary: String,
    pub slug: Option<String>,
    pub project_types: Vec<String>,
    pub downloads: u64,
    pub icon_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectCheck {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dependencies {
    pub projects: Vec<Project>,
    pub versions: Vec<Version>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub file_name: Option<String>,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyType {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

/// Public profile fields only; emails, account settings and payout data are not modeled.
#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub created: String,
    pub role: String,
    pub badges: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserHit {
    pub id: String,
    pub username: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Member {
    pub team_id: String,
    pub user: UserHit,
    pub role: String,
    pub ordering: Option<i32>,
    pub is_owner: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub team_id: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub members: Vec<Member>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModernProject {
    pub id: String,
    pub slug: Option<String>,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub project_types: Vec<String>,
    pub organization: Option<String>,
    pub icon_url: Option<String>,
    pub downloads: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllProjects {
    pub projects: Vec<ModernProject>,
    pub organizations: BTreeMap<String, Organization>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub user: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub projects: Vec<String>,
    pub status: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentPlan {
    pub primary: ResolvedContent,
    pub dependencies: Vec<ResolvedContent>,
    pub skipped: Vec<SkippedContent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResolvedContent {
    pub project_id: String,
    pub version_id: String,
    pub dependent_on_version_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkippedContent {
    pub project_id: String,
    pub version_id: Option<String>,
    pub dependent_on_version_id: Option<String>,
    pub reason: SkippedReason,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    AlreadyInstalled,
    DuplicateProject,
    ConflictingDependency,
    NoCompatibleVersion,
    MissingVersion,
    QuiltFabricApi,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Disclosures {
    pub disclosures: Vec<Disclosure>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Disclosure {
    #[serde(rename = "type")]
    pub kind: String,
    pub note: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub uses: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub interactions: Vec<String>,
    pub consent: Option<String>,
    #[serde(default)]
    pub data_collected: Vec<String>,
    #[serde(default)]
    pub sources: Vec<DerivativeSource>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DerivativeSource {
    pub label: String,
    pub link: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Category {
    pub icon: String,
    pub name: String,
    pub project_type: String,
    pub header: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Loader {
    pub icon: String,
    pub name: String,
    pub supported_project_types: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameVersion {
    pub version: String,
    #[serde(rename = "version_type", alias = "type")]
    pub kind: String,
    pub date: String,
    pub major: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct License {
    pub short: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LicenseText {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DonationPlatform {
    pub short: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LinkPlatform {
    pub name: String,
    pub donation: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoaderValue {
    pub value: String,
    pub ordering: Option<i32>,
    pub created: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub major: Option<bool>,
}
