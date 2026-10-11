//! The reqwest-backed Fetch: bearer auth, capped bodies, no retries.

use std::time::Duration;

use crate::error::{HelperError, bounded};
use crate::fetch::{Fetch, FetchResponse, MAX_BODY_BYTES};

pub struct HttpFetch {
    client: reqwest::Client,
    runtime: tokio::runtime::Runtime,
    token: String,
}

impl HttpFetch {
    pub fn new(token: &str) -> Result<Self, HelperError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("craft-helpers/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| HelperError::Http(bounded(&err.to_string())))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| HelperError::Http(bounded(&err.to_string())))?;
        Ok(HttpFetch {
            client,
            runtime,
            token: token.to_string(),
        })
    }

    async fn get_async(&self, url: &str) -> Result<FetchResponse, HelperError> {
        let mut response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|err| HelperError::Http(bounded(&err.to_string())))?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|err| HelperError::Http(bounded(&err.to_string())))?
        {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(HelperError::TooLarge(MAX_BODY_BYTES));
            }
            body.extend_from_slice(&chunk);
        }
        let body = String::from_utf8(body)
            .map_err(|_| HelperError::MalformedJson("body is not UTF-8".into()))?;
        Ok(FetchResponse {
            status: status.as_u16(),
            body,
            retry_after,
        })
    }
}

impl Fetch for HttpFetch {
    fn get(&self, url: &str) -> Result<FetchResponse, HelperError> {
        self.runtime.block_on(self.get_async(url))
    }
}
