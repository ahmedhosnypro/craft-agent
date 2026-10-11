//! The transport seam: production uses reqwest, the corpus fakes it.

use crate::error::HelperError;

/// Cap on any single response body; anything larger is a typed error.
pub const MAX_BODY_BYTES: usize = 4 << 20;

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<String>,
}

/// GET one JSON document. Implementations carry their own auth, cap the
/// body, and never retry; a 403/429 with Retry-After is surfaced so the
/// caller decides.
pub trait Fetch {
    fn get(&self, url: &str) -> Result<FetchResponse, HelperError>;
}
