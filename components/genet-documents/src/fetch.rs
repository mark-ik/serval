/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Host-owned resource fetching shared by every document engine.

#[cfg(feature = "netfetch")]
use std::sync::Arc;
use std::time::Duration;

use genet_host_api::{ResourceFetcher, ResourceResponse};

/// A local-scheme [`ResourceFetcher`]: `data:` decodes the inline payload,
/// `file://` (and a bare filesystem path) read from disk. Optional network
/// features add their schemes without selecting a styling engine.
pub struct LocalFetcher;

/// The host-owned policy shared by one remote document-resource client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceFetchPolicy {
    pub max_redirects: u32,
    pub max_concurrent_fetches: usize,
    pub max_response_bytes: usize,
    pub timeout: Duration,
}

impl Default for ResourceFetchPolicy {
    fn default() -> Self {
        Self {
            max_redirects: 20,
            max_concurrent_fetches: 6,
            max_response_bytes: 8 * 1024 * 1024,
            timeout: Duration::from_secs(15),
        }
    }
}

/// A `LocalFetcher` with a distinct shared HTTP cache, redirect cap, and
/// concurrency budget.
#[derive(Clone)]
pub struct ConfiguredLocalFetcher {
    #[cfg(feature = "netfetch")]
    http: Arc<crate::net_fetch::HttpResourceHost>,
}

impl LocalFetcher {
    pub fn with_resource_policy(policy: ResourceFetchPolicy) -> ConfiguredLocalFetcher {
        #[cfg(not(feature = "netfetch"))]
        let _ = policy;
        ConfiguredLocalFetcher {
            #[cfg(feature = "netfetch")]
            http: Arc::new(crate::net_fetch::HttpResourceHost::new(policy)),
        }
    }
}

impl ResourceFetcher for ConfiguredLocalFetcher {
    fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        self.fetch_response(url).map(|response| response.bytes)
    }

    fn fetch_response(&self, url: &str) -> Option<ResourceResponse> {
        #[cfg(feature = "netfetch")]
        if url.starts_with("http://") || url.starts_with("https://") {
            return self.http.fetch_response(url);
        }
        fetch_local_response(url)
    }
}

impl ResourceFetcher for LocalFetcher {
    fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        self.fetch_response(url).map(|response| response.bytes)
    }

    fn fetch_response(&self, url: &str) -> Option<ResourceResponse> {
        #[cfg(feature = "netfetch")]
        if url.starts_with("http://") || url.starts_with("https://") {
            return crate::net_fetch::default_http_resource_host().fetch_response(url);
        }
        fetch_local_response(url)
    }
}

fn fetch_local_response(url: &str) -> Option<ResourceResponse> {
    let bytes = {
        if url.starts_with("data:") {
            let parsed = data_url::DataUrl::process(url).ok()?;
            return parsed
                .decode_to_vec()
                .ok()
                .map(|(bytes, _fragment)| ResourceResponse::new(url, bytes));
        }
        #[cfg(feature = "netfetch")]
        if url.starts_with("http://") || url.starts_with("https://") {
            return crate::net_fetch::default_http_resource_host().fetch_response(url);
        }
        #[cfg(feature = "smolweb")]
        if url
            .split_once("://")
            .and_then(|(scheme, _)| errand::Scheme::parse(scheme))
            .is_some()
        {
            return crate::net_fetch::smolweb_get_bytes(url)
                .map(|bytes| ResourceResponse::new(url, bytes));
        }
        if let Some(rest) = url.strip_prefix("file://") {
            return std::fs::read(file_url_to_path(rest))
                .ok()
                .map(|bytes| ResourceResponse::new(url, bytes));
        }
        std::fs::read(url).ok()?
    };
    Some(ResourceResponse::new(url, bytes))
}

fn file_url_to_path(after_scheme: &str) -> String {
    let path = match after_scheme.split_once('/') {
        Some((auth, rest)) if auth.is_empty() || auth.eq_ignore_ascii_case("localhost") => {
            format!("/{rest}")
        },
        _ => after_scheme.to_string(),
    };
    #[cfg(windows)]
    if let Some(rest) = path.strip_prefix('/')
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return rest.to_string();
    }
    path
}

#[cfg(test)]
mod tests {
    use genet_host_api::ResourceFetcher;

    use super::LocalFetcher;

    #[test]
    fn missing_local_resource_is_a_clean_miss() {
        assert!(LocalFetcher.fetch("/no/such/pelt/file.html").is_none());
    }
}
