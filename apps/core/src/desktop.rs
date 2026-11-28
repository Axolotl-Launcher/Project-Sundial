// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop application services and their wiring ports.

use std::future::Future;

pub mod catalog;
pub mod curseforge;
pub mod installation;
pub mod instance_settings;
pub mod java;
pub mod launching;
pub mod project_images;
pub mod sources;

pub mod accounts {
    pub use axlauncher_worker::DesktopAccounts;
    pub use axlauncher_worker::accounts::{
        Account, AccountId, AccountKey, AccountSnapshot, Action, Change, ChoiceId, LauncherAccount,
        LauncherId, Method, Persistence, Profile, Reply, SignInCommit,
    };
}

pub mod downloads {
    use axlauncher_download_spi::PackOrigins;
    use axlauncher_storage::DataDir;

    pub use axlauncher_worker::downloads::{Downloads, Route as DownloadRoute, stats};

    /// Resolves a pack's download policy without adding credentials to the caller's handle.
    pub fn for_pack(data: DataDir, origins: PackOrigins) -> Downloads {
        let downloads = Downloads::new(data, DownloadRoute::Pack(origins));
        if origins == PackOrigins::CurseForge {
            return downloads.with_curseforge_key_source(|| crate::secrets::curseforge().ok());
        }
        downloads
    }
}

pub mod modrinth {
    pub use axlauncher_worker::modrinth::{
        AllProjects, Amount, AnalyticsEvent, Batch, BulkUpdate, CatalogError, Category, Collection,
        CollectionId, Comparison, ContentPlan, ContentRequest, ContentType, CountryCode,
        Dependencies, DependencyType, DerivativeSource, DesktopModrinth, Disclosure, Disclosures,
        DonationPlatform, Facet, FacetField, ForgeLoader, ForgeUpdates, GameVersion,
        GameVersionFilter, GameVersionType, HashAlgorithm, HashBatch, HashLookup, IndividualUpdate,
        IndividualUpdates, License, LicenseId, LicenseText, LinkPlatform, Loader, LoaderFieldQuery,
        LoaderValue, MavenCoordinate, MavenMetadata, MavenPom, MavenResult, MavenVersioning,
        MavenVersions, Member, ModernHit, ModernProject, ModernSearchPage, ModrinthLease,
        Organization, PaymentInterval, PaymentMethod, PlatformRevenue, Project, ProjectCheck,
        ProjectHit, ProjectKey, ProjectVersions, Redirect, ResolutionPreferences, ResolvedContent,
        RevenuePoint, RevenueRange, Search, SearchMethod, SearchPage, SearchResult, SearchSort,
        SkippedContent, SkippedReason, Statistics, TeamId, UpdateFilters, User, UserHit, UserKey,
        Version, VersionDependency, VersionFile, VersionFilters, VersionHashes, VersionId,
        VersionKey, VersionSearch, VersionType,
    };
}

/// A desktop worker runs one operation with its own Tokio I/O driver.
fn on_worker<T>(operation: impl Future<Output = T>) -> std::io::Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(runtime.block_on(operation))
}
