use axlauncher_worker::modrinth::{CatalogError, Operation, Payload};
use serde::Serialize;
use url::Url;

use super::{Decoder, Route, Selection, get, json_string, parameter, parse, path};

pub(super) fn select(origin: &Url, operation: &Operation) -> Selection {
    Some(match operation {
        Operation::User(key) => Ok(get!(origin, ["v2", "user", key.as_str()], User)),
        Operation::Users(keys) => batch(origin, &["v2", "users"], keys, |body| {
            parse(body).map(Payload::Users)
        }),
        Operation::SearchUsers(query) => {
            let mut url = path(origin, &["v3", "users", "search"]);
            parameter(&mut url, "query", query);
            Ok(Route::get(url, |body| {
                parse(body).map(Payload::SearchUsers)
            }))
        }
        Operation::UserProjects(key) => Ok(get!(
            origin,
            ["v2", "user", key.as_str(), "projects"],
            Projects
        )),
        Operation::UserAllProjects(key) => Ok(get!(
            origin,
            ["v3", "user", key.as_str(), "all-projects"],
            AllProjects
        )),
        Operation::ProjectMembers(key) => Ok(get!(
            origin,
            ["v2", "project", key.as_str(), "members"],
            Members
        )),
        Operation::TeamMembers(id) => Ok(get!(
            origin,
            ["v2", "team", id.as_str(), "members"],
            Members
        )),
        Operation::Teams(ids) => batch(origin, &["v2", "teams"], ids, |body| {
            parse(body).map(Payload::Teams)
        }),
        _ => return groups(origin, operation),
    })
}

fn groups(origin: &Url, operation: &Operation) -> Selection {
    Some(match operation {
        Operation::Organization(key) => Ok(get!(
            origin,
            ["v3", "organization", key.as_str()],
            Organization
        )),
        Operation::Organizations(keys) => batch(origin, &["v3", "organizations"], keys, |body| {
            parse(body).map(Payload::Organizations)
        }),
        Operation::ProjectOrganization(key) => Ok(get!(
            origin,
            ["v3", "project", key.as_str(), "organization"],
            Organization
        )),
        Operation::OrganizationProjects(key) => Ok(get!(
            origin,
            ["v3", "organization", key.as_str(), "projects"],
            OrganizationProjects
        )),
        Operation::OrganizationMembers(key) => Ok(get!(
            origin,
            ["v3", "organization", key.as_str(), "members"],
            Members
        )),
        Operation::UserOrganizations(key) => Ok(get!(
            origin,
            ["v3", "user", key.as_str(), "organizations"],
            Organizations
        )),
        Operation::Collection(id) => {
            Ok(get!(origin, ["v3", "collection", id.as_str()], Collection))
        }
        Operation::Collections(ids) => batch(origin, &["v3", "collections"], ids, |body| {
            parse(body).map(Payload::Collections)
        }),
        Operation::UserCollections(key) => Ok(get!(
            origin,
            ["v3", "user", key.as_str(), "collections"],
            Collections
        )),
        _ => return None,
    })
}

fn batch(
    origin: &Url,
    route: &[&str],
    keys: &impl Serialize,
    decode: Decoder,
) -> Result<Route, CatalogError> {
    let mut url = path(origin, route);
    parameter(&mut url, "ids", json_string(keys)?);
    Ok(Route::get(url, decode))
}
