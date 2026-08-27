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

pub mod navigation;
pub mod settings;
pub mod surface;
pub mod tile;

pub use navigation::resolve_href;
pub use surface::{
    ProviderId, SourceKindId, SurfaceAvailability, SurfaceDescriptor, SurfaceId,
    SurfaceSourceShape, SurfaceUnavailableReason,
};

/// Coarse engine selection for diagnostic hosts such as standalone Pelt.
///
/// Product hosts route individual content through Inker engine ids. This enum
/// deliberately names only the two HTML capabilities that still need a CLI
/// override; windowing mode, protocol, and historical implementation names are
/// separate concerns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EngineProfile {
    /// Script-free HTML through the owned Livery/Buckram engine. Its Inker
    /// identity is `genet.livery`.
    #[default]
    Livery,
    /// Live HTML whose scripts mutate the DOM while Livery/Buckram retain style,
    /// layout, and paint state.
    Scripted,
}

impl fmt::Display for EngineProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Livery => "livery",
            Self::Scripted => "scripted",
        };
        f.write_str(name)
    }
}

impl FromStr for EngineProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            // Keep the shipped command spellings as input aliases while making
            // their canonical identity truthful in logs and host state.
            "livery" | "viewer" | "static" => Ok(Self::Livery),
            "scripted" | "livery-scripted" => Ok(Self::Scripted),
            other => Err(format!(
                "unknown engine profile '{other}'; expected livery or scripted (legacy aliases: viewer, static, livery-scripted)"
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
            javascript: matches!(self.profile, EngineProfile::Scripted),
            ..ShellEngineCapabilities::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EngineProfile;

    #[test]
    fn legacy_profile_spellings_canonicalize_to_owned_engines() {
        assert_eq!("viewer".parse(), Ok(EngineProfile::Livery));
        assert_eq!("static".parse(), Ok(EngineProfile::Livery));
        assert_eq!("livery-scripted".parse(), Ok(EngineProfile::Scripted));
        assert_eq!(EngineProfile::default().to_string(), "livery");
    }

    #[test]
    fn retired_profiles_are_rejected() {
        assert!("browser".parse::<EngineProfile>().is_err());
        assert!("headless".parse::<EngineProfile>().is_err());
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
