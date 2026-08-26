// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared document-control capability vocabulary.
//!
//! Retained [`crate::DocumentSession`]s and hosted [`crate::WebSurface`]s
//! expose the same document-facing controls. Backend-specific surface features
//! stay in `surface_engine`; this module deliberately names only the controls a
//! browser can present uniformly for either kind of document.

use serde::{Deserialize, Serialize};

/// Availability of one capability.
///
/// `Partial` must describe the limit. `Unsupported` must say why a consumer
/// cannot use the control. This keeps product UI from treating a merely present
/// method as a working feature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityStatus {
    Supported,
    Unsupported { reason: String },
    Partial { detail: String },
}

impl CapabilityStatus {
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }
}

/// Document-facing controls available from a retained session or hosted page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentCapabilities {
    /// Text find, including stepping and clearing the current query.
    pub find_in_page: CapabilityStatus,
    /// Changing the page's own zoom, rather than scaling its host tile.
    pub page_zoom: CapabilityStatus,
    /// Capturing the current rendered page as an image.
    pub page_capture: CapabilityStatus,
    /// Reload/stop/history controls or their retained-session equivalent.
    pub navigation: CapabilityStatus,
}

impl Default for DocumentCapabilities {
    fn default() -> Self {
        Self {
            find_in_page: CapabilityStatus::unsupported("find in page is not wired"),
            page_zoom: CapabilityStatus::unsupported("page zoom is not wired"),
            page_capture: CapabilityStatus::unsupported("page capture is not wired"),
            navigation: CapabilityStatus::unsupported("document navigation controls are not wired"),
        }
    }
}

/// Compatibility spelling for document-facing capability consumers.
///
/// New generic capability APIs should use [`CapabilityStatus`].
pub type DocumentCapabilityStatus = CapabilityStatus;

/// Compatibility spelling for existing web-surface capability consumers.
pub type WebFeatureStatus = CapabilityStatus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_explicitly_unavailable() {
        let caps = DocumentCapabilities::default();
        assert!(matches!(
            caps.find_in_page,
            CapabilityStatus::Unsupported { .. }
        ));
        assert!(matches!(
            caps.page_zoom,
            CapabilityStatus::Unsupported { .. }
        ));
        assert!(matches!(
            caps.page_capture,
            CapabilityStatus::Unsupported { .. }
        ));
        assert!(matches!(
            caps.navigation,
            CapabilityStatus::Unsupported { .. }
        ));
    }

    #[test]
    fn legacy_web_status_spelling_is_the_shared_type() {
        let status: WebFeatureStatus = CapabilityStatus::Supported;
        assert_eq!(status, CapabilityStatus::Supported);
    }
}
