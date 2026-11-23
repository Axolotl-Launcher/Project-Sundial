// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! DNS routing for direct HTTP connections.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axlauncher_spi::FetchError;
use futures::StreamExt;
use futures::stream;
use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, RData, RecordType};
use reqwest::dns::{Addrs, Name as HttpName, Resolve, Resolving};
use reqwest::{Client, Url};

const DOH_CONTENT_TYPE: &str = "application/dns-message";
const MAX_DNS_MESSAGE: usize = u16::MAX as usize;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(4);
const DOH_TIMEOUT: Duration = Duration::from_secs(10);
const PARALLEL_UPSTREAMS: usize = 2;

/// An encrypted resolver and its optional bootstrap address.
#[derive(Debug, Clone)]
pub struct DohUpstream {
    pub endpoint: Url,
    pub bootstrap: Option<IpAddr>,
}

/// DNS upstreams for public domains and suffixes handled by the OS resolver.
#[derive(Debug, Clone)]
pub struct DohConfig {
    pub upstreams: Vec<DohUpstream>,
    pub system_suffixes: Vec<String>,
}

impl Default for DohConfig {
    fn default() -> Self {
        let upstreams = [
            "https://doh.pub/dns-query",
            "https://doh.pysio.online/dns-query",
            "https://cloudflare-dns.com/dns-query",
        ];
        Self {
            upstreams: upstreams
                .into_iter()
                .map(|endpoint| DohUpstream {
                    endpoint: Url::parse(endpoint).expect("valid built-in DNS endpoint"),
                    bootstrap: None,
                })
                .collect(),
            system_suffixes: Vec::new(),
        }
    }
}

pub(super) struct DohResolver(Arc<Resolver>);

struct Resolver {
    upstreams: Vec<Upstream>,
    system_suffixes: Vec<String>,
    cache: Mutex<HashMap<String, CachedAddresses>>,
}

struct Upstream {
    client: Client,
    endpoint: Url,
    score: AtomicI32,
}

struct CachedAddresses {
    addresses: Vec<SocketAddr>,
    expires: Instant,
}

impl DohResolver {
    pub(super) fn new(config: DohConfig) -> Result<Self, FetchError> {
        let upstreams = config
            .upstreams
            .into_iter()
            .map(Upstream::new)
            .collect::<Result<_, _>>()?;
        let system_suffixes = config
            .system_suffixes
            .into_iter()
            .map(|suffix| suffix.trim_matches('.').to_ascii_lowercase())
            .collect();
        Ok(Self(Arc::new(Resolver {
            upstreams,
            system_suffixes,
            cache: Mutex::default(),
        })))
    }
}

impl Resolve for DohResolver {
    fn resolve(&self, name: HttpName) -> Resolving {
        let resolver = self.0.clone();
        Box::pin(async move {
            let addresses = resolver.resolve(name.as_str()).await?;
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

impl Resolver {
    async fn resolve(&self, name: &str) -> io::Result<Vec<SocketAddr>> {
        let host = name.trim_end_matches('.').to_ascii_lowercase();
        if self.upstreams.is_empty() || self.uses_system_dns(&host) {
            return system_addresses(name).await;
        }
        let now = Instant::now();
        if let Some(cached) = self.cached(&host, now) {
            return Ok(cached);
        }
        let Ok(query_name) = Name::from_ascii(format!("{host}.")) else {
            return system_addresses(name).await;
        };
        if let Some(answer) = self.doh_addresses(&query_name).await {
            self.cache.lock().expect("DNS cache lock").insert(
                host,
                CachedAddresses {
                    addresses: answer.addresses.clone(),
                    expires: now + Duration::from_secs(u64::from(answer.ttl)),
                },
            );
            return Ok(answer.addresses);
        }
        system_addresses(name).await
    }

    async fn doh_addresses(&self, name: &Name) -> Option<Answer> {
        let ipv4 = self.race(name, RecordType::A);
        let ipv6 = self.race(name, RecordType::AAAA);
        // Finish each family separately so a slow AAAA lookup cannot discard a good A answer.
        let (ipv4, ipv6) = futures::join!(
            tokio::time::timeout(DOH_TIMEOUT, ipv4),
            tokio::time::timeout(DOH_TIMEOUT, ipv6),
        );
        combine(ipv4.ok().flatten(), ipv6.ok().flatten())
    }

    fn uses_system_dns(&self, host: &str) -> bool {
        !host.contains('.')
            || host.ends_with(".localhost")
            || host.ends_with(".local")
            || host == "home.arpa"
            || host.ends_with(".home.arpa")
            || self.system_suffixes.iter().any(|suffix| {
                host == suffix
                    || host
                        .strip_suffix(suffix)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
    }

    fn cached(&self, host: &str, now: Instant) -> Option<Vec<SocketAddr>> {
        let mut cache = self.cache.lock().expect("DNS cache lock");
        cache.retain(|_, entry| entry.expires > now);
        cache.get(host).map(|entry| entry.addresses.clone())
    }

    async fn race(&self, name: &Name, kind: RecordType) -> Option<Answer> {
        let mut candidates: Vec<_> = self.upstreams.iter().collect();
        candidates
            .sort_by_key(|upstream| std::cmp::Reverse(upstream.score.load(Ordering::Relaxed)));
        let mut lookups = Vec::with_capacity(candidates.len());
        for upstream in candidates {
            lookups.push(upstream.lookup(name, kind));
        }
        first_answer(lookups).await
    }
}

async fn first_answer(
    lookups: impl IntoIterator<Item = impl Future<Output = Option<Answer>>>,
) -> Option<Answer> {
    let answers = stream::iter(lookups)
        .buffer_unordered(PARALLEL_UPSTREAMS)
        .filter_map(std::future::ready);
    std::pin::pin!(answers).next().await
}

async fn system_addresses(host: &str) -> io::Result<Vec<SocketAddr>> {
    Ok(tokio::net::lookup_host((host, 0)).await?.collect())
}

struct Answer {
    addresses: Vec<SocketAddr>,
    ttl: u32,
}

fn combine(first: Option<Answer>, second: Option<Answer>) -> Option<Answer> {
    match (first, second) {
        (Some(mut first), Some(second)) => {
            first.addresses.extend(second.addresses);
            first.ttl = first.ttl.min(second.ttl);
            Some(first)
        }
        (answer, None) | (None, answer) => answer,
    }
}

impl Upstream {
    fn new(config: DohUpstream) -> Result<Self, FetchError> {
        let endpoint = config.endpoint;
        let Some(host) = endpoint.host_str().filter(|_| endpoint.scheme() == "https") else {
            return Err(FetchError::Fatal("DNS upstream must use HTTPS".into()));
        };
        // The bootstrap connection must resolve independently of this resolver.
        let mut builder = Client::builder()
            .no_proxy()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(UPSTREAM_TIMEOUT);
        if let Some(address) = config.bootstrap {
            builder = builder.resolve(host, SocketAddr::new(address, 0));
        }
        let client = builder
            .build()
            .map_err(|error| FetchError::Fatal(error.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            score: AtomicI32::new(0),
        })
    }

    async fn lookup(&self, name: &Name, kind: RecordType) -> Option<Answer> {
        let answer = self.query(name, kind).await?;
        let _ = self
            .score
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |score| {
                score.checked_add(1)
            });
        Some(answer)
    }

    async fn query(&self, name: &Name, kind: RecordType) -> Option<Answer> {
        let question = Query::query(name.clone(), kind);
        // RFC 8484 recommends ID zero because HTTP correlates the request and reply.
        let mut message = Message::new(0, MessageType::Query, OpCode::Query);
        message.metadata.recursion_desired = true;
        message.add_query(question.clone());
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::CONTENT_TYPE, DOH_CONTENT_TYPE)
            .header(reqwest::header::ACCEPT, DOH_CONTENT_TYPE)
            .body(message.to_vec().ok()?)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let wire = read_wire(response).await?;
        let response = Message::from_vec(&wire).ok()?;
        decode(response, &question)
    }
}

async fn read_wire(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut wire = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if MAX_DNS_MESSAGE.saturating_sub(wire.len()) < chunk.len() {
            return None;
        }
        wire.extend_from_slice(&chunk);
    }
    Some(wire)
}

fn decode(message: Message, question: &Query) -> Option<Answer> {
    let meta = message.metadata;
    if meta.id != 0
        || meta.message_type != MessageType::Response
        || meta.response_code != ResponseCode::NoError
        || meta.truncation
        || message.queries.as_slice() != std::slice::from_ref(question)
    {
        return None;
    }
    let mut addresses = Vec::new();
    let mut ttl = u32::MAX;
    let mut name = question.name();
    for _ in 0..=message.answers.len() {
        let records = message.answers.iter().filter(|record| &record.name == name);
        let candidates = records.clone().filter_map(|record| {
            let address = answer_address(&record.data, question.query_type())?;
            ttl = ttl.min(record.ttl);
            Some(SocketAddr::new(address, 0))
        });
        addresses.extend(candidates);
        if !addresses.is_empty() {
            return Some(Answer { addresses, ttl });
        }
        let (alias, alias_ttl) = records
            .filter_map(|record| match &record.data {
                RData::CNAME(target) => Some((&target.0, record.ttl)),
                _ => None,
            })
            .next()?;
        name = alias;
        ttl = ttl.min(alias_ttl);
    }
    None
}

fn answer_address(record: &RData, query: RecordType) -> Option<IpAddr> {
    match (record, query) {
        (RData::A(address), RecordType::A) => Some(address.0.into()),
        (RData::AAAA(address), RecordType::AAAA) => Some(address.0.into()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/dns.rs"]
mod tests;
