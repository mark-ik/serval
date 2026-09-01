// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Accessibility contract — the invariant every content surface declares about
//! what it can expose to the accessibility tree, plus the renderer-neutral
//! document projection that carries those semantics to a host.
//!
//! Adopted from the donor `graphshell` `SUBSYSTEM_ACCESSIBILITY.md` (per the
//! [docs harvest](../../../../design_docs/mere_docs/research/2026-05-27_graphshell_docs_full_harvest.md)
//! §1 and the [adoption roadmap](../../../../design_docs/mere_docs/implementation_strategy/2026-05-27_adoption_roadmap.md)
//! R0). This is the **contract** (the rule), not the full a11y implementation;
//! the host's AccessKit bridge (platen domains → uxtree) consumes it when the
//! a11y slice lands.
//!
//! ## The three invariants
//!
//! 1. **Capability-declaration** — every engine/surface declares its
//!    [`A11yCapability`] in *one* place ([`crate::Engine::a11y_capability`] /
//!    [`crate::SurfaceEngine::a11y_capability`]). The host never guesses a
//!    surface's accessibility from its kind; it reads the declaration.
//! 2. **Non-silent-degradation** — a surface that cannot expose its content
//!    *must* declare a lower capability ([`A11yCapability::Partial`] /
//!    [`A11yCapability::Opaque`]). It must never present as [`A11yCapability::Full`]
//!    while silently dropping semantics. Degradation is *declared*, never silent —
//!    so the host can surface "you can't inspect inside this" honestly (cf. the
//!    [scrying DOM-bridge brief](../../../../genet/docs/2026-05-26_scrying_dom_bridge.md),
//!    which lifts a WebView tile from `Opaque` toward `Partial`).
//! 3. **Cross-surface-parity** — every engine speaks this *one* vocabulary, so
//!    the host treats accessibility uniformly regardless of which engine backs a
//!    tile (a nematic document, a Genet page, a scrying WebView).

use serde::{Deserialize, Serialize};

/// Stable, document-local semantic identity.
///
/// An engine retains this identity for the same semantic object while a
/// session is live. Hosts must namespace it with their own tile/session
/// identity before lowering it into a global accessibility tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct DocumentA11yNodeId(u64);

impl DocumentA11yNodeId {
    /// Creates an engine-local identity. The numeric value has no host-global
    /// meaning and is never an AccessKit node id.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the engine-local identity for storage or deterministic tests.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A point in the final, current document-session viewport coordinate space.
///
/// Engines apply their own document zoom before returning this value. Hosts
/// may place it directly in their viewport transform, but retain ownership of
/// any outer tile transform.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentA11yPoint {
    pub x: f32,
    pub y: f32,
}

/// Bounds in the final, current document-session viewport coordinate space.
///
/// Engines apply their own document zoom before returning these bounds.
/// Structural nodes may have no bounds. A host must not invent geometry for
/// those nodes from a differently timed paint frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentA11yBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Semantic role vocabulary shared by retained document engines.
///
/// This deliberately describes document semantics rather than mirroring any
/// particular platform accessibility API. A host owns its platform lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yRole {
    Window,
    Document,
    Article,
    Region,
    Group,
    Navigation,
    Main,
    Heading { level: u8 },
    Paragraph,
    StaticText,
    Link,
    Button,
    TextField,
    CheckBox,
    RadioButton,
    RadioGroup,
    Switch,
    ComboBox,
    List,
    ListItem,
    ListBox,
    ListBoxOption,
    Table,
    Row,
    Cell,
    Image,
    Form,
    Dialog,
    Alert,
    Menu,
    MenuItem,
    MenuItemCheckBox,
    MenuItemRadio,
    TabList,
    Tab,
    TabPanel,
    Tree,
    TreeItem,
    Slider,
    SpinButton,
    Splitter,
    Toolbar,
    ProgressIndicator,
    Label,
    Status,
    Log,
    Note,
    Unknown,
}

/// Announcement priority for live regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yLive {
    Off,
    Polite,
    Assertive,
}

/// The three-value state used by switch and toggle controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yToggled {
    Off,
    On,
    Mixed,
}

/// The primary direction of an oriented control or container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yOrientation {
    Horizontal,
    Vertical,
}

/// The kind of popup a semantic node can present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yHasPopup {
    Menu,
    ListBox,
    Tree,
    Grid,
    Dialog,
}

/// Platform-neutral state for one semantic node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentA11yState {
    pub disabled: bool,
    pub hidden: bool,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub checked: Option<bool>,
    pub toggled: Option<DocumentA11yToggled>,
    pub focused: bool,
    pub editable: bool,
    pub multiline: bool,
    pub read_only: bool,
    pub required: bool,
    /// `None` preserves the absence of a live-region declaration; `Off` is
    /// an explicit declaration with distinct source semantics.
    pub live: Option<DocumentA11yLive>,
    pub orientation: Option<DocumentA11yOrientation>,
    pub has_popup: Option<DocumentA11yHasPopup>,
}

/// An action the engine advertises for a semantic node.
///
/// `Click` remains a pointer action: hosts obtain a fresh
/// [`DocumentA11yClickTarget`] and drive their ordinary pointer path. The
/// remaining actions dispatch through [`crate::DocumentSession`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yAction {
    Click,
    Focus,
    SetValue,
    ScrollIntoView,
    Increment,
    Decrement,
}

/// Data carried by an accessibility action request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11yActionData {
    Value(String),
}

/// A host request addressed to one local semantic node.
///
/// Engines must revalidate `revision`, `target`, and that the requested
/// action is advertised by their current projection before mutating state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentA11yActionRequest {
    pub revision: u64,
    pub target: DocumentA11yNodeId,
    pub action: DocumentA11yAction,
    pub data: Option<DocumentA11yActionData>,
}

/// A current, revalidated point at which a host may issue its ordinary click
/// path. `revision` makes a stale geometry observation detectable.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentA11yClickTarget {
    pub revision: u64,
    pub point: DocumentA11yPoint,
}

/// One semantic node in a renderer-neutral document projection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentA11yNode {
    pub id: DocumentA11yNodeId,
    pub parent: Option<DocumentA11yNodeId>,
    /// Ordered local child identities. This is authoritative for traversal;
    /// `parent` is retained for efficient reverse lookup and validation.
    pub children: Vec<DocumentA11yNodeId>,
    pub role: DocumentA11yRole,
    pub name: Option<String>,
    pub value: Option<String>,
    pub numeric_value: Option<f64>,
    pub numeric_minimum: Option<f64>,
    pub numeric_maximum: Option<f64>,
    pub bounds: Option<DocumentA11yBounds>,
    pub state: DocumentA11yState,
    pub actions: Vec<DocumentA11yAction>,
}

/// Why a projection cannot claim complete semantic coverage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentA11ySupportError {
    MissingLimitation { capability: A11yCapability },
}

impl std::fmt::Display for DocumentA11ySupportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingLimitation { capability } => {
                write!(
                    f,
                    "{capability:?} accessibility support requires a limitation"
                )
            },
        }
    }
}

impl std::error::Error for DocumentA11ySupportError {}

/// Coverage and explicit limitations of an accessibility projection.
///
/// A partial projection cannot be constructed without a non-blank limitation,
/// so hosts can describe its aperture honestly. The projection's support is
/// the live source of truth; an engine's registration-time capability remains
/// only a pre-spawn declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentA11ySupport {
    capability: A11yCapability,
    limitations: Vec<String>,
}

impl DocumentA11ySupport {
    pub fn new(
        capability: A11yCapability,
        limitations: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, DocumentA11ySupportError> {
        let limitations = limitations
            .into_iter()
            .map(Into::into)
            .filter(|limitation: &String| !limitation.trim().is_empty())
            .collect::<Vec<_>>();
        if capability == A11yCapability::Partial && limitations.is_empty() {
            return Err(DocumentA11ySupportError::MissingLimitation { capability });
        }
        Ok(Self {
            capability,
            limitations,
        })
    }

    pub fn capability(&self) -> A11yCapability {
        self.capability
    }

    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }
}

/// Renderer-neutral semantic snapshot of the current retained document.
///
/// `revision` increases whenever geometry, state, or actions observable
/// through this projection change. A semantic object's local ID remains stable
/// across revisions for the lifetime of that object; the revision instead
/// scopes an observation and its action requests, allowing an engine to reject
/// stale host observations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentA11yProjection {
    revision: u64,
    support: DocumentA11ySupport,
    root: DocumentA11yNodeId,
    nodes: Vec<DocumentA11yNode>,
}

impl DocumentA11yProjection {
    pub fn new(
        revision: u64,
        support: DocumentA11ySupport,
        root: DocumentA11yNodeId,
        nodes: Vec<DocumentA11yNode>,
    ) -> Self {
        Self {
            revision,
            support,
            root,
            nodes,
        }
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn support(&self) -> &DocumentA11ySupport {
        &self.support
    }

    pub const fn root(&self) -> DocumentA11yNodeId {
        self.root
    }

    pub fn nodes(&self) -> &[DocumentA11yNode] {
        &self.nodes
    }

    pub fn node(&self, id: DocumentA11yNodeId) -> Option<&DocumentA11yNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

/// What a content surface can expose to the accessibility tree. The single
/// vocabulary all engines/surfaces speak (invariant 3). Ordered worst-to-best
/// so capability can be compared / clamped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum A11yCapability {
    /// No semantic content the host can expose — a raw GPU surface or an opaque
    /// system WebView. The host surfaces this honestly (a labelled region with
    /// "contents not inspectable"), never as if it were [`Self::Full`]. Default
    /// for [`crate::SurfaceEngine`] (frame-streaming surfaces are opaque until
    /// they bridge their content).
    Opaque,
    /// Some structure available — e.g. a bridged WebView exposing a DOM
    /// projection (links / headings / text) but not full ARIA, or a
    /// partially-modelled document.
    Partial,
    /// A complete semantic tree (headings, links, roles, text). Default for
    /// document [`crate::Engine`]s: their [`crate::EngineDocument`] blocks *are*
    /// the semantic content, so they are accessible by construction.
    Full,
}

impl A11yCapability {
    /// Whether the host can build any semantic accessibility nodes from this
    /// surface (everything but [`Self::Opaque`]).
    pub fn is_inspectable(self) -> bool {
        self != Self::Opaque
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_node(id: u64) -> DocumentA11yNode {
        DocumentA11yNode {
            id: DocumentA11yNodeId::new(id),
            parent: None,
            children: Vec::new(),
            role: DocumentA11yRole::Document,
            name: Some("Example".into()),
            value: None,
            numeric_value: None,
            numeric_minimum: None,
            numeric_maximum: None,
            bounds: Some(DocumentA11yBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }),
            state: DocumentA11yState::default(),
            actions: vec![DocumentA11yAction::Focus],
        }
    }

    #[test]
    fn partial_support_requires_a_specific_limitation() {
        assert_eq!(
            DocumentA11ySupport::new(A11yCapability::Partial, ["", "   "])
                .expect_err("partial support must explain its aperture"),
            DocumentA11ySupportError::MissingLimitation {
                capability: A11yCapability::Partial
            }
        );

        let support = DocumentA11ySupport::new(
            A11yCapability::Partial,
            ["visible links are exposed; activation is unavailable"],
        )
        .expect("specific limitation makes partial support honest");
        assert_eq!(support.capability(), A11yCapability::Partial);
        assert_eq!(support.limitations().len(), 1);
    }

    #[test]
    fn projection_carries_stable_local_ids_revision_and_semantics() {
        let root = DocumentA11yNodeId::new(41);
        let projection = DocumentA11yProjection::new(
            9,
            DocumentA11ySupport::new(A11yCapability::Full, std::iter::empty::<String>())
                .expect("full support needs no limitation"),
            root,
            vec![document_node(root.get())],
        );

        assert_eq!(projection.revision(), 9);
        assert_eq!(projection.root(), root);
        let node = projection
            .node(root)
            .expect("local root remains addressable");
        assert_eq!(node.role, DocumentA11yRole::Document);
        assert_eq!(node.name.as_deref(), Some("Example"));
        assert_eq!(node.state.live, None, "absence differs from explicit Off");
        assert_eq!(node.actions, vec![DocumentA11yAction::Focus]);
        assert_eq!(projection.support().capability(), A11yCapability::Full);
    }
}
