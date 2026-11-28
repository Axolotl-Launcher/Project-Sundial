use axlauncher_spi::curseforge::CatalogError;
use serde::Deserialize;
use url::Url;

const MAX_PAGE_SIZE: u8 = 50;
const MAX_RESULT_WINDOW: u32 = 10_000;
const MAX_QUERY_BYTES: usize = 512;

/// A bounded, zero-based page request for search or file listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRequest {
    index: u32,
    size: u8,
}

impl PageRequest {
    pub fn new(index: u32, size: u8) -> Result<Self, CatalogError> {
        if !(1..=MAX_PAGE_SIZE).contains(&size) || index > MAX_RESULT_WINDOW - u32::from(size) {
            return Err(CatalogError::InvalidRequest);
        }
        Ok(Self { index, size })
    }

    pub(super) fn append(self, url: &mut Url) {
        url.query_pairs_mut()
            .append_pair("index", &self.index.to_string())
            .append_pair("pageSize", &self.size.to_string());
    }
}

#[derive(Debug)]
pub struct Search {
    term: String,
    class_id: Option<u32>,
    page: PageRequest,
}

impl Search {
    /// An empty term browses the Minecraft catalog.
    pub fn new(term: impl Into<String>, page: PageRequest) -> Result<Self, CatalogError> {
        let term = term.into();
        if term.len() > MAX_QUERY_BYTES || term.chars().any(char::is_control) {
            return Err(CatalogError::InvalidRequest);
        }
        Ok(Self {
            term,
            class_id: None,
            page,
        })
    }

    pub fn with_class(mut self, class_id: u32) -> Result<Self, CatalogError> {
        super::validate_id(class_id)?;
        self.class_id = Some(class_id);
        Ok(self)
    }

    pub(super) fn page(&self) -> PageRequest {
        self.page
    }

    pub(super) fn append(&self, url: &mut Url, game_id: u32) {
        let mut query = url.query_pairs_mut();
        query.append_pair("gameId", &game_id.to_string());
        if !self.term.is_empty() {
            query.append_pair("searchFilter", &self.term);
        }
        if let Some(class_id) = self.class_id {
            query.append_pair("classId", &class_id.to_string());
        }
        drop(query);
        self.page.append(url);
    }
}

#[derive(Debug, Deserialize)]
pub struct Page<T> {
    #[serde(rename = "data")]
    pub items: Vec<T>,
    pagination: Pagination,
}

impl<T> Page<T> {
    pub fn total_count(&self) -> u32 {
        self.pagination.total_count
    }

    pub fn next_request(&self) -> Option<PageRequest> {
        let pagination = &self.pagination;
        let next = pagination.index.saturating_add(pagination.result_count);
        let remaining = MAX_RESULT_WINDOW
            .min(pagination.total_count)
            .saturating_sub(next);
        let size = u32::from(pagination.page_size).min(remaining) as u8;
        (pagination.result_count != 0)
            .then(|| PageRequest::new(next, size).ok())
            .flatten()
    }

    pub(super) fn validate(self, request: PageRequest) -> Result<Self, CatalogError> {
        let pagination = &self.pagination;
        if pagination.index != request.index
            || pagination.page_size != request.size
            || pagination.result_count as usize != self.items.len()
            || pagination.result_count > u32::from(request.size)
        {
            return Err(CatalogError::InvalidResponse);
        }
        Ok(self)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    index: u32,
    page_size: u8,
    result_count: u32,
    total_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: u32,
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub class_id: Option<u32>,
    pub parent_category_id: Option<u32>,
    pub is_class: Option<bool>,
}
