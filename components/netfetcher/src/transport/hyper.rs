/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The default transport: hyper 1 (h1/h2) over rustls with one process-wide
//! connection pool, plus the HTTP/3 lane (feature `h3`) tried first when the
//! origin advertised it and falling back to h1/h2 on any failure.
//!
//! One pool per process, lazily built. Per-context TLS configuration is a later
//! refinement; a host that needs per-session trust or partitioned pools supplies
//! its own [`Transport`].

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

use super::{RawResponse, Transport, TransportFuture, WireRequest};
use crate::request::Method;

/// Request body type for the client: a fully-buffered `Bytes` (empty for GET).
/// Streaming request bodies are a later increment.
type ReqBody = Full<Bytes>;

/// The connection-pooled, TLS-capable client type.
type HttpClient = Client<HttpsConnector<HttpConnector>, ReqBody>;

static CLIENT: OnceLock<HttpClient> = OnceLock::new();
static SHARED: OnceLock<Arc<DefaultTransport>> = OnceLock::new();

/// Whether the shared client should trust any server certificate. Set before the
/// first fetch (the client is built once, lazily).
static ACCEPT_INVALID_CERTS: AtomicBool = AtomicBool::new(false);

/// Make the default transport trust any server certificate, for a local test
/// harness driving a self-signed server (WPT). Must be called before the first
/// request; production never calls it. This is a knob on the default
/// transport only: a host-supplied [`Transport`] owns its own trust.
pub fn accept_invalid_certs() {
    ACCEPT_INVALID_CERTS.store(true, Ordering::Relaxed);
}

/// The default [`Transport`]. Obtain it through [`DefaultTransport::shared`];
/// every context built by [`crate::FetchContext::permissive`] holds it.
pub struct DefaultTransport {
    _private: (),
}

impl DefaultTransport {
    /// The process-wide default transport.
    pub fn shared() -> Arc<dyn Transport> {
        let shared: Arc<DefaultTransport> = SHARED
            .get_or_init(|| Arc::new(DefaultTransport { _private: () }))
            .clone();
        shared
    }
}

impl Transport for DefaultTransport {
    fn send(&self, request: WireRequest) -> TransportFuture<'_> {
        Box::pin(send(request))
    }
}

/// Send one request over h3 (if preferred and available) or h1/h2.
async fn send(request: WireRequest) -> Option<RawResponse> {
    #[cfg(all(feature = "h3", not(target_arch = "wasm32")))]
    if request.prefer_h3 {
        if let Some(h3) = crate::h3_client::fetch_h3_default(
            &request.url,
            http_method(&request.method),
            &request.headers,
            request.body.clone(),
        )
        .await
        {
            return Some(RawResponse::once(h3.status, h3.headers, h3.body));
        }
        // h3 attempt failed: fall back to h1/h2.
    }

    let uri = http::Uri::try_from(request.url.as_str()).ok()?;
    let mut builder = http::Request::builder()
        .method(http_method(&request.method))
        .uri(uri);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let req = builder
        .body(Full::new(request.body.unwrap_or_default()))
        .ok()?;
    let resp = shared_client().request(req).await.ok()?;
    let status = resp.status().as_u16();
    let headers = collect_headers(resp.headers());
    let data = resp
        .into_body()
        .into_data_stream()
        .map_err(io::Error::other);
    Some(RawResponse {
        status,
        headers,
        body: Box::pin(data),
    })
}

/// The process-wide client, built on first use.
fn shared_client() -> &'static HttpClient {
    CLIENT.get_or_init(build_client)
}

fn build_client() -> HttpClient {
    // rustls 0.23 needs a process-default CryptoProvider for the high-level
    // config builders hyper-rustls uses. Installing is idempotent-ish; it errors
    // if one is already installed, which we ignore.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = hyper_rustls::HttpsConnectorBuilder::new();
    // Escape hatch for local test harnesses (WPT's self-signed server): when
    // [`accept_invalid_certs`] was called, trust any server certificate. Off by
    // default; production uses the webpki trust anchors.
    let connector = if ACCEPT_INVALID_CERTS.load(Ordering::Relaxed) {
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify))
            .with_no_client_auth();
        // ALPN is set by enable_http1()/enable_http2() below; don't pre-define it.
        builder.with_tls_config(config)
    } else {
        builder.with_webpki_roots()
    };

    let https = connector
        .https_or_http() // allow plaintext http:// too (local/dev, smolweb)
        .enable_http1()
        .enable_http2()
        .build();

    Client::builder(TokioExecutor::new()).build(https)
}

pub(crate) fn http_method(method: &Method) -> http::Method {
    match method {
        Method::Get => http::Method::GET,
        Method::Head => http::Method::HEAD,
        Method::Post => http::Method::POST,
        Method::Put => http::Method::PUT,
        Method::Delete => http::Method::DELETE,
        Method::Patch => http::Method::PATCH,
        Method::Options => http::Method::OPTIONS,
        // A custom token: build an http::Method, falling back to GET if (somehow)
        // it isn't a valid method token.
        Method::Other(m) => http::Method::from_bytes(m.as_bytes()).unwrap_or(http::Method::GET),
    }
}

fn collect_headers(map: &http::HeaderMap) -> Vec<(String, String)> {
    map.iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.as_str().to_owned(), s.to_owned()))
        })
        .collect()
}

/// Accept any server certificate; armed by [`accept_invalid_certs`] for test
/// harnesses driving a self-signed local server only.
#[derive(Debug)]
struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &CertificateDer,
        _: &[CertificateDer],
        _: &ServerName,
        _: &[u8],
        _: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
