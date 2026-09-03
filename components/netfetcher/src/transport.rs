/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The transport seam: one wire hop, injected by the host.
//!
//! Everything the web can observe about a fetch is decided by the Fetch
//! algorithm before a request reaches this seam and after its response comes
//! back: redirects, cookies, CORS, caching, decoding, tainting. What crosses the
//! seam is one already-assembled request and one raw response. A [`Transport`]
//! therefore never follows redirects, never attaches or records cookies, and
//! never decodes a body; it owns connection pooling, TLS, trust anchors, client
//! certificates, proxies and timeouts, which is to say transport choice and
//! trust, the half a host owns.
//!
//! The default transport ([`hyper::DefaultTransport`], feature
//! `hyper-transport`, on by default) is what a raw host and the WPT harness use;
//! a build without it has no network and [`NoTransport`] answers every send with
//! a network error, which is how the Fetch semantics are proven to link no
//! transport at all.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use url::Url;

use crate::request::Method;
pub use crate::response::BodyStream;

#[cfg(feature = "hyper-transport")]
pub mod hyper;

/// One hop on the wire, after the Fetch algorithm has decided everything
/// observable about it: the exact URL, method, headers and body to send.
#[derive(Clone, Debug)]
pub struct WireRequest {
    pub url: Url,
    pub method: Method,
    /// Header names are ASCII-lowercase; a transport may not add, drop or reorder
    /// entries.
    pub headers: Vec<(String, String)>,
    /// A fully buffered body, empty or absent for GET and HEAD.
    pub body: Option<Bytes>,
    /// The origin advertised HTTP/3 via Alt-Svc. A transport with an h3 lane may
    /// try it first and fall back; one without ignores the hint.
    pub prefer_h3: bool,
}

/// The raw result of one hop: status, headers as received, and the undecoded
/// body stream. Produced identically by every transport so the loop's back half
/// is transport-agnostic.
pub struct RawResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: BodyStream,
}

impl RawResponse {
    /// A response whose whole body is already in hand, as a single-chunk stream.
    pub fn once(status: u16, headers: Vec<(String, String)>, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body: Box::pin(futures_util::stream::once(async move {
                Ok::<_, std::io::Error>(body)
            })),
        }
    }
}

/// The future a [`Transport::send`] returns. Boxed so the trait is object-safe
/// and a host can hold any transport as `Arc<dyn Transport>` beside the other
/// seams on [`crate::FetchContext`].
pub type TransportFuture<'a> = Pin<Box<dyn Future<Output = Option<RawResponse>> + Send + 'a>>;

/// Send one request and return its raw response, or `None` for a network
/// failure (which the Fetch algorithm turns into a network-error response).
pub trait Transport: Send + Sync {
    fn send(&self, request: WireRequest) -> TransportFuture<'_>;
}

/// A transport with no network: every send is a network failure. The default
/// when the crate is built without `hyper-transport`; also useful to a host
/// that wants a fetch context which can serve `data:` URLs and cache hits and
/// nothing else.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoTransport;

impl Transport for NoTransport {
    fn send(&self, _request: WireRequest) -> TransportFuture<'_> {
        Box::pin(async { None })
    }
}

/// The transport a [`crate::FetchContext::permissive`] context starts with.
pub(crate) fn default_transport() -> Arc<dyn Transport> {
    #[cfg(feature = "hyper-transport")]
    {
        hyper::DefaultTransport::shared()
    }
    #[cfg(not(feature = "hyper-transport"))]
    {
        Arc::new(NoTransport)
    }
}
