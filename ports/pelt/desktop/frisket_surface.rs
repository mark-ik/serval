//! Frisket rendered through the owned Livery/Buckram lane.

use std::cell::RefCell;
use std::rc::Rc;

use cambium::{
    AnyView, DividerTarget, DomHandle, FRISKET_CSS, FRISKET_TILE_ATTR, GenetAppRunner, GenetCtx,
    GenetElement, close_target, content_target, divider_target, el, frisket, stack_target,
    tab_drop_index, tab_target,
};
use genet_host_api::tile::{TileId, TilePath, TileTree};
use genet_livery::{Device, LiveryDocument, StyleSet};
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::{LayoutDom, LocalName, Namespace};
use pelt_core::WorkspaceRect;

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
                    "pelt-chrome-button pelt-inspector-toggle pelt-inspector-toggle-open"
                } else {
                    "pelt-chrome-button pelt-inspector-toggle"
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
    ];
    let details: Vec<FrameView> = vec![
        Box::new(el::<_, FrameState, ()>("div", chrome.title.clone()).attr("class", "pelt-title")),
        Box::new(el::<_, FrameState, ()>("div", chrome.route.clone()).attr("class", "pelt-route")),
        Box::new(
            el::<_, FrameState, ()>("div", chrome.status.clone()).attr("class", "pelt-status"),
        ),
    ];
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
    header_items.push(Box::new(
        el::<_, FrameState, ()>("div", details).attr("class", "pelt-details"),
    ));
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
    let mut workspace: Vec<FrameView> = vec![
        header,
        Box::new(el::<_, FrameState, ()>("div", body).attr("class", "pelt-body")),
    ];
    if let Some(diagnostic) = chrome.diagnostic.as_ref() {
        workspace.push(diagnostic_view(diagnostic));
    }
    Box::new(el::<_, FrameState, ()>("div", workspace).attr("class", "pelt-workspace"))
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
}

/// One explicitly available engine choice in the focused-tile Pelt menu.
/// The host decides which of these are registered for a build and preserves
/// route ownership when a row is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChromeEngineChoice {
    Automatic,
    Livery,
    Scripted,
}

impl ChromeEngineChoice {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Livery => "Livery",
            Self::Scripted => "Scripted",
        }
    }

    pub(crate) const fn trigger_label(self) -> &'static str {
        match self {
            Self::Automatic => "Auto",
            Self::Livery => "Livery",
            Self::Scripted => "Scripted",
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Automatic => "engine-automatic",
            Self::Livery => "engine-livery",
            Self::Scripted => "engine-scripted",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Livery => "genet.livery",
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
    pub address_focused: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub engine_label: String,
    pub engine_menu_open: bool,
    pub engine_selected: Option<ChromeEngineChoice>,
    pub engine_choices: Vec<ChromeEngineChoice>,
    pub inspector: Option<ChromeInspector>,
    pub diagnostic: Option<ChromeDocument>,
}

/// A retained, GPU-free pane frame. Its DOM is produced by Cambium Frisket;
/// Livery/Buckram supplies both its scene and the geometry consumed by Pelt.
pub(crate) struct FrisketSurface {
    tree: TileTree,
    chrome: Option<WorkspaceChrome>,
    viewport: (u32, u32),
    document: LiveryDocument<ScriptedDom>,
}

impl FrisketSurface {
    pub fn new(tree: &TileTree) -> Self {
        let viewport = (800, 600);
        Self {
            tree: tree.clone(),
            chrome: None,
            viewport,
            document: document_for(tree, None, viewport.0, viewport.1),
        }
    }

    pub fn set_tree(&mut self, tree: &TileTree) {
        self.tree = tree.clone();
        self.document = document_for(tree, self.chrome.as_ref(), self.viewport.0, self.viewport.1);
    }

    pub fn set_chrome(&mut self, chrome: Option<WorkspaceChrome>) {
        if self.chrome.as_ref() == chrome.as_ref() {
            return;
        }
        self.chrome = chrome;
        self.document = document_for(
            &self.tree,
            self.chrome.as_ref(),
            self.viewport.0,
            self.viewport.1,
        );
    }

    pub fn frame(&mut self, width: u32, height: u32) -> Result<FrisketFrame, String> {
        let viewport = (width.max(1), height.max(1));
        if self.viewport != viewport {
            self.viewport = viewport;
            self.document = document_for(&self.tree, self.chrome.as_ref(), viewport.0, viewport.1);
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
        Ok(FrisketFrame {
            scene: paint_list_render::translate_paint_list(&list),
            content_rects,
            inspector_rect,
            diagnostic_rect,
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
}

fn document_for(
    tree: &TileTree,
    chrome: Option<&WorkspaceChrome>,
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
    LiveryDocument::new(
        dom,
        StyleSet::cambium(&[&host_css, FRISKET_CSS, PELT_CHROME_CSS]),
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
                "engine-scripted" => Some(ChromeAction::ChooseEngine(ChromeEngineChoice::Scripted)),
                "inspect" => Some(ChromeAction::ToggleInspector),
                _ => None,
            };
        }
        node = dom.parent(node)?;
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
    .pelt-workspace { position: relative; display: flex; flex-direction: column; width: 100%; height: 100%; min-height: 0; background: #202027; } \
    .pelt-chrome { display: flex; flex-direction: column; flex-grow: 0; flex-shrink: 0; flex-basis: 70px; min-height: 70px; padding: 4px 6px; background: #24242d; border-bottom: 1px solid #3c3c48; } \
    .pelt-chrome-menu-open { flex-basis: 108px; min-height: 108px; } \
    .pelt-toolbar { display: flex; align-items: center; flex-grow: 0; flex-shrink: 0; flex-basis: 32px; min-width: 0; } \
    .pelt-chrome-button { flex-grow: 0; flex-shrink: 0; flex-basis: 28px; width: 28px; height: 28px; margin-right: 4px; padding: 4px 0; text-align: center; color: #e8e8ee; background: #3a3a46; border: 1px solid #555565; } \
    .pelt-chrome-button.disabled { color: #777783; background: #2c2c34; border-color: #363640; } \
    .pelt-inspector-toggle { flex-basis: 62px; width: 62px; margin-left: 5px; font-size: 12px; } \
    .pelt-inspector-toggle-open { color: #ffffff; background: #41506b; border-color: #83a3d5; } \
    .pelt-address { display: block; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; height: 28px; padding: 5px 8px; overflow: hidden; white-space: nowrap; color: #f0f0f4; background: #16161d; border: 1px solid #555565; } \
    .pelt-engine { flex-grow: 0; flex-shrink: 0; flex-basis: 112px; width: 112px; height: 28px; margin-left: 5px; padding: 5px 7px; overflow: hidden; white-space: nowrap; color: #bfe9ff; background: #30384a; border: 1px solid #566d91; } \
    .pelt-engine-open { color: #ffffff; background: #41506b; border-color: #83a3d5; } \
    .pelt-engine-menu { display: flex; flex-direction: row; flex-grow: 0; flex-shrink: 0; flex-basis: 28px; min-height: 28px; margin: 2px 0; } \
    .pelt-engine-option { display: block; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; height: 28px; padding: 5px 8px; overflow: hidden; white-space: nowrap; text-align: center; color: #c9d9ef; background: #30384a; border: 1px solid #566d91; } \
    .pelt-engine-option-selected { color: #ffffff; background: #46628a; border-color: #9cc8ff; } \
    .pelt-details { display: flex; align-items: center; flex-grow: 0; flex-shrink: 0; flex-basis: 26px; min-width: 0; overflow: hidden; } \
    .pelt-title { flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; overflow: hidden; white-space: nowrap; color: #ffffff; font-size: 13px; } \
    .pelt-route { flex-grow: 0; flex-shrink: 1; flex-basis: auto; min-width: 0; max-width: 260px; margin-left: 8px; overflow: hidden; white-space: nowrap; color: #9ccdf0; font-size: 12px; } \
    .pelt-status { flex-grow: 0; flex-shrink: 1; flex-basis: auto; min-width: 0; max-width: 160px; margin-left: 8px; overflow: hidden; white-space: nowrap; color: #a8d6a8; font-size: 12px; } \
    .pelt-body { position: relative; display: flex; flex-direction: row; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; min-height: 0; } \
    .pelt-pane { display: flex; flex-grow: 1; flex-shrink: 1; flex-basis: 0px; min-width: 0; min-height: 0; } \
    .pelt-inspector { position: absolute; top: 0px; right: 0px; bottom: 0px; z-index: 1; display: flex; flex-direction: column; width: 248px; min-width: 248px; min-height: 0; padding: 8px; overflow: hidden; pointer-events: none; color: #e8e8ee; background: #1b1b23; border-left: 1px solid #3c3c48; } \
    .pelt-inspector-heading { flex-grow: 0; flex-shrink: 0; color: #ffffff; font-size: 14px; font-weight: bold; } \
    .pelt-inspector-capability { flex-grow: 0; flex-shrink: 0; margin-top: 4px; color: #9ccdf0; font-size: 12px; } \
    .pelt-inspector-title { flex-grow: 0; flex-shrink: 0; margin-top: 6px; overflow: hidden; white-space: nowrap; color: #ffffff; font-size: 13px; } \
    .pelt-inspector-summary { flex-grow: 0; flex-shrink: 0; margin-top: 4px; color: #c8c8d4; font-size: 12px; } \
    .pelt-inspector-section { flex-grow: 0; flex-shrink: 0; margin-top: 8px; color: #8bb9eb; font-size: 12px; } \
    .pelt-inspector-entry { flex-grow: 0; flex-shrink: 0; padding-left: 6px; overflow: hidden; white-space: nowrap; color: #e1e1ea; font-size: 12px; } \
    .pelt-inspector-more { flex-grow: 0; flex-shrink: 0; padding-left: 6px; color: #a0a0b0; font-size: 12px; } \
    .pelt-diagnostic { position: absolute; z-index: 2; display: flex; flex-direction: column; box-sizing: border-box; min-width: 0; min-height: 0; padding: 24px; overflow: hidden; pointer-events: none; border: 1px solid #596071; } \
    .pelt-diagnostic-loading { color: #dbeeff; background: #1d2839; border-color: #527aa6; } \
    .pelt-diagnostic-error { color: #ffe7e5; background: #392025; border-color: #a35e63; } \
    .pelt-diagnostic-heading { flex-grow: 0; flex-shrink: 0; color: #ffffff; font-size: 20px; font-weight: bold; } \
    .pelt-diagnostic-address { flex-grow: 0; flex-shrink: 0; margin-top: 10px; overflow: hidden; white-space: nowrap; color: #bfe0ff; font-size: 13px; } \
    .pelt-diagnostic-message { flex-grow: 0; flex-shrink: 0; margin-top: 16px; color: inherit; font-size: 14px; } \
    .pelt-diagnostic-note { flex-grow: 0; flex-shrink: 0; margin-top: 10px; color: #d2d2df; font-size: 13px; }";

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
            diagnostic: None,
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
            address_focused: false,
            can_go_back: true,
            can_go_forward: false,
            engine_label: "Auto".to_owned(),
            engine_menu_open: false,
            engine_selected: Some(ChromeEngineChoice::Automatic),
            engine_choices: vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery],
            inspector: None,
            diagnostic: None,
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
    fn inspector_is_a_retained_region_and_names_opaque_content_honestly() {
        let chrome = WorkspaceChrome {
            title: "Scrying native surface".to_owned(),
            address: "C:/example/surface.html".to_owned(),
            route: "Automatic: scrying.web · surface".to_owned(),
            status: "Ready".to_owned(),
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
            diagnostic: None,
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
