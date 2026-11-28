use super::{
    CatalogError, DeserializeOwned, Format, HashAlgorithm, MavenCoordinate, MavenResult,
    MetadataMethod, Operation, Payload, Route, Url, path,
};

const XML: &str = "application/xml, text/xml";

pub(super) fn select(origin: &Url, operation: &Operation) -> Option<Route> {
    Some(match operation {
        Operation::MavenMetadata(project) => Route {
            url: path(
                origin,
                &[
                    "maven",
                    "maven",
                    "modrinth",
                    project.as_str(),
                    "maven-metadata.xml",
                ],
            ),
            method: MetadataMethod::Get,
            response: Format::Maven {
                accept: XML,
                decode_body: |body| {
                    xml(body).map(|data| Payload::MavenMetadata(MavenResult::Value(data)))
                },
                decode_redirect: |link| Payload::MavenMetadata(MavenResult::Redirect(link)),
            },
        },
        Operation::MavenArtifact(coordinate) => Route {
            url: resource(origin, coordinate, None),
            method: MetadataMethod::Get,
            response: Format::Maven {
                accept: XML,
                decode_body: |body| {
                    xml(body).map(|data| Payload::MavenArtifact(MavenResult::Value(data)))
                },
                decode_redirect: |link| Payload::MavenArtifact(MavenResult::Redirect(link)),
            },
        },
        Operation::MavenHead(coordinate) => Route {
            url: resource(origin, coordinate, None),
            method: MetadataMethod::Head,
            response: Format::Maven {
                accept: "*/*",
                decode_body: |_| Ok(Payload::MavenHead(MavenResult::Value(()))),
                decode_redirect: |link| Payload::MavenHead(MavenResult::Redirect(link)),
            },
        },
        Operation::MavenHash(coordinate, algorithm) => Route {
            url: resource(origin, coordinate, Some(*algorithm)),
            method: MetadataMethod::Get,
            response: Format::Maven {
                accept: "text/plain",
                decode_body: match algorithm {
                    HashAlgorithm::Sha1 => |body| checksum(body, HashAlgorithm::Sha1),
                    HashAlgorithm::Sha512 => |body| checksum(body, HashAlgorithm::Sha512),
                },
                decode_redirect: |link| Payload::MavenHash(MavenResult::Redirect(link)),
            },
        },
        _ => return None,
    })
}

fn resource(origin: &Url, info: &MavenCoordinate, hash: Option<HashAlgorithm>) -> Url {
    let filename = match hash {
        Some(hash) => format!("{}.{}", info.filename, hash.as_str()),
        None => info.filename.clone(),
    };
    path(
        origin,
        &[
            "maven",
            "maven",
            "modrinth",
            info.project.as_str(),
            info.version.as_str(),
            &filename,
        ],
    )
}

fn xml<T: DeserializeOwned>(body: &[u8]) -> Result<T, CatalogError> {
    let body = std::str::from_utf8(body).map_err(|_| CatalogError::InvalidResponse)?;
    quick_xml::de::from_str(body).map_err(|_| CatalogError::InvalidResponse)
}

fn checksum(body: &[u8], algorithm: HashAlgorithm) -> Result<Payload, CatalogError> {
    let body = std::str::from_utf8(body)
        .map_err(|_| CatalogError::InvalidResponse)?
        .trim();
    if !algorithm.accepts(body) {
        return Err(CatalogError::InvalidResponse);
    }
    Ok(Payload::MavenHash(MavenResult::Value(
        body.to_ascii_lowercase(),
    )))
}
