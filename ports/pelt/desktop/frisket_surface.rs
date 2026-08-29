//! Frisket rendered through the owned Livery/Buckram lane.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use accesskit::{Action, NodeId as AccessNodeId, Role, TreeUpdate};
use cambium::{
    AnyView, DividerTarget, DomHandle, FRISKET_CSS, FRISKET_TILE_ATTR, GenetAppRunner, GenetCtx,
    GenetElement, close_target, content_target, divider_target, el, frisket, stack_target,
    tab_drop_index, tab_target,
};
use genet_host_api::tile::{TileId, TilePath, TileTree};
use genet_livery::{Device, LiveryDocument, StyleSet};
use genet_render::accesskit_tree;
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::{LayoutDom, LocalName, Namespace};
use pelt_core::WorkspaceRect;

use crate::appearance::AppearanceTheme;

type FrameView = Box<dyn AnyView<FrameState, (), GenetCtx, GenetElement>>;
type FrameLogic = fn(&FrameState) -> FrameView;

struct FrameState {
    tree: TileTree,
    chrome: Option<WorkspaceChrome>,
}

fn frame_view(state: &FrameState) -> FrameView {
    let pane: FrameView = Box::new(frisket(&state.tree, |_state: &mut FrameState, _event| {}));
    match state.chrome.as_ref() {
        Some(chrome) => chrome_view(chrome, pane),
        None => pane,
    }
}

const ATTR_CHROME_ACTION: &str = "data-pelt-chrome";
const ATTR_INSPECTOR: &str = "data-pelt-inspector";
const ATTR_DIAGNOSTIC: &str = "data-pelt-diagnostic";
const ATTR_APPEARANCE: &str = "data-pelt-appearance";

fn chrome_button(action: &str, label: &str, accessible_label: &str, disabled: bool) -> FrameView {
    let class = if disabled {
        "pelt-chrome-button disabled"
    } else {
        "pelt-chrome-button"
    };
    Box::new(
        el::<_, FrameState, ()>("div", label.to_owned())
            .attr("class", class)
            .attr("role", "button")
            .attr("aria-label", accessible_label)
            .attr("aria-disabled", if disabled { "true" } else { "false" })
            .attr(ATTR_CHROME_ACTION, action),
    )
}

/// A CSD caption control. Same retained-button shape as `chrome_button`, its
/// own class so the stylesheet can give the trio the caption look rather than
/// the toolbar look.
fn caption_button(action: &str, glyph: &str, accessible_label: &str) -> FrameView {
    Box::new(
        el::<_, FrameState, ()>("div", glyph.to_owned())
            .attr("class", "pelt-caption-button")
            .attr("role", "button")
            .attr("aria-label", accessible_label)
            .attr(ATTR_CHROME_ACTION, action),
    )
}

fn chrome_toggle_button(
    action: &str,
    label: &str,
    accessible_label: &str,
    pressed: bool,
) -> FrameView {
    Box::new(
        el::<_, FrameState, ()>("div", label.to_owned())
            .attr(
                "class",
                if pressed {
                    "pelt-chrome-button pelt-chrome-toggle pelt-chrome-toggle-open"
                } else {
                    "pelt-chrome-button pelt-chrome-toggle"
                },
            )
            .attr("role", "button")
            .attr("aria-label", accessible_label)
            .attr("aria-pressed", if pressed { "true" } else { "false" })
            .attr(ATTR_CHROME_ACTION, action),
    )
}

fn chrome_view(chrome: &WorkspaceChrome, pane: FrameView) -> FrameView {
    let address = format!(
        "{}{}",
        chrome.address,
        if chrome.address_focused { " |" } else { "" }
    );
    let toolbar: Vec<FrameView> = vec![
        chrome_button("back", "←", "Back", !chrome.can_go_back),
        chrome_button("forward", "→", "Forward", !chrome.can_go_forward),
        chrome_button("reload", "R", "Reload", false),
        Box::new(
            el::<_, FrameState, ()>("div", address)
                .attr("class", "pelt-address")
                .attr("role", "textbox")
                .attr("aria-label", "Address")
                .attr("aria-readonly", "false")
                .attr(ATTR_CHROME_ACTION, "address"),
        ),
        Box::new(
            el::<_, FrameState, ()>(
                "div",
                format!(
                    "Engine: {} {}",
                    chrome.engine_label,
                    if chrome.engine_menu_open {
                        "▴"
                    } else {
                        "▾"
                    }
                ),
            )
            .attr(
                "class",
                if chrome.engine_menu_open {
                    "pelt-engine pelt-engine-open"
                } else {
                    "pelt-engine"
                },
            )
            .attr("role", "button")
            .attr("aria-label", "Choose engine for focused tile")
            .attr("aria-haspopup", "menu")
            .attr(
                "aria-expanded",
                if chrome.engine_menu_open {
                    "true"
                } else {
                    "false"
                },
            )
            .attr(ATTR_CHROME_ACTION, "engine-menu"),
        ),
        chrome_toggle_button(
            "inspect",
            "Inspect",
            "Toggle content inspector",
            chrome.inspector.is_some(),
        ),
        chrome_toggle_button(
            "appearance",
            "Theme",
            "Toggle Pelt appearance settings",
            chrome.appearance.is_some(),
        ),
    ];
    let mut toolbar = toolbar;
    // The dense chrome row: title, route and status ride in the same row as
    // the controls. None of them carries a chrome action, so under CSD this
    // stretch of the row is the window's drag surface.
    toolbar.push(Box::new(
        el::<_, FrameState, ()>("div", chrome.title.clone()).attr("class", "pelt-title"),
    ));
    toolbar.push(Box::new(
        el::<_, FrameState, ()>("div", chrome.route.clone()).attr("class", "pelt-route"),
    ));
    toolbar.push(Box::new(
        el::<_, FrameState, ()>("div", chrome.status.clone()).attr("class", "pelt-status"),
    ));
    if chrome.window_controls {
        let (maximize_glyph, maximize_label) = if chrome.maximized {
            ("\u{2750}", "Restore")
        } else {
            ("\u{25a1}", "Maximize")
        };
        toolbar.push(caption_button("minimize", "\u{2013}", "Minimize"));
        toolbar.push(caption_button("maximize", maximize_glyph, maximize_label));
        toolbar.push(caption_button("close-window", "\u{d7}", "Close window"));
    }
    let mut header_items: Vec<FrameView> = vec![Box::new(
        el::<_, FrameState, ()>("div", toolbar).attr("class", "pelt-toolbar"),
    )];
    if chrome.engine_menu_open {
        let choices = chrome
            .engine_choices
            .iter()
            .copied()
            .map(|choice| engine_choice_view(choice, chrome.engine_selected == Some(choice)))
            .collect::<Vec<_>>();
        header_items.push(Box::new(
            el::<_, FrameState, ()>("div", choices)
                .attr("id", "pelt-engine-menu")
                .attr("class", "pelt-engine-menu")
                .attr("role", "menu")
                .attr("aria-label", "Engine for focused tile"),
        ));
    }
    let header: FrameView = Box::new(el::<_, FrameState, ()>("div", header_items).attr(
        "class",
        if chrome.engine_menu_open {
            "pelt-chrome pelt-chrome-menu-open"
        } else {
            "pelt-chrome"
        },
    ));
    let pane: FrameView = Box::new(
        el::<_, FrameState, ()>("div", pane)
            .attr("class", "pelt-pane")
            .attr("aria-label", "Workspace panes"),
    );
    let mut body: Vec<FrameView> = vec![pane];
    if let Some(inspector) = chrome.inspector.as_ref() {
        body.push(inspector_view(inspector));
    }
    if let Some(appearance) = chrome.appearance.as_ref() {
        body.push(appearance_view(appearance));
    }
    let mut workspace: Vec<FrameView> = vec![
        header,
        Box::new(el::<_, FrameState, ()>("div", body).attr("class", "pelt-body")),
    ];
    if let Some(diagnostic) = chrome.diagnostic.as_ref() {
        workspace.push(diagnostic_view(diagnostic));
    }
    Box::new(
        el::<_, FrameState, ()>("div", workspace)
            .attr("class", format!("pelt-workspace {}", chrome.theme.class())),
    )
}

/// One laid-out Frisket frame and the active content holes it authorizes.
pub(crate) struct FrisketFrame {
    pub scene: netrender::Scene,
    pub content_rects: Vec<(TileId, WorkspaceRect)>,
    /// Bounds of the retained inspector when it is open. The desktop host
    /// restores this crop after native texture composition.
    pub inspector_rect: Option<WorkspaceRect>,
    /// Bounds of the host-owned loading/error document. The desktop host
    /// restores this crop after document and native tile composition.
    pub diagnostic_rect: Option<WorkspaceRect>,
    /// Bounds of the Pelt-owned appearance drawer. The desktop host restores
    /// this crop after document and native tile composition.
    pub appearance_rect: Option<WorkspaceRect>,
}

/// Pelt's honest shell-level description of one Frisket content aperture.
///
/// The aperture is a named region in Pelt's tree. Its child document or native
/// surface is deliberately not projected here: each engine still needs a
/// namespaced composite-tree provider before Pelt can merge it safely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrisketContentA11y {
    pub tile: TileId,
    pub label: String,
    pub description: String,
}

/// One retained Frisket projection plus the current DOM nodes its actions name.
pub(crate) struct FrisketA11yProjection {
    pub tree: TreeUpdate,
    pub root: AccessNodeId,
    pub nodes: HashMap<AccessNodeId, NodeId>,
    /// The AccessKit node that represents each visible tile content aperture.
    ///
    /// These are shell nodes only. Pelt can use this map as the stable
    /// attachment point for a future namespaced child tree without changing
    /// the existing Frisket shell projection.
    pub content_nodes: HashMap<TileId, AccessNodeId>,
}

/// The shell actions a screen reader may request through the Frisket tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrisketA11yTarget {
    ChromeAction(ChromeAction),
    Close(TileId),
    Tab(TileId),
}

/// Semantic result of hit-testing the live Frisket DOM.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FrisketHit {
    Content(TileId),
    Close(TileId),
    Tab(TileId),
    ChromeAction(ChromeAction),
    Divider {
        target: DividerTarget,
        split_rect: WorkspaceRect,
    },
    Appearance,
    Chrome,
}

/// The small set of host-owned controls layered over the generic Frisket pane
/// frame. Frisket continues to own split and tab semantics; these actions are
/// deliberately Pelt-specific browser chrome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChromeAction {
    Back,
    Forward,
    Reload,
    Address,
    ToggleEngineMenu,
    ChooseEngine(ChromeEngineChoice),
    ToggleInspector,
    ToggleAppearance,
    ChooseTheme(AppearanceTheme),
    /// CSD caption verbs. Present only when the host asked the chrome to
    /// draw window controls (an undecorated Windows workspace window).
    Minimize,
    ToggleMaximize,
    CloseWindow,
}

/// One explicitly available engine choice in the focused-tile Pelt menu.
/// The host decides which of these are registered for a build and preserves
/// route ownership when a row is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChromeEngineChoice {
    Automatic,
    Livery,
    Reader,
    Scripted,
}

impl ChromeEngineChoice {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Livery => "Livery",
            Self::Reader => "Reader",
            Self::Scripted => "Scripted",
        }
    }

    pub(crate) const fn trigger_label(self) -> &'static str {
        match self {
            Self::Automatic => "Auto",
            Self::Livery => "Livery",
            Self::Reader => "Reader",
            Self::Scripted => "Scripted",
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Automatic => "engine-automatic",
            Self::Livery => "engine-livery",
            Self::Reader => "engine-reader",
            Self::Scripted => "engine-scripted",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Livery => "genet.livery",
            Self::Reader => "genet.reader",
            Self::Scripted => "genet.scripted",
        }
    }
}

fn engine_choice_view(choice: ChromeEngineChoice, selected: bool) -> FrameView {
    Box::new(
        el::<_, FrameState, ()>("div", choice.label())
            .attr(
                "class",
                if selected {
                    "pelt-engine-option pelt-engine-option-selected"
                } else {
                    "pelt-engine-option"
                },
            )
            .attr("role", "menuitemradio")
            .attr("aria-checked", if selected { "true" } else { "false" })
            .attr("data-key", choice.id())
            .attr(ATTR_CHROME_ACTION, choice.action()),
    )
}

/// Pelt-owned appearance controls. The selection is intentionally kept
/// separate from document engine themes and Tabard's preview-only palette.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChromeAppearance {
    pub theme: AppearanceTheme,
    pub persistent: bool,
}

fn appearance_choice_view(theme: AppearanceTheme, selected: bool) -> FrameView {
    Box::new(
        el::<_, FrameState, ()>("div", theme.label())
            .attr(
                "class",
                if selected {
                    "pelt-appearance-option pelt-appearance-option-selected"
                } else {
                    "pelt-appearance-option"
                },
            )
            .attr("role", "radio")
            .attr("aria-checked", if selected { "true" } else { "false" })
            .attr(ATTR_CHROME_ACTION, theme.action()),
    )
}

fn appearance_view(appearance: &ChromeAppearance) -> FrameView {
    let options = [AppearanceTheme::Dark, AppearanceTheme::Light]
        .into_iter()
        .map(|theme| appearance_choice_view(theme, appearance.theme == theme))
        .collect::<Vec<_>>();
    let rows: Vec<FrameView> = vec![
        Box::new(
            el::<_, FrameState, ()>("div", "Appearance").attr("class", "pelt-appearance-heading"),
        ),
        Box::new(
            el::<_, FrameState, ()>("div", "Chrome theme").attr("class", "pelt-appearance-label"),
        ),
        Box::new(
            el::<_, FrameState, ()>("div", options)
                .attr("class", "pelt-appearance-options")
                .attr("role", "radiogroup")
                .attr("aria-label", "Chrome theme"),
        ),
        Box::new(
            el::<_, FrameState, ()>(
                "div",
                if appearance.persistent {
                    "Saved for this Pelt application."
                } else {
                    "This Pelt session only. Supply an appearance store to keep it after restart."
                },
            )
            .attr("class", "pelt-appearance-scope"),
        ),
        Box::new(
            el::<_, FrameState, ()>("div", "Document content keeps its engine-owned theme.")
                .attr("class", "pelt-appearance-note"),
        ),
    ];
    Box::new(
        el::<_, FrameState, ()>("div", rows)
            .attr("class", "pelt-appearance")
            .attr(ATTR_APPEARANCE, "true")
            .attr("role", "region")
            .attr("aria-label", "Appearance"),
    )
}

/// Bounded, host-owned presentation data for a session's structural read.
/// It deliberately carries strings rather than Inker's report types, keeping
/// the renderer independent from engine-owned document details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChromeInspector {
    pub status: String,
    pub title: Option<String>,
    pub summary: String,
    pub sections: Vec<ChromeInspectorSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChromeInspectorSection {
    pub label: String,
    pub entries: Vec<String>,
    pub omitted: usize,
}

fn inspector_view(inspector: &ChromeInspector) -> FrameView {
    let mut rows: Vec<FrameView> = vec![
        Box::new(
            el::<_, FrameState, ()>("div", "Content inspector")
                .attr("class", "pelt-inspector-heading"),
        ),
        Box::new(
            el::<_, FrameState, ()>("div", inspector.status.clone())
                .attr("class", "pelt-inspector-capability"),
        ),
    ];
    if let Some(title) = inspector.title.as_ref() {
        rows.push(Box::new(
            el::<_, FrameState, ()>("div", title.clone()).attr("class", "pelt-inspector-title"),
        ));
    }
    rows.push(Box::new(
        el::<_, FrameState, ()>("div", inspector.summary.clone())
            .attr("class", "pelt-inspector-summary"),
    ));
    for section in &inspector.sections {
        rows.push(Box::new(
            el::<_, FrameState, ()>("div", section.label.clone())
                .attr("class", "pelt-inspector-section"),
        ));
        rows.extend(section.entries.iter().cloned().map(|entry| {
            Box::new(el::<_, FrameState, ()>("div", entry).attr("class", "pelt-inspector-entry"))
                as FrameView
        }));
        if section.omitted > 0 {
            rows.push(Box::new(
                el::<_, FrameState, ()>("div", format!("{} more", section.omitted))
                    .attr("class", "pelt-inspector-more"),
            ));
        }
    }
    Box::new(
        el::<_, FrameState, ()>("div", rows)
            .attr("class", "pelt-inspector")
            .attr(ATTR_INSPECTOR, "true")
            .attr("role", "region")
            .attr("aria-label", "Content inspector"),
    )
}

/// The bounded, host-owned content projection shown for a document session
/// that has just been replaced or could not be replaced. It is deliberately
/// not synthetic engine HTML: Pelt keeps the live controller and history
/// beneath this retained Frisket overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChromeDocumentKind {
    Loading,
    Error,
}

impl ChromeDocumentKind {
    const fn id(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChromeDocument {
    pub kind: ChromeDocumentKind,
    pub tile: TileId,
    pub rect: WorkspaceRect,
    pub address: String,
    pub message: Option<String>,
}

fn diagnostic_view(diagnostic: &ChromeDocument) -> FrameView {
    let (heading, summary, note, role) = match diagnostic.kind {
        ChromeDocumentKind::Loading => (
            "Loading",
            "Pelt has installed the replacement document.".to_owned(),
            "The document will replace this notice after its first composed frame.",
            "status",
        ),
        ChromeDocumentKind::Error => (
            "Could not load",
            diagnostic_error_summary(diagnostic),
            "The previous document and its history remain available. Use Back, Reload, or the address field to continue.",
            "alert",
        ),
    };
    let rows: Vec<FrameView> = vec![
        Box::new(el::<_, FrameState, ()>("div", heading).attr("class", "pelt-diagnostic-heading")),
        Box::new(
            el::<_, FrameState, ()>("div", diagnostic.address.clone())
                .attr("class", "pelt-diagnostic-address"),
        ),
        Box::new(el::<_, FrameState, ()>("div", summary).attr("class", "pelt-diagnostic-message")),
        Box::new(el::<_, FrameState, ()>("div", note).attr("class", "pelt-diagnostic-note")),
    ];
    Box::new(
        el::<_, FrameState, ()>("div", rows)
            .attr(
                "class",
                match diagnostic.kind {
                    ChromeDocumentKind::Loading => "pelt-diagnostic pelt-diagnostic-loading",
                    ChromeDocumentKind::Error => "pelt-diagnostic pelt-diagnostic-error",
                },
            )
            .attr(
                "style",
                format!(
                    "left: {}px; top: {}px; width: {}px; height: {}px;",
                    diagnostic.rect.x,
                    diagnostic.rect.y,
                    diagnostic.rect.width,
                    diagnostic.rect.height,
                ),
            )
            .attr(ATTR_DIAGNOSTIC, diagnostic.kind.id())
            .attr("data-pelt-tile", diagnostic.tile.0.to_string())
            .attr("role", role)
            .attr("aria-label", heading),
    )
}

fn diagnostic_error_summary(diagnostic: &ChromeDocument) -> String {
    let Some(message) = diagnostic.message.as_deref() else {
        return "The document engine did not provide an error message.".to_owned();
    };
    let repeated_address = format!("could not load {}: ", diagnostic.address);
    message
        .strip_prefix(&repeated_address)
        .unwrap_or(message)
        .to_owned()
}

/// Snapshot rendered by the retained Pelt chrome above Frisket.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorkspaceChrome {
    pub title: String,
    pub address: String,
    pub route: String,
    pub status: String,
    pub theme: AppearanceTheme,
    pub address_focused: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub engine_label: String,
    pub engine_menu_open: bool,
    pub engine_selected: Option<ChromeEngineChoice>,
    pub engine_choices: Vec<ChromeEngineChoice>,
    pub inspector: Option<ChromeInspector>,
    pub appearance: Option<ChromeAppearance>,
    pub diagnostic: Option<ChromeDocument>,
    /// Draw the caption trio (minimize / maximize / close) in the chrome row.
    /// True only for an undecorated (CSD) window, where the OS no longer
    /// supplies them.
    pub window_controls: bool,
    /// Whether the window is currently maximized; picks the maximize
    /// button's glyph and accessible label (Maximize vs Restore).
    pub maximized: bool,
}

/// A retained, GPU-free pane frame. Its DOM is produced by Cambium Frisket;
/// Livery/Buckram supplies both its scene and the geometry consumed by Pelt.
pub(crate) struct FrisketSurface {
    tree: TileTree,
    chrome: Option<WorkspaceChrome>,
    /// Optional host-authored Chrome layer. It remains outside the document
    /// tile/session path so previews can restyle the shell without changing a
    /// route, controller, or content-hole geometry.
    chrome_stylesheet: Option<String>,
    content_a11y: HashMap<TileId, FrisketContentA11y>,
    viewport: (u32, u32),
    document: LiveryDocument<ScriptedDom>,
}

impl FrisketSurface {
    pub fn new(tree: &TileTree) -> Self {
        let viewport = (800, 600);
        Self {
            tree: tree.clone(),
            chrome: None,
            chrome_stylesheet: None,
            content_a11y: HashMap::new(),
            viewport,
            document: document_for(tree, None, None, viewport.0, viewport.1),
        }
    }

    pub fn set_tree(&mut self, tree: &TileTree) {
        self.tree = tree.clone();
        self.rebuild_document();
    }

    pub fn set_chrome(&mut self, chrome: Option<WorkspaceChrome>) {
        if self.chrome.as_ref() == chrome.as_ref() {
            return;
        }
        self.chrome = chrome;
        self.rebuild_document();
    }

    #[cfg(any(feature = "tabard-preview", test))]
    /// Append or remove a host-owned author layer for the workspace shell.
    ///
    /// This deliberately has no document-tile input: Pelt keeps session,
    /// routing, and content composition authority while the retained Livery
    /// document supplies the Chrome preview.
    pub fn set_chrome_stylesheet(&mut self, stylesheet: Option<String>) {
        if self.chrome_stylesheet == stylesheet {
            return;
        }
        self.chrome_stylesheet = stylesheet;
        self.rebuild_document();
    }

    /// Supply Pelt's per-tile accessibility declarations without changing the
    /// rendered document or asking Livery to relayout it.
    pub fn set_content_accessibility(
        &mut self,
        regions: impl IntoIterator<Item = FrisketContentA11y>,
    ) {
        self.content_a11y = regions
            .into_iter()
            .map(|region| (region.tile, region))
            .collect();
    }

    pub fn frame(&mut self, width: u32, height: u32) -> Result<FrisketFrame, String> {
        let viewport = (width.max(1), height.max(1));
        if self.viewport != viewport {
            self.viewport = viewport;
            self.rebuild_document();
        }
        let list = self
            .document
            .frame(viewport.0, viewport.1)
            .map_err(|error| format!("could not lay out Frisket: {error}"))?;
        let content_rects = nodes_with_attr(self.document.dom(), FRISKET_TILE_ATTR)
            .into_iter()
            .filter_map(|node| {
                let id = attr(self.document.dom(), node, FRISKET_TILE_ATTR)?
                    .parse::<u64>()
                    .ok()?;
                self.document
                    .fragment_rect(node)
                    .map(|rect| (TileId(id), workspace_rect(rect)))
            })
            .collect();
        let inspector_rect = nodes_with_attr(self.document.dom(), ATTR_INSPECTOR)
            .into_iter()
            .find_map(|node| self.document.fragment_rect(node).map(workspace_rect));
        let diagnostic_rect = nodes_with_attr(self.document.dom(), ATTR_DIAGNOSTIC)
            .into_iter()
            .find_map(|node| self.document.fragment_rect(node).map(workspace_rect));
        let appearance_rect = nodes_with_attr(self.document.dom(), ATTR_APPEARANCE)
            .into_iter()
            .find_map(|node| self.document.fragment_rect(node).map(workspace_rect));
        Ok(FrisketFrame {
            scene: paint_list_render::translate_paint_list(&list),
            content_rects,
            inspector_rect,
            diagnostic_rect,
            appearance_rect,
        })
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<FrisketHit> {
        let node = self.document.hit_test(x, y)?;
        let dom = self.document.dom();
        if let Some(tile) = diagnostic_target(dom, node) {
            return Some(FrisketHit::Content(tile));
        }
        if let Some(action) = chrome_action(dom, node) {
            return Some(FrisketHit::ChromeAction(action));
        }
        if appearance_target(dom, node) {
            return Some(FrisketHit::Appearance);
        }
        if let Some(tile) = close_target(dom, node) {
            return Some(FrisketHit::Close(tile));
        }
        if let Some(target) = divider_target(dom, node) {
            let split_rect = divider_node(dom, node)
                .and_then(|divider| dom.parent(divider))
                .and_then(|split| self.document.fragment_rect(split))
                .map(workspace_rect)?;
            return Some(FrisketHit::Divider { target, split_rect });
        }
        if let Some(tile) = tab_target(dom, node) {
            return Some(FrisketHit::Tab(tile));
        }
        if let Some(tile) = content_target(dom, node) {
            return Some(FrisketHit::Content(tile));
        }
        Some(FrisketHit::Chrome)
    }

    /// Project the completed Frisket layout into its shell portion of the
    /// workspace AccessKit tree.
    ///
    /// This layer names only Frisket's DOM. Pelt may attach a namespaced Livery
    /// child tree below the labelled content apertures; other document and
    /// native-surface lanes remain declared apertures until they supply a
    /// compatible host-owned composition protocol.
    pub fn accessibility_projection(
        &self,
        focus: Option<&FrisketA11yTarget>,
    ) -> Option<FrisketA11yProjection> {
        let dom = self.document.dom();
        // P6 projects only Frisket's fixed shell. Pelt routes wheel and scroll
        // keys to the active tile engine, never this Livery document. A future
        // scrollable shell needs visual, scroll-adjusted bounds before it can
        // reuse this projection.
        debug_assert_eq!(self.document.scroll(), (0.0, 0.0));
        debug_assert!(self.document.element_scroll().is_empty());
        let fragments = self.document.retained_layout()?;
        let root = AccessNodeId(dom.opaque_id(dom.document()));
        let nodes = nodes_in_document(dom)
            .into_iter()
            .map(|node| (AccessNodeId(dom.opaque_id(node)), node))
            .collect::<HashMap<_, _>>();
        let mut tree = accesskit_tree(dom, fragments, None);

        // A Frisket document is rebuilt whenever Chrome state changes, so a
        // raw ScriptedDom NodeId would become foreign on the next frame. Keep
        // virtual focus by semantic target and resolve it in this fresh tree.
        if let Some(focused) = focus.and_then(|target| {
            tree.nodes.iter().find_map(|(id, access)| {
                (access.supports_action(Action::Focus)
                    && nodes.get(id).is_some_and(|node| {
                        self.accessibility_target(*node).as_ref() == Some(target)
                    }))
                .then_some(*id)
            })
        }) {
            tree.focus = focused;
        }

        let mut content_nodes = HashMap::new();
        for (id, access) in &mut tree.nodes {
            let Some(node) = nodes.get(id).copied() else {
                continue;
            };
            if chrome_action(dom, node) == Some(ChromeAction::Address) {
                if let Some(chrome) = &self.chrome {
                    access.set_value(chrome.address.clone());
                    access.add_action(Action::SetValue);
                }
            }
            let Some(tile) = attr(dom, node, FRISKET_TILE_ATTR)
                .and_then(|value| value.parse::<u64>().ok())
                .map(TileId)
            else {
                continue;
            };
            let (label, description) = self.content_a11y.get(&tile).map_or_else(
                || {
                    (
                        format!("Tile {} content", tile.0),
                        "Pelt has not received an accessibility declaration for this content."
                            .to_owned(),
                    )
                },
                |region| (region.label.clone(), region.description.clone()),
            );
            access.set_role(Role::Region);
            access.set_label(label);
            access.set_description(description);
            content_nodes.insert(tile, *id);
        }

        Some(FrisketA11yProjection {
            tree,
            root,
            nodes,
            content_nodes,
        })
    }

    /// Resolve a screen-reader Click against the same retained shell actions
    /// used by the pointer path. Focus requests deliberately do not come here.
    pub fn accessibility_target(&self, node: NodeId) -> Option<FrisketA11yTarget> {
        let dom = self.document.dom();
        if let Some(action) = chrome_action(dom, node) {
            return Some(FrisketA11yTarget::ChromeAction(action));
        }
        if let Some(tile) = close_target(dom, node) {
            return Some(FrisketA11yTarget::Close(tile));
        }
        tab_target(dom, node).map(FrisketA11yTarget::Tab)
    }

    pub fn tabbar_drop(&self, x: f32, y: f32) -> Option<(TilePath, usize)> {
        let hit = self.document.hit_test(x, y)?;
        stack_target(self.document.dom(), hit)?;
        tab_drop_index(self.document.dom(), hit, x, |node| {
            self.document
                .fragment_rect(node)
                .map(|rect| (rect[0], rect[1], rect[2], rect[3]))
        })
    }

    pub fn tab_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        self.rect_for_attr("data-tabid", &tile.0.to_string())
    }

    /// Geometry for the tab's visible text region, kept distinct from its
    /// fixed close gutter so small Chrome receipts can prove both survive.
    pub fn tab_label_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        let id = tile.0.to_string();
        let dom = self.document.dom();
        let tab = nodes_with_attr(dom, "data-tabid")
            .into_iter()
            .find(|node| attr(dom, *node, "data-tabid").as_deref() == Some(id.as_str()))?;
        dom.dom_children(tab)
            .find(|node| attr(dom, *node, "class").as_deref() == Some("frisket-label"))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }

    pub fn close_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        nodes_in_document(self.document.dom())
            .into_iter()
            .find(|node| close_target(self.document.dom(), *node) == Some(tile))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }

    pub fn chrome_rect(&self, action: &str) -> Option<WorkspaceRect> {
        self.rect_for_attr(ATTR_CHROME_ACTION, action)
    }

    #[cfg(any(feature = "tabard-preview", test))]
    /// Read a resolved style from a named shell class for a focused receipt.
    /// It avoids exposing the transient DOM node identifiers which become
    /// invalid every time the retained Chrome document is rebuilt.
    pub fn chrome_computed_style(&self, class: &str, property: &str) -> Option<String> {
        let dom = self.document.dom();
        dom.first_with_class(dom.document(), class)
            .and_then(|node| self.document.computed_style(node, property))
    }

    pub fn divider_rect(&self, target: &DividerTarget) -> Option<WorkspaceRect> {
        nodes_with_attr(self.document.dom(), "data-divider")
            .into_iter()
            .find(|node| divider_target(self.document.dom(), *node).as_ref() == Some(target))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }

    fn rect_for_attr(&self, name: &str, value: &str) -> Option<WorkspaceRect> {
        nodes_with_attr(self.document.dom(), name)
            .into_iter()
            .find(|node| attr(self.document.dom(), *node, name).as_deref() == Some(value))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }

    fn rebuild_document(&mut self) {
        self.document = document_for(
            &self.tree,
            self.chrome.as_ref(),
            self.chrome_stylesheet.as_deref(),
            self.viewport.0,
            self.viewport.1,
        );
    }
}

fn document_for(
    tree: &TileTree,
    chrome: Option<&WorkspaceChrome>,
    chrome_stylesheet: Option<&str>,
    width: u32,
    height: u32,
) -> LiveryDocument<ScriptedDom> {
    let tree = chrome.map_or_else(|| tree.clone(), |_| compact_tab_tree(tree));
    let handle: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
    let runner = GenetAppRunner::new(
        handle.clone(),
        frame_view as FrameLogic,
        FrameState {
            tree,
            chrome: chrome.cloned(),
        },
    );
    let html = handle.borrow().outer_html(runner.root());
    let dom = ScriptedDom::from_serialized_document(&html);
    let host_css = format!(
        "html, body {{ display: block; width: {width}px; height: {height}px; margin: 0; overflow: hidden; }} \
         * {{ box-sizing: border-box; }} \
         .frisket-body {{ width: {width}px; height: {height}px; }}"
    );
    let mut stylesheets = vec![
        host_css.as_str(),
        FRISKET_CSS,
        PELT_CHROME_CSS,
        PELT_LIGHT_THEME_CSS,
    ];
    if chrome.is_some() {
        if let Some(stylesheet) = chrome_stylesheet {
            stylesheets.push(stylesheet);
        }
    }
    LiveryDocument::new(
        dom,
        StyleSet::cambium(&stylesheets),
        Device::screen(width as f32, height as f32),
    )
}

fn compact_tab_tree(tree: &TileTree) -> TileTree {
    let mut compact = tree.clone();
    let ids = compact
        .tiles()
        .into_iter()
        .map(|tile| tile.id)
        .collect::<Vec<_>>();
    for id in ids {
        if let Some(tile) = compact.tile_mut(id) {
            let base = tile
                .title
                .split_once(" [")
                .map_or(tile.title.as_str(), |(base, _)| base);
            tile.title = truncated(base.trim(), 28);
        }
    }
    compact
}

fn truncated(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn chrome_action(dom: &ScriptedDom, hit: NodeId) -> Option<ChromeAction> {
    let mut node = hit;
    loop {
        let action = attr(dom, node, ATTR_CHROME_ACTION);
        if let Some(action) = action.as_deref() {
            if attr(dom, node, "aria-disabled").as_deref() == Some("true") {
                return None;
            }
            return match action {
                "back" => Some(ChromeAction::Back),
                "forward" => Some(ChromeAction::Forward),
                "reload" => Some(ChromeAction::Reload),
                "address" => Some(ChromeAction::Address),
                "engine-menu" => Some(ChromeAction::ToggleEngineMenu),
                "engine-automatic" => {
                    Some(ChromeAction::ChooseEngine(ChromeEngineChoice::Automatic))
                },
                "engine-livery" => Some(ChromeAction::ChooseEngine(ChromeEngineChoice::Livery)),
                "engine-reader" => Some(ChromeAction::ChooseEngine(ChromeEngineChoice::Reader)),
                "engine-scripted" => Some(ChromeAction::ChooseEngine(ChromeEngineChoice::Scripted)),
                "inspect" => Some(ChromeAction::ToggleInspector),
                "appearance" => Some(ChromeAction::ToggleAppearance),
                "appearance-dark" => Some(ChromeAction::ChooseTheme(AppearanceTheme::Dark)),
                "appearance-light" => Some(ChromeAction::ChooseTheme(AppearanceTheme::Light)),
                "minimize" => Some(ChromeAction::Minimize),
                "maximize" => Some(ChromeAction::ToggleMaximize),
                "close-window" => Some(ChromeAction::CloseWindow),
                _ => None,
            };
        }
        node = dom.parent(node)?;
    }
}

fn appearance_target(dom: &ScriptedDom, hit: NodeId) -> bool {
    let mut node = hit;
    loop {
        if attr(dom, node, ATTR_APPEARANCE).is_some() {
            return true;
        }
        let Some(parent) = dom.parent(node) else {
            return false;
        };
        node = parent;
    }
}

fn diagnostic_target(dom: &ScriptedDom, hit: NodeId) -> Option<TileId> {
    let mut node = hit;
    loop {
        if attr(dom, node, ATTR_DIAGNOSTIC).is_some() {
            return attr(dom, node, "data-pelt-tile")
                .and_then(|tile| tile.parse::<u64>().ok())
                .map(TileId);
        }
        node = dom.parent(node)?;
    }
}

const PELT_CHROME_CSS: &str = "\
    .pelt-workspace { \
        --pelt-chrome-workspace: #202027; --pelt-chrome-surface: #24242d; --pelt-chrome-border: #3c3c48; \
        --pelt-chrome-control-text: #e8e8ee; --pelt-chrome-control-surface: #3a3a46; --pelt-chrome-control-border: #555565; \
        --pelt-chrome-disabled-text: #777783; --pelt-chrome-disabled-surface: #2c2c34; --pelt-chrome-disabled-border: #363640; \
        --pelt-chrome-accent-text: #ffffff; --pelt-chrome-accent-surface: #41506b; --pelt-chrome-accent-border: #83a3d5; \
        --pelt-chrome-address-text: #f0f0f4; --pelt-chrome-address-surface: #16161d; \
        --pelt-chrome-context-text: #bfe9ff; --pelt-chrome-context-surface: #30384a; --pelt-chrome-context-border: #566d91; \
        --pelt-chrome-selection-text: #ffffff; --pelt-chrome-selection-surface: #46628a; --pelt-chrome-selection-border: #9cc8ff; \
        --pelt-chrome-heading: #ffffff; --pelt-chrome-route: #9ccdf0; --pelt-chrome-status: #a8d6a8; \
        --pelt-chrome-panel-text: #e8e8ee; --pelt-chrome-panel-surface: #1b1b23; --pelt-chrome-panel-border: #3c3c48; \
        --pelt-chrome-summary: #c8c8d4; --pelt-chrome-section: #8bb9eb; --pelt-chrome-entry: #e1e1ea; --pelt-chrome-muted: #a0a0b0; \
        --pelt-chrome-diagnostic-border: #596071; --pelt-chrome-loading-text: #dbeeff; --pelt-chrome-loading-surface: #1d2839; --pelt-chrome-loading-border: #527aa6; \
        --pelt-chrome-error-text: #ffe7e5; --pelt-chrome-error-surface: #392025; --pelt-chrome-error-border: #a35e63; \
        --pelt-chrome-diagnostic-address: #bfe0ff; --pelt-chrome-diagnostic-note: #d2d2df; \
        --pelt-chrome-tabbar: #33333a; --pelt-chrome-tab-text: #cccccc; --pelt-chrome-tab-surface: #2a2a30; \
        --pelt-chrome-tab-active-text: #ffffff; --pelt-chrome-tab-active-surface: #4a4a55; --pelt-chrome-tab-close: #e7e9f0; \
        --pelt-chrome-tab-close-surface: #383946; --pelt-chrome-tab-close-border: #707487; \
        --pelt-chrome-tab-close-active-surface: #535768; --pelt-chrome-tab-close-active-border: #b7c2d9; \
        --pelt-chrome-content-surface: #ffffff; --pelt-chrome-divider: #1a1a1f; \
        position: relative; display: flex; flex-direction: column; width: 100%; height: 100%; min-height: 0; background: var(--pelt-chrome-workspace); \
    } \
    .pelt-chrome { display: flex; flex-direction: column; flex-grow: 0; flex-shrink: 0; flex-basis: 40px; min-height: 40px; padding: 0 0 0 6px; background: var(--pelt-chrome-surface); border-bottom: 1px solid var(--pelt-chrome-border); } \
    .pelt-toolbar { display: flex; align-items: center; flex-grow: 0; flex-shrink: 0; flex-basis: 40px; min-height: 40px; min-width: 0; } \
    .pelt-chrome-button { flex-grow: 0; flex-shrink: 0; flex-basis: 28px; width: 28px; height: 28px; margin-right: 6px; padding: 4px 0; text-align: center; color: var(--pelt-chrome-control-text); background: var(--pelt-chrome-control-surface); border: 1px solid var(--pelt-chrome-control-border); } \
    .pelt-chrome-button.disabled { color: var(--pelt-chrome-disabled-text); background: var(--pelt-chrome-disabled-surface); border-color: var(--pelt-chrome-disabled-border); } \
    .pelt-chrome-toggle { flex-basis: 62px; width: 62px; margin-left: 6px; font-size: 12px; } \
    .pelt-chrome-toggle-open { color: var(--pelt-chrome-accent-text); background: var(--pelt-chrome-accent-surface); border-color: var(--pelt-chrome-accent-border); } \
    .pelt-address { display: block; flex-grow: 1; flex-shrink: 1; flex-basis: 320px; min-width: 160px; max-width: 560px; height: 28px; padding: 5px 8px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-address-text); background: var(--pelt-chrome-address-surface); border: 1px solid var(--pelt-chrome-control-border); } \
    .pelt-engine { flex-grow: 0; flex-shrink: 0; flex-basis: 112px; width: 112px; height: 28px; margin-left: 6px; padding: 5px 7px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-context-text); background: var(--pelt-chrome-context-surface); border: 1px solid var(--pelt-chrome-context-border); } \
    .pelt-engine-open { color: var(--pelt-chrome-accent-text); background: var(--pelt-chrome-accent-surface); border-color: var(--pelt-chrome-accent-border); } \
    .pelt-engine-menu { position: absolute; top: 40px; left: 0px; right: 0px; z-index: 4; display: flex; flex-direction: row; min-height: 32px; padding: 4px 6px; background: var(--pelt-chrome-surface); border-bottom: 1px solid var(--pelt-chrome-border); } \
    .pelt-engine-option { display: block; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; height: 28px; padding: 5px 8px; overflow: hidden; white-space: nowrap; text-align: center; color: var(--pelt-chrome-context-text); background: var(--pelt-chrome-context-surface); border: 1px solid var(--pelt-chrome-context-border); } \
    .pelt-engine-option-selected { color: var(--pelt-chrome-selection-text); background: var(--pelt-chrome-selection-surface); border-color: var(--pelt-chrome-selection-border); } \
    .pelt-title { flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 90px; margin-left: 12px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-heading); font-size: 13px; } \
    .pelt-route { flex-grow: 0; flex-shrink: 2; flex-basis: auto; min-width: 0; max-width: 220px; margin-left: 8px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-route); font-size: 12px; } \
    .pelt-status { flex-grow: 0; flex-shrink: 1; flex-basis: auto; min-width: 40px; max-width: 150px; margin-left: 8px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-status); font-size: 12px; } \
    .pelt-caption-button { flex-grow: 0; flex-shrink: 0; flex-basis: 44px; width: 44px; height: 40px; margin-left: 0; padding: 13px 0; text-align: center; line-height: 1; font-size: 13px; color: var(--pelt-chrome-control-text); } \
    .pelt-body { position: relative; display: flex; flex-direction: row; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; min-height: 0; } \
    .pelt-pane { display: flex; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; min-height: 0; } \
    .pelt-inspector { position: absolute; top: 0px; right: 0px; bottom: 0px; z-index: 1; display: flex; flex-direction: column; width: 248px; min-width: 248px; min-height: 0; padding: 8px; overflow: hidden; pointer-events: none; color: var(--pelt-chrome-panel-text); background: var(--pelt-chrome-panel-surface); border-left: 1px solid var(--pelt-chrome-panel-border); } \
    .pelt-inspector-heading { flex-grow: 0; flex-shrink: 0; color: var(--pelt-chrome-heading); font-size: 14px; font-weight: bold; } \
    .pelt-inspector-capability { flex-grow: 0; flex-shrink: 0; margin-top: 4px; color: var(--pelt-chrome-route); font-size: 12px; } \
    .pelt-inspector-title { flex-grow: 0; flex-shrink: 0; margin-top: 6px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-heading); font-size: 13px; } \
    .pelt-inspector-summary { flex-grow: 0; flex-shrink: 0; margin-top: 4px; color: var(--pelt-chrome-summary); font-size: 12px; } \
    .pelt-inspector-section { flex-grow: 0; flex-shrink: 0; margin-top: 8px; color: var(--pelt-chrome-section); font-size: 12px; } \
    .pelt-inspector-entry { flex-grow: 0; flex-shrink: 0; padding-left: 6px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-entry); font-size: 12px; } \
    .pelt-inspector-more { flex-grow: 0; flex-shrink: 0; padding-left: 6px; color: var(--pelt-chrome-muted); font-size: 12px; } \
    .pelt-appearance { position: absolute; top: 0px; right: 0px; bottom: 0px; z-index: 3; display: flex; flex-direction: column; width: 260px; min-width: 260px; min-height: 0; padding: 16px; overflow: hidden; color: var(--pelt-chrome-panel-text); background: var(--pelt-chrome-panel-surface); border-left: 1px solid var(--pelt-chrome-panel-border); } \
    .pelt-appearance-heading { flex-grow: 0; flex-shrink: 0; color: var(--pelt-chrome-heading); font-size: 16px; font-weight: bold; } \
    .pelt-appearance-label { flex-grow: 0; flex-shrink: 0; margin-top: 16px; color: var(--pelt-chrome-route); font-size: 13px; } \
    .pelt-appearance-options { display: flex; flex-direction: row; flex-grow: 0; flex-shrink: 0; flex-basis: 32px; min-width: 0; margin-top: 6px; } \
    .pelt-appearance-option { flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; padding: 7px 8px; overflow: hidden; white-space: nowrap; text-align: center; color: var(--pelt-chrome-context-text); background: var(--pelt-chrome-context-surface); border: 1px solid var(--pelt-chrome-context-border); } \
    .pelt-appearance-option-selected { color: var(--pelt-chrome-selection-text); background: var(--pelt-chrome-selection-surface); border-color: var(--pelt-chrome-selection-border); } \
    .pelt-appearance-scope { flex-grow: 0; flex-shrink: 0; margin-top: 18px; color: var(--pelt-chrome-entry); font-size: 13px; } \
    .pelt-appearance-note { flex-grow: 0; flex-shrink: 0; margin-top: 8px; color: var(--pelt-chrome-muted); font-size: 12px; } \
    .pelt-diagnostic { position: absolute; z-index: 2; display: flex; flex-direction: column; box-sizing: border-box; min-width: 0; min-height: 0; padding: 24px; overflow: hidden; pointer-events: none; border: 1px solid var(--pelt-chrome-diagnostic-border); } \
    .pelt-diagnostic-loading { color: var(--pelt-chrome-loading-text); background: var(--pelt-chrome-loading-surface); border-color: var(--pelt-chrome-loading-border); } \
    .pelt-diagnostic-error { color: var(--pelt-chrome-error-text); background: var(--pelt-chrome-error-surface); border-color: var(--pelt-chrome-error-border); } \
    .pelt-diagnostic-heading { flex-grow: 0; flex-shrink: 0; color: var(--pelt-chrome-heading); font-size: 20px; font-weight: bold; } \
    .pelt-diagnostic-address { flex-grow: 0; flex-shrink: 0; margin-top: 10px; overflow: hidden; white-space: nowrap; color: var(--pelt-chrome-diagnostic-address); font-size: 13px; } \
    .pelt-diagnostic-message { flex-grow: 0; flex-shrink: 0; margin-top: 16px; color: inherit; font-size: 14px; } \
    .pelt-diagnostic-note { flex-grow: 0; flex-shrink: 0; margin-top: 10px; color: var(--pelt-chrome-diagnostic-note); font-size: 13px; } \
    .pelt-workspace .frisket-tabbar { padding-left: 6px; background: var(--pelt-chrome-tabbar); } \
    .pelt-workspace .frisket-tab { flex-grow: 0; flex-shrink: 1; flex-basis: auto; min-width: 96px; color: var(--pelt-chrome-tab-text); background: var(--pelt-chrome-tab-surface); } \
    .pelt-workspace .frisket-label { flex-grow: 0; flex-shrink: 1; flex-basis: auto; text-overflow: ellipsis; } \
    .pelt-workspace .frisket-tab.active { color: var(--pelt-chrome-tab-active-text); background: var(--pelt-chrome-tab-active-surface); } \
    .pelt-workspace .frisket-tab.active .frisket-label { font-weight: bold; } \
    .pelt-workspace .frisket-close { display: flex; align-items: center; justify-content: center; flex-grow: 0; flex-shrink: 0; flex-basis: 28px; width: 28px; height: 28px; margin-left: 4px; padding: 0; line-height: 1; font-size: 18px; font-weight: bold; color: var(--pelt-chrome-tab-close); background: var(--pelt-chrome-tab-close-surface); border: 1px solid var(--pelt-chrome-tab-close-border); } \
    .pelt-workspace .frisket-tab.active .frisket-close { background: var(--pelt-chrome-tab-close-active-surface); border-color: var(--pelt-chrome-tab-close-active-border); } \
    .pelt-workspace .frisket-content { background: var(--pelt-chrome-content-surface); } \
    .pelt-workspace .frisket-divider { background: var(--pelt-chrome-divider); } \
    @media (max-width: 800px) { \
        .pelt-title, .pelt-route, .pelt-status { display: none; } \
    } \
    @media (max-width: 640px) { \
        .pelt-caption-button { display: none; } \
        .pelt-engine { flex-basis: 70px; width: 70px; margin-left: 4px; padding: 5px 3px; font-size: 10px; } \
    } \
    @media (max-width: 520px) { \
        .pelt-address { min-width: 140px; } \
        .frisket-tab { padding: 8px 1px; font-size: 12px; } \
        .frisket-close { flex-basis: 28px; width: 28px; margin-left: 2px; } \
    } \
    @media (max-width: 440px) { \
        .pelt-chrome-toggle { display: none; } \
    }";

const PELT_LIGHT_THEME_CSS: &str = "\
    .pelt-workspace.pelt-theme-light { \
        --pelt-chrome-workspace: #f4f6f8; --pelt-chrome-surface: #f7f8fa; --pelt-chrome-border: #c7ccd4; \
        --pelt-chrome-control-text: #202933; --pelt-chrome-control-surface: #ffffff; --pelt-chrome-control-border: #aeb7c3; \
        --pelt-chrome-disabled-text: #8d97a3; --pelt-chrome-disabled-surface: #edf0f3; --pelt-chrome-disabled-border: #d7dce3; \
        --pelt-chrome-accent-text: #153e60; --pelt-chrome-accent-surface: #e0f0ff; --pelt-chrome-accent-border: #79acd5; \
        --pelt-chrome-address-text: #1d2730; --pelt-chrome-address-surface: #ffffff; \
        --pelt-chrome-context-text: #234f75; --pelt-chrome-context-surface: #edf5fb; --pelt-chrome-context-border: #9bbbd5; \
        --pelt-chrome-selection-text: #133e64; --pelt-chrome-selection-surface: #d9edff; --pelt-chrome-selection-border: #75add8; \
        --pelt-chrome-heading: #202933; --pelt-chrome-route: #18618e; --pelt-chrome-status: #276638; \
        --pelt-chrome-panel-text: #27323d; --pelt-chrome-panel-surface: #ffffff; --pelt-chrome-panel-border: #c7ccd4; \
        --pelt-chrome-summary: #3d4752; --pelt-chrome-section: #18618e; --pelt-chrome-entry: #3d4752; --pelt-chrome-muted: #67727e; \
        --pelt-chrome-diagnostic-border: #c7ccd4; --pelt-chrome-loading-text: #183f63; --pelt-chrome-loading-surface: #eaf4ff; --pelt-chrome-loading-border: #73a5d5; \
        --pelt-chrome-error-text: #7a242a; --pelt-chrome-error-surface: #fff1f0; --pelt-chrome-error-border: #d88488; \
        --pelt-chrome-diagnostic-address: #18527d; --pelt-chrome-diagnostic-note: #4e5965; \
        --pelt-chrome-tabbar: #e3e7ec; --pelt-chrome-tab-text: #4c5966; --pelt-chrome-tab-surface: #f5f7f9; \
        --pelt-chrome-tab-active-text: #202933; --pelt-chrome-tab-active-surface: #d9e8f5; --pelt-chrome-tab-close: #1f3448; \
        --pelt-chrome-tab-close-surface: #ffffff; --pelt-chrome-tab-close-border: #8aa7c1; \
        --pelt-chrome-tab-close-active-surface: #eef7ff; --pelt-chrome-tab-close-active-border: #5c93be; \
        --pelt-chrome-content-surface: #ffffff; --pelt-chrome-divider: #cbd2da; \
    }";

fn attr(dom: &ScriptedDom, node: NodeId, name: &str) -> Option<String> {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .map(|value| value.to_string())
}

fn nodes_with_attr(dom: &ScriptedDom, name: &str) -> Vec<NodeId> {
    fn visit(dom: &ScriptedDom, node: NodeId, name: &str, out: &mut Vec<NodeId>) {
        if attr(dom, node, name).is_some() {
            out.push(node);
        }
        for child in dom.dom_children(node) {
            visit(dom, child, name, out);
        }
    }

    let mut nodes = Vec::new();
    visit(dom, dom.document(), name, &mut nodes);
    nodes
}

fn nodes_in_document(dom: &ScriptedDom) -> Vec<NodeId> {
    fn visit(dom: &ScriptedDom, node: NodeId, out: &mut Vec<NodeId>) {
        out.push(node);
        for child in dom.dom_children(node) {
            visit(dom, child, out);
        }
    }

    let mut nodes = Vec::new();
    visit(dom, dom.document(), &mut nodes);
    nodes
}

fn divider_node(dom: &ScriptedDom, hit: NodeId) -> Option<NodeId> {
    let mut node = hit;
    loop {
        if attr(dom, node, "data-divider").is_some() {
            return Some(node);
        }
        node = dom.parent(node)?;
    }
}

fn workspace_rect(rect: [f32; 4]) -> WorkspaceRect {
    WorkspaceRect::new(rect[0], rect[1], rect[2], rect[3])
}

#[cfg(test)]
mod tests {
    use genet_host_api::tile::{ContentSource, DocumentRef, SplitAxis, Tile, TileBranch};

    use super::*;

    fn tile(id: u64) -> Tile {
        Tile {
            id: TileId(id),
            title: format!("Tile {id}"),
            content: ContentSource::Document(DocumentRef(format!("tile-{id}.html"))),
            accent: None,
        }
    }

    fn nested_tree() -> TileTree {
        TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::stack(vec![tile(1), tile(2)], 0)),
                TileBranch::new(
                    0.5,
                    TileTree::split(
                        SplitAxis::Column,
                        vec![
                            TileBranch::new(0.5, TileTree::single(tile(3))),
                            TileBranch::new(0.5, TileTree::single(tile(4))),
                        ],
                    ),
                ),
            ],
        )
    }

    #[test]
    fn livery_geometry_is_the_content_hole_and_hit_authority() {
        let mut surface = FrisketSurface::new(&nested_tree());
        let frame = surface.frame(800, 600).unwrap();
        assert_eq!(frame.content_rects.len(), 3);
        let rects = frame
            .content_rects
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        let left = rects[&TileId(1)];
        let top_right = rects[&TileId(3)];
        let bottom_right = rects[&TileId(4)];
        assert!(left.x < top_right.x);
        assert!(top_right.y < bottom_right.y);
        assert!(
            left.height > top_right.height,
            "left={left:?} top_right={top_right:?} bottom_right={bottom_right:?}"
        );
        assert_eq!(
            surface.hit(left.x + left.width / 2.0, left.y + left.height / 2.0),
            Some(FrisketHit::Content(TileId(1)))
        );

        let tab = surface
            .rect_for_attr("data-tabid", "2")
            .expect("second tab");
        assert_eq!(
            surface.hit(tab.x + 4.0, tab.y + tab.height / 2.0),
            Some(FrisketHit::Tab(TileId(2)))
        );
        let divider = surface
            .rect_for_attr("data-divider", "")
            .expect("root divider");
        assert!(matches!(
            surface.hit(
                divider.x + divider.width / 2.0,
                divider.y + divider.height / 2.0
            ),
            Some(FrisketHit::Divider { .. })
        ));
    }

    #[test]
    fn chrome_reserves_host_geometry_and_keeps_tab_labels_compact() {
        let mut tree = nested_tree();
        tree.tile_mut(TileId(1)).expect("first tile").title =
            "A deliberately long workspace title that would collide with its close control"
                .to_owned();
        let mut surface = FrisketSurface::new(&tree);
        surface.set_chrome(Some(WorkspaceChrome {
            title: "Focused document".to_owned(),
            address: "C:/example/static.html".to_owned(),
            route: "Automatic: genet.livery · document".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Dark,
            address_focused: false,
            can_go_back: false,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: true,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![
                ChromeEngineChoice::Automatic,
                ChromeEngineChoice::Livery,
                ChromeEngineChoice::Scripted,
            ],
            inspector: None,
            appearance: None,
            diagnostic: None,
            window_controls: false,
            maximized: false,
        }));
        let frame = surface.frame(800, 600).expect("chrome Frisket frame");
        let address = surface.chrome_rect("address").expect("address geometry");
        let engine = surface.chrome_rect("engine-menu").expect("engine geometry");
        let livery = surface
            .chrome_rect("engine-livery")
            .expect("Livery choice geometry");
        let first = frame
            .content_rects
            .iter()
            .find_map(|(tile, rect)| (*tile == TileId(1)).then_some(*rect))
            .expect("first content geometry");
        assert!(address.width > engine.width);
        assert!(livery.y > address.y);
        assert!(first.y > livery.y + livery.height);
        assert_eq!(
            surface.hit(
                address.x + address.width / 2.0,
                address.y + address.height / 2.0
            ),
            Some(FrisketHit::ChromeAction(ChromeAction::Address))
        );
        assert_eq!(
            surface.hit(
                livery.x + livery.width / 2.0,
                livery.y + livery.height / 2.0
            ),
            Some(FrisketHit::ChromeAction(ChromeAction::ChooseEngine(
                ChromeEngineChoice::Livery
            )))
        );
        let trigger = nodes_with_attr(surface.document.dom(), ATTR_CHROME_ACTION)
            .into_iter()
            .find(|node| {
                attr(surface.document.dom(), *node, ATTR_CHROME_ACTION).as_deref()
                    == Some("engine-menu")
            })
            .expect("engine trigger semantic node");
        assert_eq!(
            attr(surface.document.dom(), trigger, "aria-haspopup").as_deref(),
            Some("menu")
        );
        assert_eq!(
            attr(surface.document.dom(), trigger, "aria-expanded").as_deref(),
            Some("true")
        );
        let menu = nodes_with_attr(surface.document.dom(), "id")
            .into_iter()
            .find(|node| {
                attr(surface.document.dom(), *node, "id").as_deref() == Some("pelt-engine-menu")
            })
            .expect("engine menu semantic node");
        assert_eq!(
            attr(surface.document.dom(), menu, "role").as_deref(),
            Some("menu")
        );
        let automatic = nodes_with_attr(surface.document.dom(), ATTR_CHROME_ACTION)
            .into_iter()
            .find(|node| {
                attr(surface.document.dom(), *node, ATTR_CHROME_ACTION).as_deref()
                    == Some("engine-automatic")
            })
            .expect("automatic choice semantic node");
        assert_eq!(
            attr(surface.document.dom(), automatic, "role").as_deref(),
            Some("menuitemradio")
        );
        assert_eq!(
            attr(surface.document.dom(), automatic, "aria-checked").as_deref(),
            Some("true")
        );
        let back = surface.chrome_rect("back").expect("back geometry");
        assert_eq!(
            surface.hit(back.x + back.width / 2.0, back.y + back.height / 2.0),
            Some(FrisketHit::Chrome)
        );

        let compact = compact_tab_tree(&tree);
        let title = compact
            .find(TileId(1))
            .expect("compact first tile")
            .title
            .as_str();
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= 29);
    }

    #[test]
    fn appended_chrome_stylesheet_recolors_shell_without_moving_content_or_close_targets() {
        let mut chrome = WorkspaceChrome {
            title: "Focused document".to_owned(),
            address: "C:/example/static.html".to_owned(),
            route: "Automatic: genet.livery · document".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Dark,
            address_focused: false,
            can_go_back: false,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: false,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: None,
            appearance: None,
            diagnostic: None,
            window_controls: false,
            maximized: false,
        };
        let mut surface = FrisketSurface::new(&nested_tree());
        surface.set_chrome(Some(chrome.clone()));
        let baseline = surface.frame(800, 600).expect("baseline Chrome frame");
        let baseline_color = surface
            .chrome_computed_style("pelt-chrome", "background-color")
            .expect("baseline Chrome surface color");
        let baseline_close = surface.close_rect(TileId(1)).expect("first close target");

        surface.set_chrome_stylesheet(Some(
            ".pelt-workspace, .pelt-workspace.pelt-theme-light { \
                --pelt-chrome-surface: #123456; --pelt-chrome-tabbar: #102030; \
                --pelt-chrome-content-surface: #f0e0d0; --pelt-chrome-divider: #a0b0c0; \
            }"
            .to_owned(),
        ));
        let preview = surface.frame(800, 600).expect("preview Chrome frame");
        assert_eq!(preview.content_rects, baseline.content_rects);
        assert_eq!(surface.close_rect(TileId(1)), Some(baseline_close));
        assert_eq!(
            surface
                .chrome_computed_style("pelt-chrome", "background-color")
                .as_deref(),
            Some("rgb(18, 52, 86)")
        );

        // Light's built-in variables have higher specificity than a bare
        // workspace selector, so a portable artifact maps both selectors.
        chrome.theme = AppearanceTheme::Light;
        surface.set_chrome(Some(chrome.clone()));
        let light_preview = surface.frame(800, 600).expect("light preview Chrome frame");
        assert_eq!(light_preview.content_rects, baseline.content_rects);
        assert_eq!(surface.close_rect(TileId(1)), Some(baseline_close));
        assert_eq!(
            surface
                .chrome_computed_style("pelt-chrome", "background-color")
                .as_deref(),
            Some("rgb(18, 52, 86)")
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-tabbar", "background-color")
                .as_deref(),
            Some("rgb(16, 32, 48)")
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-content", "background-color")
                .as_deref(),
            Some("rgb(240, 224, 208)")
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-divider", "background-color")
                .as_deref(),
            Some("rgb(160, 176, 192)")
        );

        chrome.theme = AppearanceTheme::Dark;
        surface.set_chrome(Some(chrome));
        surface.set_chrome_stylesheet(None);
        let restored = surface.frame(800, 600).expect("restored Chrome frame");
        assert_eq!(restored.content_rects, baseline.content_rects);
        assert_eq!(surface.close_rect(TileId(1)), Some(baseline_close));
        assert_eq!(
            surface
                .chrome_computed_style("pelt-chrome", "background-color")
                .as_deref(),
            Some(baseline_color.as_str())
        );
    }

    #[test]
    fn chrome_stylesheet_waits_for_the_workspace_shell() {
        let mut surface = FrisketSurface::new(&nested_tree());
        surface.set_chrome_stylesheet(Some(".frisket-content { background: #123456; }".to_owned()));
        surface
            .frame(800, 600)
            .expect("unstyled Frisket fallback frame");
        assert_eq!(
            surface
                .chrome_computed_style("frisket-content", "background-color")
                .as_deref(),
            Some("rgb(255, 255, 255)"),
            "a Chrome author sheet cannot style the generic Frisket fallback"
        );
    }

    #[test]
    fn narrow_chrome_keeps_one_fixed_row_and_sheds_secondary_controls() {
        let mut tree = nested_tree();
        tree.tile_mut(TileId(1)).expect("first tile").title =
            "A deliberately long workspace title that keeps the close target visible".to_owned();
        let mut surface = FrisketSurface::new(&tree);
        let chrome = WorkspaceChrome {
            title: "Focused document".to_owned(),
            address: "C:/example/static.html".to_owned(),
            route: "Automatic: genet.livery · document".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Dark,
            address_focused: false,
            can_go_back: false,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: false,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: None,
            appearance: None,
            diagnostic: None,
            window_controls: false,
            maximized: false,
        };
        surface.set_chrome(Some(chrome.clone()));
        let frame = surface.frame(360, 480).expect("narrow Chrome frame");
        let within = |rect: WorkspaceRect| {
            rect.width > 0.0
                && rect.height > 0.0
                && rect.x >= 0.0
                && rect.y >= 0.0
                && rect.x + rect.width <= 360.0
                && rect.y + rect.height <= 480.0
        };
        let back = surface.chrome_rect("back").expect("back geometry");
        let address = surface.chrome_rect("address").expect("address geometry");
        for action in ["forward", "reload", "engine-menu"] {
            assert!(
                within(
                    surface
                        .chrome_rect(action)
                        .expect("narrow Chrome control geometry")
                ),
                "{action} stays inside the small Chrome viewport"
            );
        }
        for action in ["inspect", "appearance"] {
            assert!(
                surface.chrome_rect(action).is_none(),
                "{action} is shed at this width instead of crowding the fixed row"
            );
        }
        assert!(within(back));
        assert!(within(address));
        assert!(
            address.y < back.y + back.height && address.y + address.height > back.y,
            "the address shares the single fixed-height row with navigation"
        );
        assert!(
            address.width >= 120.0,
            "the narrow address keeps a usable minimum width"
        );
        let content = frame
            .content_rects
            .iter()
            .find_map(|(tile, rect)| (*tile == TileId(1)).then_some(*rect))
            .expect("first content hole");
        assert!(content.y > address.y + address.height);
        let tab = surface.tab_rect(TileId(1)).expect("first tab geometry");
        let label = surface
            .tab_label_rect(TileId(1))
            .expect("first tab label geometry");
        let close = surface
            .close_rect(TileId(1))
            .expect("first tab close geometry");
        assert!(within(tab));
        assert!(within(label));
        assert!(within(close));
        assert_eq!(close.width, 28.0);
        assert_eq!(close.height, 28.0);
        assert!(
            label.width >= 48.0,
            "the tab retains visible label space: label={label:?} tab={tab:?} close={close:?}"
        );
        assert!(close.x >= tab.x && close.x + close.width <= tab.x + tab.width);
        assert_eq!(
            surface.hit(label.x + label.width / 2.0, label.y + label.height / 2.0),
            Some(FrisketHit::Tab(TileId(1)))
        );
        assert_eq!(
            surface.hit(close.x + close.width / 2.0, close.y + close.height / 2.0),
            Some(FrisketHit::Close(TileId(1)))
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-label", "font-weight")
                .as_deref(),
            Some("bold")
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-close", "background-color")
                .as_deref(),
            Some("rgb(83, 87, 104)")
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-close", "border-top-color")
                .as_deref(),
            Some("rgb(183, 194, 217)")
        );

        let mut light_chrome = chrome;
        light_chrome.theme = AppearanceTheme::Light;
        surface.set_chrome(Some(light_chrome));
        surface.frame(360, 480).expect("narrow light Chrome frame");
        assert_eq!(
            surface
                .chrome_computed_style("frisket-close", "background-color")
                .as_deref(),
            Some("rgb(238, 247, 255)")
        );
        assert_eq!(
            surface
                .chrome_computed_style("frisket-close", "border-top-color")
                .as_deref(),
            Some("rgb(92, 147, 190)")
        );
    }

    fn document_text(dom: &ScriptedDom) -> String {
        nodes_in_document(dom)
            .into_iter()
            .filter_map(|node| dom.text(node).map(str::to_owned))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn loading_and_error_documents_overlay_one_content_hole_without_blocking_it() {
        let chrome = WorkspaceChrome {
            title: "Focused document".to_owned(),
            address: "C:/example/index.html".to_owned(),
            route: "Automatic: genet.livery · document".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Dark,
            address_focused: false,
            can_go_back: true,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: false,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: None,
            appearance: None,
            diagnostic: None,
            window_controls: false,
            maximized: false,
        };
        let mut surface = FrisketSurface::new(&nested_tree());
        surface.set_chrome(Some(chrome.clone()));
        let baseline = surface.frame(800, 600).expect("baseline chrome frame");
        let first = baseline
            .content_rects
            .iter()
            .find_map(|(tile, rect)| (*tile == TileId(1)).then_some(*rect))
            .expect("first content hole");

        surface.set_chrome(Some(WorkspaceChrome {
            diagnostic: Some(ChromeDocument {
                kind: ChromeDocumentKind::Loading,
                tile: TileId(1),
                rect: first,
                address: "C:/example/next.html".to_owned(),
                message: None,
            }),
            ..chrome.clone()
        }));
        let loading = surface.frame(800, 600).expect("loading document frame");
        assert_eq!(loading.content_rects, baseline.content_rects);
        assert_eq!(loading.diagnostic_rect, Some(first));
        let loading_node = nodes_with_attr(surface.document.dom(), ATTR_DIAGNOSTIC)
            .into_iter()
            .next()
            .expect("loading document node");
        assert_eq!(
            attr(surface.document.dom(), loading_node, "role").as_deref(),
            Some("status")
        );
        assert_eq!(
            surface.hit(first.x + first.width / 2.0, first.y + first.height / 2.0),
            Some(FrisketHit::Content(TileId(1))),
            "the read-only diagnostic leaves the underlying content route live"
        );
        assert!(document_text(surface.document.dom()).contains("Loading"));

        assert_eq!(
            diagnostic_error_summary(&ChromeDocument {
                kind: ChromeDocumentKind::Error,
                tile: TileId(1),
                rect: first,
                address: "C:/example/missing.html".to_owned(),
                message: Some(
                    "could not load C:/example/missing.html: session spawn failed: could not load C:/example/missing.html"
                        .to_owned(),
                ),
            }),
            "session spawn failed: could not load C:/example/missing.html"
        );

        surface.set_chrome(Some(WorkspaceChrome {
            diagnostic: Some(ChromeDocument {
                kind: ChromeDocumentKind::Error,
                tile: TileId(1),
                rect: first,
                address: "C:/example/missing.html".to_owned(),
                message: Some("could not load C:/example/missing.html".to_owned()),
            }),
            ..chrome
        }));
        let error = surface.frame(800, 600).expect("error document frame");
        assert_eq!(error.diagnostic_rect, Some(first));
        let error_node = nodes_with_attr(surface.document.dom(), ATTR_DIAGNOSTIC)
            .into_iter()
            .next()
            .expect("error document node");
        assert_eq!(
            attr(surface.document.dom(), error_node, "role").as_deref(),
            Some("alert")
        );
        let text = document_text(surface.document.dom());
        assert!(text.contains("Could not load"));
        assert!(text.contains("previous document and its history remain available"));
    }

    #[test]
    fn appearance_drawer_is_live_and_themed_without_moving_content() {
        let chrome = WorkspaceChrome {
            title: "Focused document".to_owned(),
            address: "C:/example/index.html".to_owned(),
            route: "Automatic: genet.livery · document".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Dark,
            address_focused: false,
            can_go_back: true,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: false,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: None,
            appearance: None,
            diagnostic: None,
            window_controls: false,
            maximized: false,
        };
        let mut surface = FrisketSurface::new(&nested_tree());
        surface.set_chrome(Some(chrome.clone()));
        let baseline = surface.frame(800, 600).expect("baseline chrome frame");

        surface.set_chrome(Some(WorkspaceChrome {
            theme: AppearanceTheme::Light,
            appearance: Some(ChromeAppearance {
                theme: AppearanceTheme::Light,
                persistent: false,
            }),
            ..chrome
        }));
        let light = surface.frame(800, 600).expect("light appearance frame");
        assert_eq!(light.content_rects, baseline.content_rects);
        let appearance_rect = light.appearance_rect.expect("appearance drawer geometry");
        assert!(appearance_rect.width >= 260.0);
        assert!(appearance_rect.height > 0.0);
        let light_choice = surface
            .chrome_rect("appearance-light")
            .expect("Light choice geometry");
        assert_eq!(
            surface.hit(
                light_choice.x + light_choice.width / 2.0,
                light_choice.y + light_choice.height / 2.0
            ),
            Some(FrisketHit::ChromeAction(ChromeAction::ChooseTheme(
                AppearanceTheme::Light
            )))
        );
        assert_eq!(
            surface.hit(
                appearance_rect.x + 5.0,
                appearance_rect.y + appearance_rect.height - 5.0
            ),
            Some(FrisketHit::Appearance),
            "the drawer captures its unused surface instead of forwarding it to content"
        );

        let region = nodes_with_attr(surface.document.dom(), ATTR_APPEARANCE)
            .into_iter()
            .next()
            .expect("appearance region");
        assert_eq!(
            attr(surface.document.dom(), region, "role").as_deref(),
            Some("region")
        );
        assert_eq!(
            attr(surface.document.dom(), region, "aria-label").as_deref(),
            Some("Appearance")
        );
        let selected = nodes_with_attr(surface.document.dom(), ATTR_CHROME_ACTION)
            .into_iter()
            .find(|node| {
                attr(surface.document.dom(), *node, ATTR_CHROME_ACTION).as_deref()
                    == Some("appearance-light")
            })
            .expect("selected Light radio");
        assert_eq!(
            attr(surface.document.dom(), selected, "role").as_deref(),
            Some("radio")
        );
        assert_eq!(
            attr(surface.document.dom(), selected, "aria-checked").as_deref(),
            Some("true")
        );
        let root = nodes_with_attr(surface.document.dom(), "class")
            .into_iter()
            .find(|node| {
                attr(surface.document.dom(), *node, "class")
                    .is_some_and(|class| class.contains("pelt-workspace"))
            })
            .expect("workspace root");
        assert!(
            attr(surface.document.dom(), root, "class")
                .is_some_and(|class| class.contains("pelt-theme-light"))
        );
        let text = document_text(surface.document.dom());
        assert!(text.contains("This Pelt session only."));
        assert!(text.contains("Document content keeps its engine-owned theme."));
    }

    #[test]
    fn accesskit_projection_keeps_shell_controls_and_declared_content_boundaries() {
        let chrome = WorkspaceChrome {
            title: "Focused document".to_owned(),
            address: "C:/example/index.html".to_owned(),
            route: "Automatic: genet.livery · document".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Light,
            address_focused: false,
            can_go_back: true,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: true,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: None,
            appearance: Some(ChromeAppearance {
                theme: AppearanceTheme::Light,
                persistent: false,
            }),
            diagnostic: None,
            window_controls: false,
            maximized: false,
        };
        let mut surface = FrisketSurface::new(&nested_tree());
        surface.set_chrome(Some(chrome.clone()));
        surface.set_content_accessibility([FrisketContentA11y {
            tile: TileId(1),
            label: "Tile 1 content".to_owned(),
            description: "The engine declares partial accessibility. Pelt does not compose its document semantics into this workspace tree yet.".to_owned(),
        }]);
        surface.frame(800, 600).expect("retained Frisket frame");
        let projection = surface
            .accessibility_projection(None)
            .expect("completed retained Frisket layout");

        let content = projection
            .tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Region && node.label() == Some("Tile 1 content"))
            .map(|(_, node)| node)
            .expect("named content aperture");
        assert!(
            content
                .description()
                .is_some_and(|description| description.contains("partial accessibility"))
        );
        let visible_tiles = [TileId(1), TileId(3), TileId(4)];
        assert_eq!(projection.content_nodes.len(), visible_tiles.len());
        for tile in visible_tiles {
            let content_id = projection
                .content_nodes
                .get(&tile)
                .copied()
                .expect("every visible tile has a content aperture node");
            let content_node = projection
                .tree
                .nodes
                .iter()
                .find(|(id, _)| *id == content_id)
                .map(|(_, node)| node)
                .expect("content aperture node is in the shell tree");
            assert_eq!(content_node.role(), Role::Region);
            let tile_attr = tile.0.to_string();
            let dom_content = nodes_with_attr(surface.document.dom(), FRISKET_TILE_ATTR)
                .into_iter()
                .find(|node| {
                    attr(surface.document.dom(), *node, FRISKET_TILE_ATTR).as_deref()
                        == Some(tile_attr.as_str())
                })
                .expect("content aperture has a Frisket DOM node");
            assert_eq!(projection.nodes.get(&content_id), Some(&dom_content));
        }

        let active_tab = projection
            .tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Tile 1"))
            .map(|(_, node)| node)
            .expect("active Frisket tab");
        assert_eq!(active_tab.is_selected(), Some(true));
        assert!(active_tab.supports_action(accesskit::Action::Click));

        let divider = projection
            .tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Splitter)
            .map(|(_, node)| node)
            .expect("Frisket split divider");
        assert_eq!(
            divider.orientation(),
            Some(accesskit::Orientation::Vertical)
        );

        let light = projection
            .tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::RadioButton && node.label() == Some("Light"))
            .expect("Light appearance radio");
        assert_eq!(light.1.toggled(), Some(accesskit::Toggled::True));
        assert!(light.1.supports_action(accesskit::Action::Click));
        assert!(light.1.supports_action(accesskit::Action::Focus));
        let light_dom = projection.nodes[&light.0];
        assert_eq!(
            surface.accessibility_target(light_dom),
            Some(FrisketA11yTarget::ChromeAction(ChromeAction::ChooseTheme(
                AppearanceTheme::Light
            )))
        );

        let address = projection
            .tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::TextInput && node.label() == Some("Address"))
            .map(|(_, node)| node)
            .expect("address accessibility node");
        assert_eq!(address.value(), Some("C:/example/index.html"));
        assert!(address.supports_action(accesskit::Action::SetValue));
        let address_rect = surface.chrome_rect("address").expect("address geometry");
        let bounds = address.bounds().expect("address bounds");
        assert_eq!(bounds.x0, f64::from(address_rect.x));
        assert_eq!(bounds.y0, f64::from(address_rect.y));
        assert_eq!(bounds.x1, f64::from(address_rect.x + address_rect.width));
        assert_eq!(bounds.y1, f64::from(address_rect.y + address_rect.height));

        surface.set_chrome(Some(WorkspaceChrome {
            appearance: None,
            ..chrome
        }));
        surface.frame(800, 600).expect("closed appearance frame");
        let closed = surface
            .accessibility_projection(None)
            .expect("closed retained Frisket layout");
        assert!(closed.tree.nodes.iter().all(|(_, node)| {
            node.role() != Role::RadioButton || node.label() != Some("Light")
        }));
    }

    #[test]
    fn inspector_is_a_retained_region_and_names_opaque_content_honestly() {
        let chrome = WorkspaceChrome {
            title: "Scrying native surface".to_owned(),
            address: "C:/example/surface.html".to_owned(),
            route: "Automatic: scrying.web · surface".to_owned(),
            status: "Ready".to_owned(),
            theme: AppearanceTheme::Dark,
            address_focused: false,
            can_go_back: false,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: false,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: Some(ChromeInspector {
                status: "Opaque surface".to_owned(),
                title: Some("scrying.web".to_owned()),
                summary: "Contents not inspectable on this surface.".to_owned(),
                sections: Vec::new(),
            }),
            appearance: None,
            diagnostic: None,
            window_controls: false,
            maximized: false,
        };
        let mut surface = FrisketSurface::new(&nested_tree());
        surface.set_chrome(Some(chrome.clone()));
        let frame = surface.frame(800, 600).expect("inspector Frisket frame");
        let inspector_rect = frame.inspector_rect.expect("inspector geometry");
        assert!(inspector_rect.width >= 248.0);
        assert!(inspector_rect.height > 0.0);
        let inspector = nodes_with_attr(surface.document.dom(), "role")
            .into_iter()
            .find(|node| attr(surface.document.dom(), *node, "role").as_deref() == Some("region"))
            .expect("content inspector region");
        assert_eq!(
            attr(surface.document.dom(), inspector, "aria-label").as_deref(),
            Some("Content inspector")
        );
        let toggle = surface.chrome_rect("inspect").expect("inspector toggle");
        assert_eq!(
            surface.hit(
                toggle.x + toggle.width / 2.0,
                toggle.y + toggle.height / 2.0
            ),
            Some(FrisketHit::ChromeAction(ChromeAction::ToggleInspector))
        );
        let mut without_inspector = FrisketSurface::new(&nested_tree());
        without_inspector.set_chrome(Some(WorkspaceChrome {
            inspector: None,
            ..chrome
        }));
        let baseline = without_inspector
            .frame(800, 600)
            .expect("baseline chrome Frisket frame");
        assert_eq!(baseline.inspector_rect, None);
        assert_eq!(
            frame.content_rects, baseline.content_rects,
            "the inspector overlays rather than resizes Frisket content holes"
        );
        let surface_tab = surface.tab_rect(TileId(4)).expect("surface tab geometry");
        assert_eq!(
            surface.hit(
                surface_tab.x + surface_tab.width / 2.0,
                surface_tab.y + surface_tab.height / 2.0
            ),
            Some(FrisketHit::Tab(TileId(4))),
            "the read-only inspector does not block workspace tab selection"
        );
        let text = document_text(surface.document.dom());
        assert!(text.contains("Contents not inspectable on this surface."));
        assert!(!text.contains("Headings ("));
        assert!(!text.contains("Links ("));
    }
}
