use axlauncher_worker::modrinth::{
    CatalogError, Operation, Payload, Search, SearchMethod, VersionFilters, VersionType,
};
use serde::Serialize;
use url::Url;

use super::{Route, Selection, get, json_string, parameter, parse, path};

pub(super) fn select(origin: &Url, operation: &Operation) -> Selection {
    Some(match operation {
        Operation::Search(search) => search_route(origin, search),
        Operation::Project(key) => Ok(get!(origin, ["v2", "project", key.as_str()], Project)),
        Operation::Projects(keys) => batch(origin, "projects", keys, |body| {
            parse(body).map(Payload::Projects)
        }),
        Operation::RandomProjects(count) => {
            let mut url = path(origin, &["v2", "projects_random"]);
            parameter(&mut url, "count", count);
            Ok(Route::get(url, |body| parse(body).map(Payload::Projects)))
        }
        Operation::CheckProject(key) => Ok(get!(
            origin,
            ["v2", "project", key.as_str(), "check"],
            ProjectCheck
        )),
        Operation::Dependencies(key) => Ok(get!(
            origin,
            ["v2", "project", key.as_str(), "dependencies"],
            Dependencies
        )),
        Operation::Versions(query) => {
            let url = path(
                origin,
                &["v2", "project", query.project.as_str(), "version"],
            );
            versions(url, &query.filters)
        }
        Operation::Version(id) => Ok(get!(origin, ["v2", "version", id.as_str()], Version)),
        Operation::ProjectVersion(project, key) => Ok(get!(
            origin,
            ["v2", "project", project.as_str(), "version", key.as_str()],
            Version
        )),
        Operation::BatchVersions(ids) => batch(origin, "versions", ids, |body| {
            parse(body).map(Payload::Versions)
        }),
        Operation::FindVersions(query) => {
            let mut url = path(origin, &["v2", "versions"]);
            let ids =
                json_string(&query.projects).map(|value| parameter(&mut url, "project_ids", value));
            ids.and_then(|()| versions(url, &query.filters))
        }
        Operation::ResolveContent(query) => {
            Route::post(path(origin, &["v3", "content", "resolve"]), query, |body| {
                parse(body).map(Payload::Content)
            })
        }
        Operation::Disclosures(project) => Ok(get!(
            origin,
            ["v3", "project", project.as_str(), "disclosures"],
            Disclosures
        )),
        _ => return None,
    })
}

fn batch(
    origin: &Url,
    route: &str,
    keys: &impl Serialize,
    decode: super::Decoder,
) -> Result<Route, CatalogError> {
    let mut url = path(origin, &["v2", route]);
    parameter(&mut url, "ids", json_string(keys)?);
    Ok(Route::get(url, decode))
}

#[derive(Serialize)]
struct SearchBody<'a> {
    query: &'a str,
    offset: String,
    limit: String,
    index: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    facets: Option<String>,
}

fn search_route(origin: &Url, search: &Search) -> Result<Route, CatalogError> {
    let (query, offset, limit) = search.parameters();
    let facets = (!search.facets.is_empty())
        .then(|| {
            let terms: Vec<Vec<String>> = search
                .facets
                .iter()
                .map(|group| {
                    group
                        .iter()
                        .map(|facet| facet.expression(search.method))
                        .collect()
                })
                .collect();
            json_string(&terms)
        })
        .transpose()?;
    let index = search.sort.as_str();
    match search.method {
        SearchMethod::Get => {
            let mut url = path(origin, &["v2", "search"]);
            for (key, value) in [
                ("query", query),
                ("offset", &offset.to_string()),
                ("limit", &limit.to_string()),
                ("index", index),
            ] {
                parameter(&mut url, key, value);
            }
            if let Some(facets) = facets {
                parameter(&mut url, "facets", facets);
            }
            Ok(Route::get(url, |body| parse(body).map(Payload::Search)))
        }
        SearchMethod::Post => Route::post(
            path(origin, &["v3", "search"]),
            &SearchBody {
                query,
                offset: offset.to_string(),
                limit: limit.to_string(),
                index,
                facets,
            },
            |body| parse(body).map(Payload::ModernSearch),
        ),
    }
}

fn versions(mut url: Url, filters: &VersionFilters) -> Result<Route, CatalogError> {
    for (key, list) in [
        ("loaders", &filters.loaders),
        ("game_versions", &filters.game_versions),
    ] {
        if !list.is_empty() {
            parameter(&mut url, key, json_string(list)?);
        }
    }
    if let Some(featured) = filters.featured {
        parameter(&mut url, "featured", featured);
    }
    if let Some(channel) = filters.version_type {
        let name = match channel {
            VersionType::Release => "release",
            VersionType::Beta => "beta",
            VersionType::Alpha => "alpha",
        };
        parameter(&mut url, "version_type", name);
    }
    if let Some(limit) = filters.limit {
        parameter(&mut url, "limit", limit);
    }
    if let Some(offset) = filters.offset {
        parameter(&mut url, "offset", offset);
    }
    parameter(&mut url, "include_changelog", filters.include_changelog);
    Ok(Route::get(url, |body| parse(body).map(Payload::Versions)))
}
