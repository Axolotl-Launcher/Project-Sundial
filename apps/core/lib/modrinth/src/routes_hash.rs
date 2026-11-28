use axlauncher_network::metadata::MetadataMethod;
use axlauncher_worker::modrinth::{
    HashAlgorithm, HashBatch, HashLookup, IndividualUpdates, Operation, Payload, UpdateFilters,
};
use serde::Serialize;
use url::Url;

use super::{Format, Route, Selection, parameter, parse, path};

pub(super) fn select(origin: &Url, operation: &Operation) -> Selection {
    Some(match operation {
        Operation::FileVersion(hash) => Ok(Route::get(file(origin, hash, None), |body| {
            parse(body).map(Payload::Version)
        })),
        Operation::FileDownload(hash) => Ok(Route {
            url: file(origin, hash, Some("download")),
            method: MetadataMethod::Get,
            response: Format::Redirect,
        }),
        Operation::FileUpdate(hash, filters) => {
            Route::post(file(origin, hash, Some("update")), filters, |body| {
                parse(body).map(Payload::Version)
            })
        }
        Operation::ProjectsByHash(batch) => Route::post(
            path(origin, &["v2", "version_file", "project"]),
            &hashes(batch),
            |body| parse(body).map(Payload::ProjectsByHash),
        ),
        Operation::VersionsByHash(batch) => Route::post(
            path(origin, &["v2", "version_files"]),
            &hashes(batch),
            |body| parse(body).map(Payload::VersionsByHash),
        ),
        Operation::UpdatesByHash(query) => Route::post(
            path(origin, &["v2", "version_files", "update"]),
            &UpdateBody {
                batch: hashes(&query.hashes),
                filters: &query.filters,
            },
            |body| parse(body).map(Payload::VersionsByHash),
        ),
        Operation::AllUpdatesByHash(query) => Route::post(
            path(origin, &["v2", "version_files", "update_many"]),
            &UpdateBody {
                batch: hashes(&query.hashes),
                filters: &query.filters,
            },
            |body| parse(body).map(Payload::UpdatesByHash),
        ),
        Operation::IndividualUpdates(query) => Route::post(
            path(origin, &["v2", "version_files", "update_individual"]),
            &individual(query),
            |body| parse(body).map(Payload::VersionsByHash),
        ),
        _ => return None,
    })
}

fn file(origin: &Url, lookup: &HashLookup, suffix: Option<&str>) -> Url {
    let hash = lookup.hash.to_ascii_lowercase();
    let segments: Vec<_> = ["v2", "version_file", hash.as_str()]
        .into_iter()
        .chain(suffix)
        .collect();
    let mut url = path(origin, &segments);
    parameter(&mut url, "algorithm", lookup.algorithm.as_str());
    if let Some(version) = &lookup.version {
        parameter(&mut url, "version_id", version.as_str());
    }
    url
}

#[derive(Serialize)]
struct HashBody<T> {
    hashes: Vec<T>,
    algorithm: HashAlgorithm,
}

#[derive(Serialize)]
struct UpdateBody<'a> {
    #[serde(flatten)]
    batch: HashBody<String>,
    #[serde(flatten)]
    filters: &'a UpdateFilters,
}

#[derive(Serialize)]
struct UpdateItem<'a> {
    hash: String,
    #[serde(flatten)]
    filters: &'a UpdateFilters,
}

fn hashes(query: &HashBatch) -> HashBody<String> {
    HashBody {
        hashes: query
            .hashes
            .items()
            .iter()
            .map(|hash| hash.to_ascii_lowercase())
            .collect(),
        algorithm: query.algorithm,
    }
}

fn individual(query: &IndividualUpdates) -> HashBody<UpdateItem<'_>> {
    HashBody {
        hashes: query
            .hashes
            .items()
            .iter()
            .map(|item| UpdateItem {
                hash: item.hash.to_ascii_lowercase(),
                filters: &item.filters,
            })
            .collect(),
        algorithm: query.algorithm,
    }
}
