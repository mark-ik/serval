/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! CORS preflight: send an `OPTIONS` and verify it grants the actual request.
//!
//! The preflight goes through the context's transport seam like every other
//! hop, so a host-supplied transport sees it and a build without a transport
//! cannot issue one.

use url::Url;

use crate::FetchContext;
use crate::cors;
use crate::referrer;
use crate::request::{Credentials, Method, ReferrerPolicy};
use crate::transport::WireRequest;

use super::USER_AGENT;

/// Send a CORS preflight `OPTIONS` and verify it. `Some(max_age)` if the actual
/// request is permitted, `None` if denied (or the preflight itself failed).
pub(super) async fn run_preflight(
    cx: &FetchContext,
    target: &Url,
    origin: Option<&url::Origin>,
    method: &Method,
    requested_headers: &[String],
    credentials: Credentials,
    referrer: Option<&Url>,
    referrer_policy: ReferrerPolicy,
) -> Option<u64> {
    let mut headers = vec![
        ("accept".to_owned(), "*/*".to_owned()),
        ("user-agent".to_owned(), USER_AGENT.to_owned()),
        (
            "access-control-request-method".to_owned(),
            method.as_str().to_owned(),
        ),
    ];
    if let Some(o) = origin {
        headers.push(("origin".to_owned(), o.ascii_serialization()));
    }
    // The preflight carries the request's referrer under its policy (same as the
    // actual request would for this target).
    if let Some(r) = referrer {
        if let Some(value) = referrer::referrer_header(r, target, referrer_policy) {
            headers.push(("referer".to_owned(), value.to_string()));
        }
    }
    if !requested_headers.is_empty() {
        headers.push((
            "access-control-request-headers".to_owned(),
            requested_headers.join(","),
        ));
    }
    let raw = cx
        .transport
        .send(WireRequest {
            url: target.clone(),
            method: Method::Options,
            headers,
            body: None,
            prefer_h3: false,
        })
        .await?;
    // The preflight response must have an ok (2xx) status; a redirect or error
    // status is a network error (WHATWG CORS-preflight fetch). The transport does
    // not follow redirects, so a 3xx is delivered as-is and rejected.
    if !(200..300).contains(&raw.status) {
        return None;
    }
    cors::preflight_verdict(origin, credentials, method, requested_headers, &raw.headers)
}
