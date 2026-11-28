use axlauncher_network::metadata::MetadataMethod;
use axlauncher_worker::modrinth::{
    CatalogError, ForgeLoader, HashAlgorithm, MavenCoordinate, MavenResult, Operation, Payload,
    RevenueRange, Tags,
};
use serde::de::DeserializeOwned;
use url::Url;

use super::{Format, Route, Selection, get, parameter, parse, path};

#[path = "routes_maven.rs"]
mod maven;

pub(super) fn select(origin: &Url, operation: &Operation) -> Selection {
    Some(match operation {
        Operation::Tags(value) => Ok(tags(origin, *value)),
        Operation::License(id) => Ok(get!(
            origin,
            ["v2", "tag", "license", id.as_str()],
            LicenseText
        )),
        Operation::GameVersions(filter) => {
            let mut url = path(origin, &["v2", "tag", "game_version"]);
            if let Some(kind) = filter.version_type {
                parameter(&mut url, "type", kind.as_str());
            }
            if let Some(major) = filter.major {
                parameter(&mut url, "major", major);
            }
            Ok(Route::get(url, |body| {
                parse(body).map(Payload::GameVersions)
            }))
        }
        Operation::LoaderField(query) => {
            let mut url = path(origin, &["v3", "loader_field"]);
            parameter(&mut url, "loader_field", query.as_str());
            Ok(Route::get(url, |body| {
                parse(body).map(Payload::LoaderValues)
            }))
        }
        Operation::ForgeUpdates(project, loader) => {
            let mut url = path(origin, &["updates", project.as_str(), "forge_updates.json"]);
            match loader {
                ForgeLoader::Forge => (),
                ForgeLoader::NeoForge => parameter(&mut url, "neoforge", "only"),
                ForgeLoader::Both => parameter(&mut url, "neoforge", "include"),
            }
            Ok(Route::get(url, |body| {
                parse(body).map(Payload::ForgeUpdates)
            }))
        }
        Operation::Statistics => Ok(get!(origin, ["v2", "statistics"], Statistics)),
        Operation::AnalyticsEvents => Ok(get!(origin, ["v3", "analytics-event"], AnalyticsEvents)),
        Operation::PaymentMethods(country) => {
            let mut url = path(origin, &["v3", "payout", "methods"]);
            if let Some(country) = country {
                parameter(&mut url, "country", country.as_str());
            }
            Ok(Route::get(url, |body| {
                parse(body).map(Payload::PaymentMethods)
            }))
        }
        Operation::PlatformRevenue(range) => revenue(origin, range),
        _ => return maven::select(origin, operation).map(Ok),
    })
}

fn revenue(origin: &Url, range: &RevenueRange) -> Result<Route, CatalogError> {
    let mut url = path(origin, &["v3", "payout", "platform_revenue"]);
    for (key, timestamp) in [("start", range.start), ("end", range.end)]
        .into_iter()
        .filter_map(|(key, timestamp)| Some((key, timestamp?)))
    {
        let timestamp = timestamp
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| CatalogError::InvalidRequest)?;
        parameter(&mut url, key, timestamp);
    }
    Ok(Route::get(url, |body| {
        parse(body).map(Payload::PlatformRevenue)
    }))
}

fn tags(origin: &Url, tags: Tags) -> Route {
    match tags {
        Tags::Categories => get!(origin, ["v2", "tag", "category"], Categories),
        Tags::Loaders => get!(origin, ["v2", "tag", "loader"], Loaders),
        Tags::Licenses => get!(origin, ["v2", "tag", "license"], Licenses),
        Tags::DonationPlatforms => get!(
            origin,
            ["v2", "tag", "donation_platform"],
            DonationPlatforms
        ),
        Tags::LinkPlatforms => get!(origin, ["v3", "link_platform"], LinkPlatforms),
        Tags::ReportTypes => get!(origin, ["v2", "tag", "report_type"], TagNames),
        Tags::ProjectTypes => get!(origin, ["v2", "tag", "project_type"], TagNames),
        Tags::SideTypes => get!(origin, ["v2", "tag", "side_type"], TagNames),
    }
}
