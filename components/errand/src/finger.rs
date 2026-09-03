// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The finger protocol (`finger://`, port 79, RFC 1288), via the
//! [`finger-protocol`](https://crates.io/crates/finger-protocol) crate.
//!
//! That crate owns the transaction and the URL's user resolution
//! (`finger://host/user` and `finger://user@host` both name a user); this
//! module maps its reply onto errand's [`Response`]. There is no status line,
//! so the reply is always [`Status::Success`] as `text/plain`.
//!
//! errand takes that crate without its `webfinger` feature: WebFinger is
//! finger's successor but it rides on HTTP, and errand does not speak HTTP.

use url::Url;

use crate::{Error, Response, Status};

/// Fetch a `finger://` URL.
pub(crate) async fn fetch(url: &Url) -> Result<Response, Error> {
    let reply = finger_protocol::fetch(url.as_str())
        .await
        .map_err(map_error)?;
    Ok(Response {
        url: url.clone(),
        status: Status::Success,
        raw_status: None,
        meta: "text/plain".to_string(),
        body: reply.body,
    })
}

fn map_error(error: finger_protocol::ClientError) -> Error {
    use finger_protocol::ClientError as Finger;
    match error {
        Finger::BadUrl(message) => Error::BadUrl(message),
        Finger::Connect(message) => Error::Connect(message),
        Finger::Io(message) => Error::Io(message),
    }
}
