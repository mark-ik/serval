/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Fetch integration tests (mockito-backed) for the public `fetch()` entry.

use super::mixed_content::is_same_site;
use super::*;
use crate::SameSiteContext;
use crate::cache::{InMemoryHttpCache, NoHttpCache};
use crate::context::{AllowAllCsp, CookieStore};
use crate::cookie_jar::InMemoryCookieJar;
use crate::request::RedirectMode;
use crate::response::ResponseType;
use std::sync::{Arc, Mutex};
use url::Url;

/// A context with a real in-memory cache (and jar) wired.
fn caching_cx() -> FetchContext {
    FetchContext {
        cookies: Box::new(InMemoryCookieJar::new()),
        cache: std::sync::Arc::new(InMemoryHttpCache::new()),
        csp: Box::new(AllowAllCsp),
        hsts: Box::new(crate::InMemoryHsts::new()),
        preflight: Box::new(crate::InMemoryPreflightCache::new()),
        alt_svc: Box::new(crate::InMemoryAltSvc::new()),
        transport: crate::transport::default_transport(),
        redirect_limit: FetchContext::DEFAULT_REDIRECT_LIMIT,
        cache_max_body_bytes: FetchContext::DEFAULT_CACHE_MAX_BODY_BYTES,
    }
}

#[tokio::test]
async fn basic_get_returns_status_and_body() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/hello")
        .with_status(200)
        .with_body("hi there")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let url = format!("{}/hello", server.url());
    let res = fetch(Request::get(url.parse().unwrap()), &cx).await;

    m.assert_async().await;
    assert!(!res.is_network_error());
    assert_eq!(res.status, 200);
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"hi there");
}

#[tokio::test]
async fn follows_a_redirect_and_records_the_chain() {
    let mut server = mockito::Server::new_async().await;
    let _r1 = server
        .mock("GET", "/a")
        .with_status(302)
        .with_header("location", "/b")
        .create_async()
        .await;
    let _r2 = server
        .mock("GET", "/b")
        .with_status(200)
        .with_body("arrived")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let res = fetch(
        Request::get(format!("{}/a", server.url()).parse().unwrap()),
        &cx,
    )
    .await;

    assert_eq!(res.status, 200);
    assert_eq!(res.url_list.len(), 2, "original + redirect target");
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"arrived");
}

#[tokio::test]
async fn redirect_error_mode_yields_network_error() {
    let mut server = mockito::Server::new_async().await;
    let _r = server
        .mock("GET", "/a")
        .with_status(302)
        .with_header("location", "/b")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let mut req = Request::get(format!("{}/a", server.url()).parse().unwrap());
    req.redirect = RedirectMode::Error;
    let res = fetch(req, &cx).await;

    assert!(res.is_network_error());
}

#[tokio::test]
async fn decodes_gzip_content_encoding() {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(b"compressed hello").unwrap();
    let gz = enc.finish().unwrap();

    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/gz")
        .with_status(200)
        .with_header("content-encoding", "gzip")
        .with_body(gz)
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let res = fetch(
        Request::get(format!("{}/gz", server.url()).parse().unwrap()),
        &cx,
    )
    .await;

    assert_eq!(res.bytes().await.unwrap().as_ref(), b"compressed hello");
}

#[tokio::test]
async fn records_set_cookie_through_the_jar() {
    #[derive(Clone, Default)]
    struct SpyJar(Arc<Mutex<Vec<String>>>);
    impl CookieStore for SpyJar {
        fn cookies_for(&self, _: &Url, _: SameSiteContext) -> Vec<String> {
            Vec::new()
        }
        fn set_cookie(&self, _: &Url, header: &str) {
            self.0.lock().unwrap().push(header.to_owned());
        }
    }

    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/c")
        .with_status(200)
        .with_header("set-cookie", "id=abc")
        .with_body("ok")
        .create_async()
        .await;

    let spy = SpyJar::default();
    let cx = FetchContext {
        cookies: Box::new(spy.clone()),
        cache: std::sync::Arc::new(NoHttpCache),
        csp: Box::new(AllowAllCsp),
        hsts: Box::new(crate::InMemoryHsts::new()),
        preflight: Box::new(crate::InMemoryPreflightCache::new()),
        alt_svc: Box::new(crate::InMemoryAltSvc::new()),
        transport: crate::transport::default_transport(),
        redirect_limit: FetchContext::DEFAULT_REDIRECT_LIMIT,
        cache_max_body_bytes: FetchContext::DEFAULT_CACHE_MAX_BODY_BYTES,
    };
    let res = fetch(
        Request::get(format!("{}/c", server.url()).parse().unwrap()),
        &cx,
    )
    .await;

    assert_eq!(res.status, 200);
    assert_eq!(spy.0.lock().unwrap().as_slice(), &["id=abc".to_string()]);
}

#[tokio::test]
async fn fresh_response_served_from_cache_without_a_second_request() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/cached")
        .with_status(200)
        .with_header("cache-control", "max-age=300")
        .with_body("v1")
        .expect(1) // one network hit despite two fetches
        .create_async()
        .await;

    let cx = caching_cx();
    let url = format!("{}/cached", server.url());
    let r1 = fetch(Request::get(url.parse().unwrap()), &cx).await;
    assert_eq!(r1.bytes().await.unwrap().as_ref(), b"v1");
    let r2 = fetch(Request::get(url.parse().unwrap()), &cx).await;
    assert_eq!(r2.bytes().await.unwrap().as_ref(), b"v1");

    m.assert_async().await; // exactly one upstream request
}

#[tokio::test]
async fn stale_entry_revalidates_via_304_and_serves_stored_body() {
    let mut server = mockito::Server::new_async().await;
    // Initial load: immediately stale (max-age=0), carries an ETag.
    let initial = server
        .mock("GET", "/r")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("cache-control", "max-age=0")
        .with_header("etag", "\"v1\"")
        .with_body("hello")
        .expect(1)
        .create_async()
        .await;
    // Conditional revalidation returns 304.
    let revalidated = server
        .mock("GET", "/r")
        .match_header("if-none-match", "\"v1\"")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let cx = caching_cx();
    let url = format!("{}/r", server.url());
    let r1 = fetch(Request::get(url.parse().unwrap()), &cx).await;
    assert_eq!(r1.bytes().await.unwrap().as_ref(), b"hello");
    let r2 = fetch(Request::get(url.parse().unwrap()), &cx).await;
    assert_eq!(r2.status, 200, "304 is served as the stored 200, not a 304");
    assert_eq!(r2.bytes().await.unwrap().as_ref(), b"hello");

    initial.assert_async().await;
    revalidated.assert_async().await;
}

#[tokio::test]
async fn no_store_response_is_not_cached() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/ns")
        .with_status(200)
        .with_header("cache-control", "no-store")
        .with_body("v1")
        .expect(2) // two fetches → two upstream hits
        .create_async()
        .await;

    let cx = caching_cx();
    let url = format!("{}/ns", server.url());
    let _ = fetch(Request::get(url.parse().unwrap()), &cx)
        .await
        .bytes()
        .await;
    let _ = fetch(Request::get(url.parse().unwrap()), &cx)
        .await
        .bytes()
        .await;

    m.assert_async().await;
}

fn origin_of(s: &str) -> url::Origin {
    Url::parse(s).unwrap().origin()
}

#[tokio::test]
async fn cross_origin_cors_pass_taints_cors() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/api")
        .with_status(200)
        .with_header("access-control-allow-origin", "http://app.example")
        .with_body("data")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let req = Request::get(format!("{}/api", server.url()).parse().unwrap())
        .with_origin(origin_of("http://app.example/"));
    let res = fetch(req, &cx).await;

    assert_eq!(res.response_type, ResponseType::Cors);
    assert!(!res.is_network_error());
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"data");
}

#[tokio::test]
async fn cross_origin_cors_without_header_is_blocked() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/api")
        .with_status(200)
        .with_body("data")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let req = Request::get(format!("{}/api", server.url()).parse().unwrap())
        .with_origin(origin_of("http://app.example/"));
    let res = fetch(req, &cx).await;

    assert!(
        res.is_network_error(),
        "cross-origin CORS with no ACAO is blocked"
    );
}

#[tokio::test]
async fn cross_origin_no_cors_is_opaque() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/img")
        .with_status(200)
        .with_body("data")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let mut req = Request::get(format!("{}/img", server.url()).parse().unwrap())
        .with_origin(origin_of("http://app.example/"));
    req.mode = crate::RequestMode::NoCors;
    let res = fetch(req, &cx).await;

    assert_eq!(res.response_type, ResponseType::Opaque);
}

#[test]
fn mixed_content_active_passive_split() {
    use crate::Destination;
    let cx = FetchContext::permissive();

    // Optionally-blockable (image) in a secure context → auto-upgraded, allowed.
    let mut img: Url = "http://example.org/x.png".parse().unwrap();
    assert!(!resolve_mixed_content(
        &mut img,
        Destination::Image,
        true,
        &cx
    ));
    assert_eq!(img.scheme(), "https", "passive mixed content is upgraded");

    // Blockable (script, and the empty fetch() destination) in a secure
    // context → blocked.
    let mut script: Url = "http://example.org/a.js".parse().unwrap();
    assert!(resolve_mixed_content(
        &mut script,
        Destination::Other,
        true,
        &cx
    ));
    let mut xhr: Url = "http://example.org/api".parse().unwrap();
    assert!(resolve_mixed_content(
        &mut xhr,
        Destination::None,
        true,
        &cx
    ));

    // No secure context, no HSTS → plain http left as-is, not blocked.
    let mut insecure: Url = "http://example.org/a.js".parse().unwrap();
    assert!(!resolve_mixed_content(
        &mut insecure,
        Destination::Other,
        false,
        &cx
    ));
    assert_eq!(insecure.scheme(), "http");
}

#[test]
fn same_site_by_public_suffix_list() {
    let target: Url = "https://api.example.org/x".parse().unwrap();
    assert!(is_same_site(
        Some(&origin_of("https://www.example.org/")),
        &target
    ));
    assert!(!is_same_site(
        Some(&origin_of("https://other.example/")),
        &target
    ));
    assert!(is_same_site(None, &target), "no initiator is same-site");

    // PSL edge the last-two-labels approximation got wrong: github.io is a
    // public suffix, so two users' subdomains are *different* sites.
    let pages: Url = "https://alice.github.io/x".parse().unwrap();
    assert!(
        !is_same_site(Some(&origin_of("https://bob.github.io/")), &pages),
        "distinct github.io subdomains are cross-site"
    );
    assert!(is_same_site(
        Some(&origin_of("https://alice.github.io/y")),
        &pages
    ));
}

#[tokio::test]
async fn preflight_allows_then_sends_actual_request() {
    let mut server = mockito::Server::new_async().await;
    let options = server
        .mock("OPTIONS", "/x")
        .match_header("access-control-request-method", "PUT")
        .with_status(204)
        .with_header("access-control-allow-origin", "http://app.example")
        .with_header("access-control-allow-methods", "PUT")
        .with_header("access-control-max-age", "600")
        .expect(1)
        .create_async()
        .await;
    let actual = server
        .mock("PUT", "/x")
        .with_status(200)
        .with_header("access-control-allow-origin", "http://app.example")
        .with_body("done")
        .expect(1)
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let mut req = Request::get(format!("{}/x", server.url()).parse().unwrap())
        .with_origin(origin_of("http://app.example/"));
    req.method = Method::Put;
    let res = fetch(req, &cx).await;

    options.assert_async().await;
    actual.assert_async().await;
    assert_eq!(res.response_type, ResponseType::Cors);
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"done");
}

#[tokio::test]
async fn preflight_denial_blocks_without_sending_actual() {
    let mut server = mockito::Server::new_async().await;
    // OPTIONS allows the origin but not the method → denied.
    let options = server
        .mock("OPTIONS", "/x")
        .with_status(204)
        .with_header("access-control-allow-origin", "http://app.example")
        .with_header("access-control-allow-methods", "GET")
        .expect(1)
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let mut req = Request::get(format!("{}/x", server.url()).parse().unwrap())
        .with_origin(origin_of("http://app.example/"));
    req.method = Method::Put;
    let res = fetch(req, &cx).await;

    options.assert_async().await;
    assert!(
        res.is_network_error(),
        "preflight denial blocks the actual request"
    );
}

#[tokio::test]
async fn records_alt_svc_h3_advertisement() {
    #[derive(Clone, Default)]
    struct SpyAltSvc(Arc<Mutex<Option<u16>>>);
    impl crate::AltSvcStore for SpyAltSvc {
        fn h3_port(&self, _: &str) -> Option<u16> {
            *self.0.lock().unwrap()
        }
        fn record_h3(&self, _: &str, port: u16, _: u64) {
            *self.0.lock().unwrap() = Some(port);
        }
        fn clear(&self, _: &str) {
            *self.0.lock().unwrap() = None;
        }
    }

    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("alt-svc", "h3=\":443\"; ma=3600")
        .with_body("ok")
        .create_async()
        .await;

    let spy = SpyAltSvc::default();
    let cx = FetchContext {
        cookies: Box::new(InMemoryCookieJar::new()),
        cache: std::sync::Arc::new(NoHttpCache),
        csp: Box::new(AllowAllCsp),
        hsts: Box::new(crate::InMemoryHsts::new()),
        preflight: Box::new(crate::InMemoryPreflightCache::new()),
        alt_svc: Box::new(spy.clone()),
        transport: crate::transport::default_transport(),
        redirect_limit: FetchContext::DEFAULT_REDIRECT_LIMIT,
        cache_max_body_bytes: FetchContext::DEFAULT_CACHE_MAX_BODY_BYTES,
    };
    let _ = fetch(Request::get(server.url().parse().unwrap()), &cx)
        .await
        .bytes()
        .await;

    assert_eq!(
        *spy.0.lock().unwrap(),
        Some(443),
        "h3 advertisement recorded"
    );
}

#[tokio::test]
async fn cors_response_headers_are_filtered() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/data")
        .with_status(200)
        .with_header("access-control-allow-origin", "http://app.example")
        .with_header("content-type", "application/json")
        .with_header("x-secret", "leak")
        .with_body("{}")
        .create_async()
        .await;

    let cx = FetchContext::permissive();
    let req = Request::get(format!("{}/data", server.url()).parse().unwrap())
        .with_origin(origin_of("http://app.example/"));
    let res = fetch(req, &cx).await;

    assert_eq!(res.response_type, ResponseType::Cors);
    let names: Vec<&str> = res.headers.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"content-type"), "safelisted header kept");
    assert!(
        !names.iter().any(|n| n.eq_ignore_ascii_case("x-secret")),
        "non-exposed header filtered out"
    );
}

// The transport seam (platform boundary plan, P1): every hop, the CORS
// preflight included, crosses `FetchContext::transport`, and a context with no
// transport can only fail.

/// A transport that records every hop it is asked for and answers each with
/// the same canned response.
#[derive(Clone)]
struct SpyTransport {
    seen: Arc<Mutex<Vec<crate::transport::WireRequest>>>,
    status: u16,
    headers: Vec<(String, String)>,
}

impl crate::transport::Transport for SpyTransport {
    fn send(
        &self,
        request: crate::transport::WireRequest,
    ) -> crate::transport::TransportFuture<'_> {
        self.seen.lock().unwrap().push(request);
        let status = self.status;
        let headers = self.headers.clone();
        Box::pin(async move {
            Some(crate::transport::RawResponse::once(
                status,
                headers,
                bytes::Bytes::from_static(b"ok"),
            ))
        })
    }
}

#[tokio::test]
async fn every_hop_and_the_preflight_go_through_the_transport_seam() {
    let spy = SpyTransport {
        seen: Arc::default(),
        status: 200,
        headers: vec![
            (
                "access-control-allow-origin".to_owned(),
                "https://app.example".to_owned(),
            ),
            ("access-control-allow-methods".to_owned(), "PUT".to_owned()),
            (
                "access-control-allow-headers".to_owned(),
                "x-custom".to_owned(),
            ),
        ],
    };
    let cx = FetchContext::permissive().with_transport(Arc::new(spy.clone()));
    let mut req = Request::get("https://api.example/resource".parse().unwrap())
        .with_origin(origin_of("https://app.example/"));
    req.method = crate::request::Method::Put;
    req.headers.push(("x-custom".to_owned(), "1".to_owned()));

    let res = fetch(req, &cx).await;

    assert_eq!(res.status, 200);
    assert_eq!(res.response_type, ResponseType::Cors);
    let seen = spy.seen.lock().unwrap();
    let hops: Vec<(String, String)> = seen
        .iter()
        .map(|r| (r.method.as_str().to_owned(), r.url.to_string()))
        .collect();
    assert_eq!(
        hops,
        vec![
            (
                "OPTIONS".to_owned(),
                "https://api.example/resource".to_owned()
            ),
            ("PUT".to_owned(), "https://api.example/resource".to_owned()),
        ],
        "the preflight is a hop on the same seam as the request it guards"
    );
    let preflight = &seen[0];
    let has = |name: &str, value: &str| {
        preflight
            .headers
            .iter()
            .any(|(k, v)| k == name && v == value)
    };
    assert!(has("access-control-request-method", "PUT"));
    assert!(has("access-control-request-headers", "x-custom"));
    assert!(has("origin", "https://app.example"));
    assert!(preflight.body.is_none() && !preflight.prefer_h3);
}

#[tokio::test]
async fn a_context_without_a_transport_is_a_network_error() {
    let cx = FetchContext::permissive().with_transport(Arc::new(crate::transport::NoTransport));
    let res = fetch(
        Request::get("http://example.invalid/".parse().unwrap()),
        &cx,
    )
    .await;
    assert!(res.is_network_error());
}

#[tokio::test]
async fn the_cache_body_cap_is_the_contexts_policy() {
    use crate::cache::HttpCache;

    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/five")
        .with_status(200)
        .with_header("cache-control", "max-age=60")
        .with_body("hello")
        .create_async()
        .await;
    let url: Url = format!("{}/five", server.url()).parse().unwrap();
    let key = crate::cache::cache_key("GET", &url);

    for (cap, stored) in [(4u64, false), (5, true)] {
        let cache = Arc::new(InMemoryHttpCache::new());
        let mut cx = caching_cx().with_cache_max_body_bytes(cap);
        cx.cache = cache.clone();
        let _ = fetch(Request::get(url.clone()), &cx).await.bytes().await;
        assert_eq!(
            cache.get(&key).is_some(),
            stored,
            "a five-byte body against a cap of {cap}"
        );
    }
}
