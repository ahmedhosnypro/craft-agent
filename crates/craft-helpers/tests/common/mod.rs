// Shared corpus plumbing: fake fetch routes and throwaway fixture trees.
#![allow(clippy::unwrap_used)]
#![allow(clippy::panic)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use craft_helpers::error::HelperError;
use craft_helpers::fetch::{Fetch, FetchResponse};

pub struct FakeFetch {
    routes: HashMap<String, Result<FetchResponse, HelperError>>,
}

impl FakeFetch {
    pub fn new() -> Self {
        FakeFetch {
            routes: HashMap::new(),
        }
    }

    pub fn route(
        mut self,
        url: String,
        status: u16,
        body: String,
        retry_after: Option<String>,
    ) -> Self {
        let response = FetchResponse {
            status,
            body,
            retry_after,
        };
        self.routes.insert(url, Ok(response));
        self
    }
}

impl Default for FakeFetch {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for FakeFetch {
    fn get(&self, url: &str) -> Result<FetchResponse, HelperError> {
        match self.routes.get(url) {
            Some(result) => result.clone(),
            None => Err(HelperError::Http(format!("no route for {url}"))),
        }
    }
}

/// A recorded REST fixture body from tests/fixtures/prs.
pub fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/prs/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("fixture {name}: {err}"))
}

static TREE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A fresh throwaway tree under the system temp dir; `tag` keeps suites apart.
pub fn temp_tree(tag: &str) -> PathBuf {
    let seq = TREE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("craft-helpers-{tag}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write `text` at `dir/rel`, creating parents as needed.
pub fn put(dir: &std::path::Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

/// Write raw bytes at `dir/rel` (invalid UTF-8 fixtures included).
pub fn put_bytes(dir: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}
