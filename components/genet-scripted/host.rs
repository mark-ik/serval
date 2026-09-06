/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Per-document capabilities installed before authored scripts execute.

#[cfg(feature = "livery")]
use script_engine_api::ScriptEngine;
#[cfg(feature = "livery")]
use script_runtime_api::Runtime;
use script_runtime_api::{FetchHandler, WebGlFactory};

/// Host capabilities for one live scripted document.
///
/// Construct a fresh value for each navigation. The runtime and its handlers
/// stay on the document's thread; a host factory may be shared across sessions.
/// Resource loading continues through [`crate::ResourceFetcher`], while page
/// `fetch()` uses its richer request, response, cancellation and delivery seam.
#[derive(Default)]
pub struct ScriptedDocumentOptions {
    pub fetch: Option<Box<dyn FetchHandler>>,
    pub webgl: Option<WebGlFactory>,
}

#[cfg(feature = "livery")]
impl ScriptedDocumentOptions {
    pub(crate) fn install<E: ScriptEngine>(self, runtime: &mut Runtime<E>) {
        if let Some(fetch) = self.fetch {
            runtime.set_fetch_handler(fetch);
        }
        if let Some(webgl) = self.webgl {
            runtime.set_webgl_factory(webgl);
        }
    }
}
