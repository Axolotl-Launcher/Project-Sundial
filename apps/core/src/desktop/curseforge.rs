//! Application entry point for CurseForge catalog lookups and pinned imports.

use std::sync::LazyLock;

use axlauncher_content::curseforge::Catalog;
use axlauncher_network::auth::AuthHttp;
use axlauncher_network_spi::auth::{ApiKey, AuthNetwork, TransportError};

pub use axlauncher_content::curseforge::{Category, Page, PageRequest, Search};
pub use axlauncher_spi::curseforge::{CatalogError, File, Mod, ModpackCatalog};

static CATALOG: LazyLock<Result<Catalog<AuthHttp>, CatalogError>> =
    LazyLock::new(|| build_catalog(crate::secrets::curseforge(), AuthHttp::desktop));

pub(crate) fn build_catalog<N: AuthNetwork>(
    key: Result<&ApiKey, CatalogError>,
    transport: impl FnOnce() -> Result<N, TransportError>,
) -> Result<Catalog<N>, CatalogError> {
    let key = key?;
    let transport = transport().map_err(|_| CatalogError::Transport)?;
    Ok(Catalog::new(transport, key.clone()))
}

fn catalog() -> Result<&'static Catalog<AuthHttp>, CatalogError> {
    CATALOG.as_ref().map_err(|error| *error)
}

/// A catalog capability; it does not own or return the launcher secret.
pub struct DesktopCurseForge;

impl DesktopCurseForge {
    pub async fn search(&self, search: Search) -> Result<Page<Mod>, CatalogError> {
        catalog()?.search(search).await
    }

    pub async fn get_mod(&self, mod_id: u32) -> Result<Mod, CatalogError> {
        catalog()?.get_mod(mod_id).await
    }

    pub async fn list_files(
        &self,
        mod_id: u32,
        page: PageRequest,
    ) -> Result<Page<File>, CatalogError> {
        catalog()?.list_files(mod_id, page).await
    }

    pub async fn get_file(&self, mod_id: u32, file_id: u32) -> Result<File, CatalogError> {
        catalog()?.get_file(mod_id, file_id).await
    }

    pub async fn download_url(
        &self,
        mod_id: u32,
        file_id: u32,
    ) -> Result<Option<url::Url>, CatalogError> {
        catalog()?.download_url(mod_id, file_id).await
    }

    /// Project descriptions are untrusted provider HTML.
    pub async fn description(&self, mod_id: u32) -> Result<String, CatalogError> {
        catalog()?.description(mod_id).await
    }

    /// File changelogs are untrusted provider HTML.
    pub async fn changelog(&self, mod_id: u32, file_id: u32) -> Result<String, CatalogError> {
        catalog()?.changelog(mod_id, file_id).await
    }

    pub async fn categories(&self, class_id: Option<u32>) -> Result<Vec<Category>, CatalogError> {
        catalog()?.categories(class_id).await
    }
}

impl ModpackCatalog for DesktopCurseForge {
    async fn get_mods(&self, ids: &[u32]) -> Result<Vec<Mod>, CatalogError> {
        catalog()?.get_mods(ids).await
    }

    async fn get_files(&self, ids: &[u32]) -> Result<Vec<File>, CatalogError> {
        catalog()?.get_files(ids).await
    }
}
