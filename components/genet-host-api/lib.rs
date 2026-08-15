/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Product-neutral host contracts for Genet.
//!
//! Hosts keep browser chrome, platform integration, protocol UI, and automation
//! routing distinct from whichever Genet engine profile they embed. Pelt is the
//! reference host, while Mere and Turnstone consume the same tile vocabulary.

use std::fmt;
use std::str::FromStr;

pub mod settings;
pub mod tile;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineProfile {
    /// Current all-up Servo browser engine: JS, DOM, layout, paint, webdriver.
    Browser,
    /// Future script-free static resource / document validation profile.
    Viewer,
    /// Alias for the script-free validation profile.
    Static,
    /// The opt-in clean-room Livery static engine. Its inker identity is
    /// `genet.livery`; selecting it never changes the incumbent static default.
    Livery,
    /// The scripted profile (V4): a live document whose inline `<script>` runs
    /// through `script-runtime-api` on a JS engine, mutating the DOM, rendered each
    /// frame. The content tier's proving ground (and the gc-arena soak's host).
    Scripted,
    /// Explicit scripted Livery route. The runtime owns the mutable DOM while
    /// Livery owns its CSSOM, resource graph, shaped layout, and paint session.
    /// Selecting it never changes either incumbent profile.
    LiveryScripted,
    /// Future automation-first profile. This is separate from `--headless`,
    /// which only selects the shell windowing mode.
    Headless,
}

impl EngineProfile {
    pub fn is_browser(self) -> bool {
        matches!(self, Self::Browser)
    }
}

impl Default for EngineProfile {
    fn default() -> Self {
        Self::Viewer
    }
}

impl fmt::Display for EngineProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Browser => "browser",
            Self::Viewer => "viewer",
            Self::Static => "static",
            Self::Livery => "livery",
            Self::Scripted => "scripted",
            Self::LiveryScripted => "livery-scripted",
            Self::Headless => "headless",
        };
        f.write_str(name)
    }
}

impl FromStr for EngineProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "browser" => Ok(Self::Browser),
            "viewer" => Ok(Self::Viewer),
            "static" => Ok(Self::Static),
            "livery" => Ok(Self::Livery),
            "scripted" => Ok(Self::Scripted),
            "livery-scripted" => Ok(Self::LiveryScripted),
            "headless" => Ok(Self::Headless),
            other => Err(format!(
                "unknown engine profile '{other}'; expected browser, viewer, static, livery, scripted, livery-scripted, or headless"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellEngineCapabilities {
    pub javascript: bool,
    pub webdriver: bool,
    pub devtools: bool,
    pub webgpu: bool,
    pub webxr: bool,
}

pub trait ShellEngine {
    fn profile(&self) -> EngineProfile;
    fn capabilities(&self) -> ShellEngineCapabilities;
}

pub struct DeferredShellEngine {
    profile: EngineProfile,
}

impl DeferredShellEngine {
    pub fn new(profile: EngineProfile) -> Self {
        Self { profile }
    }

    pub fn unavailable_message(&self) -> String {
        format!(
            "engine profile {} is reserved for a future Genet validation path",
            self.profile
        )
    }
}

impl ShellEngine for DeferredShellEngine {
    fn profile(&self) -> EngineProfile {
        self.profile
    }

    fn capabilities(&self) -> ShellEngineCapabilities {
        ShellEngineCapabilities {
            // The scripted profile runs JS; the other profiles are script-free today.
            javascript: matches!(
                self.profile,
                EngineProfile::Scripted | EngineProfile::LiveryScripted
            ),
            ..ShellEngineCapabilities::default()
        }
    }
}

/// One host-owned resource response. Engines receive the final identity and a
/// bounded metadata surface, never a transport handle or mutable cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceResponse {
    /// Identity after the host's redirect policy has finished.
    pub final_url: String,
    /// The response `Content-Type` when the host obtained one.
    pub content_type: Option<String>,
    /// Fully collected resource bytes. Streaming remains a host concern at this
    /// synchronous document-session boundary.
    pub bytes: Vec<u8>,
}

impl ResourceResponse {
    pub fn new(final_url: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            final_url: final_url.into(),
            content_type: None,
            bytes,
        }
    }

    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }
}

/// Host-shell resource-fetch contract: turn a URL into bytes for whichever engine
/// is hosted underneath. Networking is a *platform-integration* concern the shell
/// owns — kept off the engine, which only consumes host-owned response records.
/// Impls live in the ports (a local-file fetcher, a netfetcher-backed fetcher, …);
/// an engine's own byte seams (e.g. genet's `ImageLoader`) delegate to whichever
/// the shell supplies.
pub trait ResourceFetcher {
    /// Fetch `url` to bytes, or `None` on failure / unsupported scheme.
    fn fetch(&self, url: &str) -> Option<Vec<u8>>;

    /// Fetch `url` with its redirect-final identity and response type. Existing
    /// byte-only hosts remain valid: their requested URL is the final identity and
    /// their content type is unknown until they opt into this richer response.
    fn fetch_response(&self, url: &str) -> Option<ResourceResponse> {
        self.fetch(url)
            .map(|bytes| ResourceResponse::new(url, bytes))
    }
}
