//! Headed recursive Pelt workspace over TileTree and Frisket.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use accesskit::{Action, Affine, NodeId as AccessNodeId, Role, TreeUpdate};
use genet_host_api::tile::{
    ContentSource, DocumentRef, DropTarget, Edge, SplitAxis, Tile, TileBranch, TileEvent, TileId,
    TileTree,
};
use genet_winit_host::{
    A11yActionRequest, AccessKitBridge, BridgeStatus, SurfaceHost, wheel_delta_from_winit,
};
use inker::{
    A11yCapability, EngineProfileBinding, SessionButtonState, SessionCursor, SessionIme,
    SessionInput, SessionKey, SessionModifiers, SessionNavigationCommand, SessionPointerButton,
    SessionRegistry, SessionScrollKey, SurfaceEngineRegistry, SurfaceFrame,
};
#[cfg(target_os = "windows")]
use inker::{FrameHandleOwnership, NativeTextureHandle};
use netrender::external_texture::ExternalTexturePlacement;
use netrender::{ColorLoad, NetrenderOptions, Scene};
use pelt_core::{
    PeltController, PeltDocumentState, PeltHostEffect, PeltRegistries, PeltRouteSource,
    PeltRouteState, PeltTileInspection, PeltTileRequest, PeltWorkspace, WorkspaceRect,
};
#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

#[cfg(target_os = "windows")]
use crate::dx12_surface::Dx12SurfaceCache;
use crate::frisket_surface::{
    ChromeAction, ChromeAppearance, ChromeDocument, ChromeDocumentKind, ChromeEngineChoice,
    ChromeInspector, ChromeInspectorSection, ChromeTheme, FrisketA11yProjection, FrisketA11yTarget,
    FrisketContentA11y, FrisketHit, FrisketSurface, WorkspaceChrome,
};
#[cfg(target_os = "windows")]
use crate::scrying_receipt::{ScryingReceiptEngine, ScryingReceiptHost};
use crate::{WindowingMode, static_viewer};

const RECEIPT_STEPS: u8 = 8;
const WORKSPACE_RECEIPT_STAGE_TIMEOUT: Duration = Duration::from_secs(20);
const MIXED_WORKSPACE_ASSERTION: &str =
    "Gemtext navigation rerouted only tile 1; Livery, scripted, and external neighbors held";
const CHROME_WORKSPACE_ASSERTION: &str = "focused-tile chrome navigated history, bound an explicit engine choice menu, applied a per-tile override, and exposed truthful structural inspection while the mixed workspace held";
const LOADING_ERROR_WORKSPACE_ASSERTION: &str =
    "host-owned loading and error documents preserved the focused tile's prior session and history";
const APPEARANCE_WORKSPACE_ASSERTION: &str =
    "session-only appearance changed the live Pelt chrome theme while the focused document held";
const ACCESSIBILITY_WORKSPACE_ASSERTION: &str = "AccessKit installed before the Pelt window became visible; typed Focus held state while Click opened and selected the session appearance controls";
const NARROW_CHROME_WORKSPACE_ASSERTION: &str = "compact two-row Chrome kept controls, tab text, and close targets usable while loading and error documents held their content hole";
const CHROME_DPI_WORKSPACE_ASSERTION_PREFIX: &str =
    "high-DPI Chrome converted physical pointer input into its retained logical controls";
const INSPECTOR_VISIBLE_ROWS: usize = 3;

/// One bounded semantic receipt for a recursive Pelt workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceReceipt {
    /// A Gemtext link replaces only its own tile through capability routing.
    Mixed,
    /// An unavailable external engine visibly falls back to the owned Livery lane.
    Fallback,
    /// P6 host chrome controls a focused tile while the P5 mixed workspace remains live.
    Chrome,
    /// P6's document-lane loading and failed-navigation projection, without a
    /// native surface dependency.
    LoadingError,
    /// P6's session-only Pelt appearance drawer, without a document theme or
    /// persistence claim.
    Appearance,
    /// P6's retained Chrome/Frisket AccessKit tree and typed action routing.
    Accessibility,
    /// P6's compact Chrome layout at a small logical viewport.
    NarrowChrome,
    /// P6's actual high-DPI Chrome input and capture alignment.
    ChromeDpi,
}

impl WorkspaceReceipt {
    pub fn id(self) -> &'static str {
        match self {
            Self::Mixed => "mixed",
            Self::Fallback => "fallback",
            Self::Chrome => "chrome",
            Self::LoadingError => "loading-error",
            Self::Appearance => "appearance",
            Self::Accessibility => "accessibility",
            Self::NarrowChrome => "narrow-chrome",
            Self::ChromeDpi => "chrome-dpi",
        }
    }

    fn default_size(self) -> (u32, u32) {
        self.logical_viewport().unwrap_or((960, 640))
    }

    fn default_frames(self) -> u32 {
        3
    }

    fn keeps_chrome(self) -> bool {
        matches!(
            self,
            Self::Chrome
                | Self::LoadingError
                | Self::Appearance
                | Self::Accessibility
                | Self::NarrowChrome
                | Self::ChromeDpi
        )
    }

    /// Named geometry receipts pin CSS/layout dimensions in logical pixels;
    /// Winit resolves their physical client size from the active monitor's
    /// actual scale factor before the host boots its device.
    fn logical_viewport(self) -> Option<(u32, u32)> {
        match self {
            Self::NarrowChrome => Some((360, 480)),
            Self::ChromeDpi => Some((960, 640)),
            _ => None,
        }
    }
}

/// Configuration for the recursive Livery workspace.
pub struct WorkspaceViewerConfig {
    pub urls: Vec<String>,
    pub windowing: WindowingMode,
    pub size: Option<(u32, u32)>,
    pub frames: Option<u32>,
    /// Drive the checked-in P3 interaction receipt through the same semantic
    /// pointer and navigation paths as the window.
    pub interaction_receipt: bool,
    /// Drive the mixed P4 routing receipt after the first shared frame.
    pub capability_receipt: bool,
    /// Named P5/P6 workspace receipt with a captured compositor artifact.
    pub workspace_receipt: Option<WorkspaceReceipt>,
    /// Show the retained P6 chrome above the Frisket pane frame.
    pub chrome: bool,
    /// Ordered physical client sizes for a live workspace resize receipt.
    pub workspace_size_matrix: Option<Vec<(u32, u32)>>,
    /// Maximum elapsed time for each mixed-receipt readiness or resize stage.
    pub workspace_receipt_stage_timeout: Duration,
    /// Caller-owned PNG path for the named workspace receipt.
    pub artifact: Option<PathBuf>,
    /// One-based tile number to explicit engine id.
    pub route_overrides: HashMap<u64, String>,
}

impl WorkspaceViewerConfig {
    pub fn new(urls: Vec<String>, windowing: WindowingMode) -> Self {
        Self {
            urls,
            windowing,
            size: None,
            frames: None,
            interaction_receipt: false,
            capability_receipt: false,
            workspace_receipt: None,
            chrome: true,
            workspace_size_matrix: None,
            workspace_receipt_stage_timeout: WORKSPACE_RECEIPT_STAGE_TIMEOUT,
            artifact: None,
            route_overrides: HashMap::new(),
        }
    }

    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = Some((width.max(1), height.max(1)));
        self
    }

    pub fn with_frame_limit(mut self, frames: u32) -> Self {
        self.frames = Some(frames.max(1));
        self
    }

    pub fn with_interaction_receipt(mut self) -> Self {
        self.interaction_receipt = true;
        self.chrome = false;
        self.frames = Some(self.frames.unwrap_or(0).max(u32::from(RECEIPT_STEPS) + 1));
        self
    }

    pub fn with_route_override(mut self, tile: u64, engine_id: impl Into<String>) -> Self {
        self.route_overrides.insert(tile, engine_id.into());
        self
    }

    pub fn with_capability_receipt(mut self) -> Self {
        self.capability_receipt = true;
        self.chrome = false;
        self.frames = Some(self.frames.unwrap_or(0).max(600));
        self
    }

    pub fn with_workspace_receipt(
        mut self,
        receipt: WorkspaceReceipt,
        artifact: impl AsRef<Path>,
    ) -> Self {
        self.size = Some(receipt.default_size());
        self.frames = Some(receipt.default_frames());
        self.workspace_receipt = Some(receipt);
        self.chrome = receipt.keeps_chrome();
        self.artifact = Some(artifact.as_ref().to_owned());
        self
    }

    /// Request an ordered matrix of physical client sizes for a live workspace
    /// receipt. The first size is also the initial window size.
    pub fn with_workspace_size_matrix(mut self, sizes: Vec<(u32, u32)>) -> Self {
        if let Some(&(width, height)) = sizes.first() {
            self.size = Some((width, height));
        }
        self.workspace_size_matrix = Some(sizes);
        self
    }

    pub fn with_workspace_receipt_stage_timeout(mut self, timeout: Duration) -> Self {
        self.workspace_receipt_stage_timeout = timeout.max(Duration::from_millis(1));
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceReceiptOutcome {
    pub id: &'static str,
    pub assertion: String,
    pub artifact: PathBuf,
    pub digest: u64,
    pub verified_sizes: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceViewerOutcome {
    pub first_url: String,
    pub created_window: bool,
    pub redraws: u32,
    pub size: (u32, u32),
    pub tile_count: usize,
    pub interaction_receipt: bool,
    pub capability_receipt: bool,
    pub workspace_receipt: Option<WorkspaceReceiptOutcome>,
    pub routes: Vec<String>,
}

pub fn run_livery_workspace_viewer(
    config: WorkspaceViewerConfig,
) -> Result<WorkspaceViewerOutcome, String> {
    if config.workspace_receipt.is_some()
        && (config.interaction_receipt || config.capability_receipt)
    {
        return Err(
            "a named workspace receipt cannot be combined with the P3 or P4 receipt driver"
                .to_owned(),
        );
    }
    if let Some(sizes) = config.workspace_size_matrix.as_deref() {
        if config.workspace_receipt != Some(WorkspaceReceipt::Mixed) {
            return Err("a workspace size matrix requires the mixed workspace receipt".to_owned());
        }
        if sizes.len() < 2
            || sizes
                .iter()
                .any(|&(width, height)| width == 0 || height == 0)
            || sizes
                .iter()
                .enumerate()
                .any(|(index, size)| sizes[..index].contains(size))
        {
            return Err(
                "a workspace size matrix needs at least two unique positive physical sizes"
                    .to_owned(),
            );
        }
    }
    let tree = tree_from_urls(&config.urls);
    let tile_count = tree.tiles().len();
    if matches!(config.windowing, WindowingMode::Headless) {
        return Ok(WorkspaceViewerOutcome {
            first_url: config.urls.first().cloned().unwrap_or_default(),
            created_window: false,
            redraws: 0,
            size: (0, 0),
            tile_count,
            interaction_receipt: false,
            capability_receipt: false,
            workspace_receipt: None,
            routes: Vec::new(),
        });
    }

    let initial_size = config.size.unwrap_or((1100, 750));
    #[cfg(target_os = "windows")]
    let omit_scrying = matches!(
        config.workspace_receipt,
        Some(
            WorkspaceReceipt::Fallback
                | WorkspaceReceipt::LoadingError
                | WorkspaceReceipt::Appearance
                | WorkspaceReceipt::Accessibility
                | WorkspaceReceipt::NarrowChrome
                | WorkspaceReceipt::ChromeDpi
        )
    );
    #[cfg(target_os = "windows")]
    let scrying_host = (!omit_scrying).then(ScryingReceiptHost::new);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(scrying_host.clone());
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let overrides = config.route_overrides.clone();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                return Err("standalone Pelt only routes document tile sources".to_owned());
            };
            let mut request = PeltTileRequest::new(address, initial_size);
            if let Some(engine) = overrides.get(&tile.id.0) {
                request = request.with_engine_override(engine);
            }
            Ok(request)
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )?;
    let frisket = FrisketSurface::new(workspace.tree());
    let event_loop =
        EventLoop::new().map_err(|error| format!("could not create event loop: {error}"))?;
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, scrying_host);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("workspace event loop failed: {error}"))?;
    if let Some(error) = app.receipt_error.take() {
        return Err(error);
    }
    if app.config.capability_receipt && !app.receipt_complete {
        return Err("P4 capability receipt ended before a native surface frame arrived".to_owned());
    }
    if app.config.workspace_receipt.is_some() && app.workspace_receipt_outcome.is_none() {
        return Err("workspace receipt ended before its semantic assertion and capture".to_owned());
    }
    #[cfg(target_os = "windows")]
    if app.config.capability_receipt
        || matches!(
            app.config.workspace_receipt,
            Some(WorkspaceReceipt::Mixed | WorkspaceReceipt::Chrome)
        )
    {
        let native = app.native_surfaces.stats();
        println!(
            "pelt native-surface receipt frames={} imports={} waits={} compositions={}",
            native.frames, native.imports, native.waits, native.compositions
        );
    }
    Ok(app.outcome())
}

struct WorkspaceClock(Instant);

impl pelt_core::PeltClock for WorkspaceClock {
    fn now_ms(&self) -> f64 {
        self.0.elapsed().as_secs_f64() * 1000.0
    }
}

fn workspace_registries(
    #[cfg(target_os = "windows")] scrying_host: Option<ScryingReceiptHost>,
) -> PeltRegistries<Scene> {
    let mut sessions: SessionRegistry<Scene> = SessionRegistry::new();
    let fetcher = genet_documents::LocalFetcher::with_resource_policy(
        genet_documents::ResourceFetchPolicy::default(),
    );
    sessions.register(Box::new(genet_documents::LiverySessionEngine::new(
        fetcher.clone(),
    )));
    #[cfg(feature = "reader")]
    sessions.register(Box::new(genet_documents::ReaderSessionEngine::default()));
    #[cfg(feature = "scripted")]
    sessions.register(Box::new(genet_documents::ScriptedSessionEngine::<
        script_engine_boa::BoaEngine,
        _,
    >::new(
        inker::routing::ENGINE_GENET_SCRIPTED,
        fetcher.clone(),
    )));
    #[cfg(feature = "scripted-nova")]
    sessions.register(Box::new(genet_documents::ScriptedSessionEngine::<
        script_engine_nova::NovaEngine,
        _,
    >::new(
        inker::routing::ENGINE_GENET_SCRIPTED_NOVA,
        fetcher.clone(),
    )));
    #[cfg(feature = "smolweb")]
    for engine_id in [
        inker::routing::ENGINE_NEMATIC_GEMTEXT,
        inker::routing::ENGINE_NEMATIC_GOPHER,
        inker::routing::ENGINE_NEMATIC_FEED,
        inker::routing::ENGINE_NEMATIC_NEX,
        inker::routing::ENGINE_NEMATIC_FINGER,
    ] {
        sessions.register(Box::new(genet_documents::SmolwebSessionEngine::new(
            engine_id,
            fetcher.clone(),
            genet_documents::SmolwebTheme::System,
        )));
    }

    let mut policy = inker::routing::EngineRoutePolicy::default();
    for rule in &mut policy.rules {
        if rule.engine_id == inker::routing::ENGINE_GENET_WEB {
            rule.engine_id = inker::routing::ENGINE_GENET_LIVERY.to_owned();
        }
    }
    policy.fallback.engine_id = inker::routing::ENGINE_GENET_LIVERY.to_owned();
    let mut surfaces = SurfaceEngineRegistry::new();
    #[cfg(target_os = "windows")]
    if let Some(host) = scrying_host {
        surfaces.register(Box::new(ScryingReceiptEngine::new(host)));
    }
    PeltRegistries::new(
        sessions,
        surfaces,
        policy,
        "pelt.workspace",
        inker::routing::ENGINE_GENET_LIVERY,
        EngineProfileBinding {
            user_data_dir: "pelt-surface-profile".to_owned(),
        },
    )
}

fn tree_from_urls(urls: &[String]) -> TileTree {
    let urls = if urls.is_empty() {
        vec!["about:blank".to_owned()]
    } else {
        urls.to_vec()
    };
    let make_tile = |index: usize| Tile {
        id: TileId(index as u64 + 1),
        title: tile_title(&urls[index]),
        content: ContentSource::Document(DocumentRef(urls[index].clone())),
        accent: None,
    };
    match urls.len() {
        1 => TileTree::single(make_tile(0)),
        2 => TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::single(make_tile(0))),
                TileBranch::new(0.5, TileTree::single(make_tile(1))),
            ],
        ),
        3 => TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::stack(vec![make_tile(0), make_tile(1)], 0)),
                TileBranch::new(0.5, TileTree::single(make_tile(2))),
            ],
        ),
        _ => TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::stack(vec![make_tile(0), make_tile(1)], 0)),
                TileBranch::new(
                    0.5,
                    TileTree::split(
                        SplitAxis::Column,
                        vec![
                            TileBranch::new(0.5, TileTree::single(make_tile(2))),
                            TileBranch::new(
                                0.5,
                                TileTree::stack((3..urls.len()).map(&make_tile).collect(), 0),
                            ),
                        ],
                    ),
                ),
            ],
        ),
    }
}

fn tile_title(address: &str) -> String {
    address
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(address)
        .to_owned()
}

struct DividerDrag {
    target: cambium::DividerTarget,
    horizontal: bool,
    extent: f32,
    start: f32,
    init_first: f32,
    pair_total: f32,
}

struct TabDrag {
    tile: TileId,
    start: (f32, f32),
    moved: bool,
}

enum PointerGesture {
    Content,
    Divider(DividerDrag),
    Tab(TabDrag),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChromeStatus {
    Ready,
    Loading,
    Message(String),
    Error(String),
}

impl ChromeStatus {
    fn label(&self) -> String {
        match self {
            Self::Ready => "Ready".to_owned(),
            Self::Loading => "Loading".to_owned(),
            Self::Message(message) | Self::Error(message) => message.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct ChromeAddressInput {
    value: String,
    replace_on_insert: bool,
}

/// The menu is bound to the tile that opened it.  Rendering still flows
/// through the chrome snapshot, but route mutation never follows a later
/// focus change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChromeEngineMenu {
    tile: TileId,
}

/// The P6 appearance receipt snapshots the document-facing state before a
/// host-only palette change, so it can prove the theme did not replace or
/// rearrange the focused session.
#[derive(Clone, Debug, PartialEq)]
struct AppearanceReceiptBaseline {
    content: WorkspaceRect,
    address: String,
    can_go_back: bool,
}

fn capability_label(capability: inker::A11yCapability) -> &'static str {
    match capability {
        inker::A11yCapability::Full => "Full",
        inker::A11yCapability::Partial => "Partial",
        inker::A11yCapability::Opaque => "Opaque",
    }
}

fn count_label<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn inspector_section(label: &str, entries: Vec<String>) -> Option<ChromeInspectorSection> {
    (!entries.is_empty()).then(|| ChromeInspectorSection {
        label: format!("{label} ({})", entries.len()),
        omitted: entries.len().saturating_sub(INSPECTOR_VISIBLE_ROWS),
        entries: entries.into_iter().take(INSPECTOR_VISIBLE_ROWS).collect(),
    })
}

fn inspector_snapshot(
    active_engine: String,
    route_is_surface: bool,
    inspection: Option<PeltTileInspection>,
) -> ChromeInspector {
    match inspection {
        Some(PeltTileInspection {
            capability: inker::A11yCapability::Opaque,
            ..
        }) => ChromeInspector {
            status: if route_is_surface {
                "Opaque surface"
            } else {
                "Opaque document"
            }
            .to_owned(),
            title: Some(active_engine),
            summary: if route_is_surface {
                "Contents not inspectable on this surface."
            } else {
                "Contents not inspectable for this document."
            }
            .to_owned(),
            sections: Vec::new(),
        },
        Some(PeltTileInspection {
            capability,
            report: Some(report),
        }) => {
            let heading_count = report.headings.len();
            let outline_count = report.outline.len();
            let link_count = report.links.len();
            let mut sections = Vec::new();
            if let Some(section) = inspector_section("Headings", report.headings) {
                sections.push(section);
            }
            if let Some(section) = inspector_section(
                "Outline",
                report
                    .outline
                    .into_iter()
                    .map(|entry| {
                        let indent = "  ".repeat(entry.depth.min(3));
                        if entry.name.is_empty() {
                            format!("{indent}{}", entry.role)
                        } else {
                            format!("{indent}{}: {}", entry.role, entry.name)
                        }
                    })
                    .collect(),
            ) {
                sections.push(section);
            }
            if let Some(section) = inspector_section("Links", report.links) {
                sections.push(section);
            }
            if let Some(lineage) = report.lineage {
                let mut entry = format!(
                    "{} {} · {} · {} blocks",
                    lineage.tool, lineage.version, lineage.selector, lineage.block_count
                );
                if let Some(score) = lineage.score {
                    entry.push_str(&format!(" · score {score}"));
                }
                sections.push(ChromeInspectorSection {
                    label: "Lineage".to_owned(),
                    entries: vec![entry],
                    omitted: 0,
                });
            }
            ChromeInspector {
                status: format!("{} structural report", capability_label(capability)),
                title: report.title,
                summary: format!(
                    "{heading_count} {} · {outline_count} {} · {link_count} {}",
                    count_label(heading_count, "heading", "headings"),
                    count_label(outline_count, "outline entry", "outline entries"),
                    count_label(link_count, "link", "links"),
                ),
                sections,
            }
        },
        Some(PeltTileInspection { capability, .. }) if route_is_surface => ChromeInspector {
            status: format!("{} surface", capability_label(capability)),
            title: Some(active_engine),
            summary: "This surface does not expose a structural report to Pelt.".to_owned(),
            sections: Vec::new(),
        },
        Some(PeltTileInspection { capability, .. }) => ChromeInspector {
            status: format!("{} document", capability_label(capability)),
            title: Some(active_engine),
            summary: "This document does not expose structural content.".to_owned(),
            sections: Vec::new(),
        },
        None => ChromeInspector {
            status: "Unavailable".to_owned(),
            title: Some(active_engine),
            summary: "Content inspection is unavailable for this tile.".to_owned(),
            sections: Vec::new(),
        },
    }
}

struct WorkspaceApp {
    config: WorkspaceViewerConfig,
    workspace: PeltWorkspace<Scene>,
    frisket: FrisketSurface,
    window: Option<Arc<Window>>,
    host: Option<SurfaceHost>,
    width: u32,
    height: u32,
    scale_factor: f32,
    redraws: u32,
    modifiers: SessionModifiers,
    cursor: (f32, f32),
    gesture: Option<PointerGesture>,
    receipt_step: u8,
    receipt_complete: bool,
    workspace_receipt_redraws: u32,
    receipt_error: Option<String>,
    pending_workspace_assertion: Option<String>,
    workspace_receipt_outcome: Option<WorkspaceReceiptOutcome>,
    workspace_receipt_stage_started: Instant,
    chrome_status: ChromeStatus,
    last_chrome_document: Option<ChromeDocument>,
    chrome_address: Option<ChromeAddressInput>,
    chrome_engine_menu: Option<ChromeEngineMenu>,
    chrome_inspector_open: bool,
    chrome_theme: ChromeTheme,
    chrome_appearance_open: bool,
    appearance_receipt_baseline: Option<AppearanceReceiptBaseline>,
    accessibility: WorkspaceAccessibility,
    #[cfg(target_os = "windows")]
    native_surfaces: Dx12SurfaceCache,
    #[cfg(target_os = "windows")]
    mixed_content_ready_frame: Option<u32>,
    #[cfg(target_os = "windows")]
    mixed_verified_sizes: Vec<(u32, u32)>,
    #[cfg(target_os = "windows")]
    mixed_pending_resize: Option<MixedPendingResize>,
    #[cfg(target_os = "windows")]
    scrying_host: Option<ScryingReceiptHost>,
}

/// Per-window platform bridge and retained shell action map.
///
/// The map deliberately names only Frisket's one DOM. Child document and
/// native-surface trees need stable per-tile namespaces before they can join
/// this tree, so they remain labelled apertures in this P6 slice.
struct WorkspaceAccessibility {
    bridge: AccessKitBridge,
    window_revealed: bool,
    last_install_error: Option<String>,
    action_map: HashMap<AccessNodeId, genet_scripted_dom::NodeId>,
    focus: Option<FrisketA11yTarget>,
    wake: Arc<AtomicBool>,
}

impl WorkspaceAccessibility {
    fn new() -> Self {
        let wake = Arc::new(AtomicBool::new(false));
        let requested = Arc::clone(&wake);
        Self {
            bridge: AccessKitBridge::new(move || {
                requested.store(true, Ordering::Release);
            }),
            window_revealed: false,
            last_install_error: None,
            action_map: HashMap::new(),
            focus: None,
            wake,
        }
    }

    fn prepare(&mut self, projection: FrisketA11yProjection, scale_factor: f64) -> TreeUpdate {
        self.action_map = projection.nodes;
        let mut tree = projection.tree;
        if let Some((_, root)) = tree.nodes.iter_mut().find(|(id, _)| *id == projection.root) {
            // Livery lays the retained shell out in logical CSS pixels. The
            // platform tree needs physical client coordinates, matching the
            // raster and pointer conversion paths below.
            root.set_transform(Affine::scale(scale_factor));
        }
        tree
    }

    fn sync(
        &mut self,
        window: &Window,
        projection: FrisketA11yProjection,
        scale_factor: f64,
    ) -> Vec<A11yActionRequest> {
        let node_count = projection.tree.nodes.len();
        let tree = self.prepare(projection, scale_factor);
        if self.bridge.status() != BridgeStatus::Installed {
            match self.bridge.install(window, tree) {
                Ok(()) => {
                    self.last_install_error = None;
                    eprintln!(
                        "[pelt] accessibility {:?}, {node_count} retained shell nodes projected",
                        self.bridge.status()
                    );
                },
                Err(error) => {
                    if self.last_install_error.as_deref() != Some(error.as_str()) {
                        eprintln!("[pelt] accessibility install failed: {error}");
                    }
                    self.last_install_error = Some(error);
                },
            }
            // The Windows adapter has to attach before the first visible frame.
            // An initial failure still leaves ordinary Pelt usable and is tried
            // again on a later redraw instead of permanently disabling a11y.
            if !self.window_revealed {
                window.set_visible(true);
                self.window_revealed = true;
            }
            return self.bridge.drain_actions();
        }
        self.bridge.update(tree);
        self.bridge.drain_actions()
    }

    fn node_for(&self, id: AccessNodeId) -> Option<genet_scripted_dom::NodeId> {
        self.action_map.get(&id).copied()
    }

    fn set_focus(&mut self, target: FrisketA11yTarget) -> bool {
        if self.focus.as_ref() == Some(&target) {
            return false;
        }
        self.focus = Some(target);
        true
    }

    fn status(&self) -> BridgeStatus {
        self.bridge.status()
    }

    fn update_window_focus(&mut self, focused: bool) {
        self.bridge.update_window_focus(focused);
    }

    fn take_wake(&self) -> bool {
        self.wake.swap(false, Ordering::AcqRel)
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
struct MixedPendingResize {
    target: (u32, u32),
    frames: u32,
    imports: u32,
    waits: u32,
    compositions: u32,
}

impl WorkspaceApp {
    fn new(
        config: WorkspaceViewerConfig,
        workspace: PeltWorkspace<Scene>,
        frisket: FrisketSurface,
        #[cfg(target_os = "windows")] scrying_host: Option<ScryingReceiptHost>,
    ) -> Self {
        let (width, height) = config.size.unwrap_or((1100, 750));
        Self {
            config,
            workspace,
            frisket,
            window: None,
            host: None,
            width,
            height,
            scale_factor: 1.0,
            redraws: 0,
            modifiers: SessionModifiers::default(),
            cursor: (0.0, 0.0),
            gesture: None,
            receipt_step: 0,
            receipt_complete: false,
            workspace_receipt_redraws: 0,
            receipt_error: None,
            pending_workspace_assertion: None,
            workspace_receipt_outcome: None,
            workspace_receipt_stage_started: Instant::now(),
            chrome_status: ChromeStatus::Ready,
            last_chrome_document: None,
            chrome_address: None,
            chrome_engine_menu: None,
            chrome_inspector_open: false,
            chrome_theme: ChromeTheme::Dark,
            chrome_appearance_open: false,
            appearance_receipt_baseline: None,
            accessibility: WorkspaceAccessibility::new(),
            #[cfg(target_os = "windows")]
            native_surfaces: Dx12SurfaceCache::new(),
            #[cfg(target_os = "windows")]
            mixed_content_ready_frame: None,
            #[cfg(target_os = "windows")]
            mixed_verified_sizes: Vec::new(),
            #[cfg(target_os = "windows")]
            mixed_pending_resize: None,
            #[cfg(target_os = "windows")]
            scrying_host,
        }
    }

    fn outcome(&self) -> WorkspaceViewerOutcome {
        let mut routes = self
            .workspace
            .routes()
            .map(|route| {
                let state = match &route.state {
                    PeltRouteState::Document => "document".to_owned(),
                    PeltRouteState::Surface => {
                        format!("surface:{:?}", route.decision.surface_contract.mode)
                    },
                    PeltRouteState::Fallback {
                        active_engine,
                        reason,
                    } => {
                        format!("fallback:{active_engine}:{reason}")
                    },
                };
                format!("{}={}:{state}", route.tile.0, route.selected_engine())
            })
            .collect::<Vec<_>>();
        routes.sort();
        WorkspaceViewerOutcome {
            first_url: self.config.urls.first().cloned().unwrap_or_default(),
            created_window: self.window.is_some(),
            redraws: self.redraws,
            size: if self.window.is_some() {
                (self.width, self.height)
            } else {
                (0, 0)
            },
            tile_count: self.workspace.tree().tiles().len(),
            interaction_receipt: self.config.interaction_receipt && self.receipt_complete,
            capability_receipt: self.config.capability_receipt && self.receipt_complete,
            workspace_receipt: self.workspace_receipt_outcome.clone(),
            routes,
        }
    }

    fn window_title(&self) -> String {
        let controller = self
            .workspace
            .focused_tile()
            .and_then(|tile| self.workspace.controller(tile));
        static_viewer::pelt_window_title(
            controller.and_then(PeltController::title).as_deref(),
            controller.map(PeltController::address),
        )
    }

    fn refresh_chrome(&mut self) {
        self.dismiss_chrome_engine_menu_if_focus_changed();
        let chrome = self.config.chrome.then(|| self.chrome_model());
        self.frisket.set_chrome(chrome);
    }

    fn refresh_accessibility_content_regions(&mut self) {
        let regions = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .map(|tile| {
                let description = match self
                    .workspace
                    .controller(tile.id)
                    .map(PeltController::a11y_capability)
                {
                    Some(A11yCapability::Opaque) => {
                        "The engine declares opaque accessibility. Pelt cannot inspect or compose this content's semantics."
                    },
                    Some(A11yCapability::Partial) => {
                        "The engine declares partial accessibility. Pelt does not compose its document semantics into this workspace tree yet."
                    },
                    Some(A11yCapability::Full) => {
                        "The engine declares a full semantic tree. Pelt does not compose that child tree into this workspace tree yet."
                    },
                    None => {
                        "Pelt has not received an accessibility declaration for this content."
                    },
                };
                FrisketContentA11y {
                    tile: tile.id,
                    label: format!("{} content", tile.title),
                    description: description.to_owned(),
                }
            })
            .collect::<Vec<_>>();
        self.frisket.set_content_accessibility(regions);
    }

    /// Build the currently retained shell tree without a window. This makes the
    /// semantic projection and the same action map independently testable.
    fn prepare_accessibility_tree(&mut self) -> Result<TreeUpdate, String> {
        self.refresh_chrome();
        self.refresh_accessibility_content_regions();
        let (width, height) = self.logical_size();
        self.frisket
            .frame(width, height)
            .map_err(|error| format!("could not lay out Frisket for accessibility: {error}"))?;
        let projection = self
            .frisket
            .accessibility_projection(self.accessibility.focus.as_ref())
            .ok_or_else(|| {
                "Frisket has no completed retained layout for accessibility".to_owned()
            })?;
        Ok(self
            .accessibility
            .prepare(projection, self.scale_factor as f64))
    }

    fn install_accessibility_before_show(&mut self) -> Result<(), String> {
        let _ = self.prepare_accessibility_tree()?;
        let Some(window) = self.window.clone() else {
            return Err("Pelt accessibility install needs its live window".to_owned());
        };
        let projection = self
            .frisket
            .accessibility_projection(self.accessibility.focus.as_ref())
            .ok_or_else(|| {
                "Frisket lost its retained layout before accessibility install".to_owned()
            })?;
        let _ = self
            .accessibility
            .sync(&window, projection, self.scale_factor as f64);
        if self.config.workspace_receipt == Some(WorkspaceReceipt::Accessibility)
            && self.accessibility.status() != BridgeStatus::Installed
        {
            return Err(
                "Pelt accessibility receipt could not install the platform AccessKit bridge"
                    .to_owned(),
            );
        }
        Ok(())
    }

    fn sync_accessibility(&mut self) -> bool {
        let Some(window) = self.window.clone() else {
            return false;
        };
        self.refresh_accessibility_content_regions();
        let Some(projection) = self
            .frisket
            .accessibility_projection(self.accessibility.focus.as_ref())
        else {
            return false;
        };
        self.accessibility
            .sync(&window, projection, self.scale_factor as f64)
            .into_iter()
            .fold(false, |redraw, request| {
                self.apply_accessibility_request(request) || redraw
            })
    }

    fn apply_accessibility_request(&mut self, request: A11yActionRequest) -> bool {
        let Some(node) = self.accessibility.node_for(request.target_node) else {
            return false;
        };
        match request.action {
            Action::Focus => self
                .frisket
                .accessibility_target(node)
                .is_some_and(|target| self.accessibility.set_focus(target)),
            Action::Click => match self.frisket.accessibility_target(node) {
                Some(FrisketA11yTarget::ChromeAction(action)) => self.apply_chrome_action(action),
                Some(FrisketA11yTarget::Close(tile)) => {
                    self.clear_chrome_address();
                    self.clear_chrome_engine_menu();
                    self.clear_chrome_appearance();
                    self.apply_tile_event(TileEvent::Closed(tile))
                },
                Some(FrisketA11yTarget::Tab(tile)) => {
                    self.clear_chrome_address();
                    self.clear_chrome_engine_menu();
                    self.clear_chrome_appearance();
                    self.apply_tile_event(TileEvent::Activated(tile))
                },
                None => false,
            },
            _ => false,
        }
    }

    fn chrome_engine_choices() -> Vec<ChromeEngineChoice> {
        #[cfg(feature = "scripted")]
        {
            vec![
                ChromeEngineChoice::Automatic,
                ChromeEngineChoice::Livery,
                ChromeEngineChoice::Scripted,
            ]
        }
        #[cfg(not(feature = "scripted"))]
        {
            vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery]
        }
    }

    fn selected_chrome_engine(&self, tile: TileId) -> Option<ChromeEngineChoice> {
        let route = self.workspace.route(tile)?;
        if route.source == PeltRouteSource::Automatic {
            return Some(ChromeEngineChoice::Automatic);
        }
        match route.selected_engine() {
            inker::routing::ENGINE_GENET_LIVERY => Some(ChromeEngineChoice::Livery),
            inker::routing::ENGINE_GENET_SCRIPTED => Some(ChromeEngineChoice::Scripted),
            _ => None,
        }
    }

    fn dismiss_chrome_engine_menu_if_focus_changed(&mut self) {
        if self
            .chrome_engine_menu
            .is_some_and(|menu| self.workspace.focused_tile() != Some(menu.tile))
        {
            self.chrome_engine_menu = None;
        }
    }

    fn clear_chrome_engine_menu(&mut self) {
        self.chrome_engine_menu = None;
    }

    fn clear_chrome_appearance(&mut self) {
        self.chrome_appearance_open = false;
    }

    fn chrome_inspector(&self, tile: TileId) -> ChromeInspector {
        let route = self.workspace.route(tile);
        let active_engine = route
            .map(|route| route.active_engine().to_owned())
            .unwrap_or_else(|| "No active engine".to_owned());
        let route_is_surface =
            route.is_some_and(|route| matches!(route.state, PeltRouteState::Surface));
        inspector_snapshot(
            active_engine,
            route_is_surface,
            self.workspace.inspection(tile),
        )
    }

    fn chrome_model(&self) -> WorkspaceChrome {
        let Some(tile) = self.workspace.focused_tile() else {
            return WorkspaceChrome {
                title: "No focused tile".to_owned(),
                address: String::new(),
                route: "No route".to_owned(),
                status: self.chrome_status.label(),
                theme: self.chrome_theme,
                address_focused: self.chrome_address.is_some(),
                can_go_back: false,
                can_go_forward: false,
                engine_label: "Auto".to_owned(),
                engine_menu_open: false,
                engine_selected: None,
                engine_choices: Self::chrome_engine_choices(),
                inspector: None,
                appearance: self.chrome_appearance_open.then_some(ChromeAppearance {
                    theme: self.chrome_theme,
                }),
                diagnostic: None,
            };
        };
        let controller = self.workspace.controller(tile);
        let title = controller
            .and_then(PeltController::title)
            .or_else(|| {
                self.workspace
                    .tree()
                    .tiles()
                    .into_iter()
                    .find(|candidate| candidate.id == tile)
                    .map(|candidate| candidate.title.clone())
            })
            .unwrap_or_else(|| format!("Tile {}", tile.0));
        let title = title
            .split_once(" [")
            .map_or(title.as_str(), |(base, _)| base)
            .trim()
            .to_owned();
        let address = self
            .chrome_address
            .as_ref()
            .map(|input| input.value.clone())
            .unwrap_or_else(|| self.focused_address());
        let route = self
            .workspace
            .route(tile)
            .map(|route| {
                let source = match route.source {
                    PeltRouteSource::Automatic => "Automatic",
                    PeltRouteSource::UserOverride => "Pinned",
                };
                match &route.state {
                    PeltRouteState::Document => {
                        format!("{source}: {} · document", route.selected_engine())
                    },
                    PeltRouteState::Surface => {
                        format!("{source}: {} · surface", route.selected_engine())
                    },
                    PeltRouteState::Fallback {
                        active_engine,
                        reason,
                    } => format!(
                        "{source}: {} → {active_engine} · {reason}",
                        route.selected_engine()
                    ),
                }
            })
            .unwrap_or_else(|| "No route".to_owned());
        let engine_selected = self.selected_chrome_engine(tile);
        let engine_label = engine_selected
            .map(ChromeEngineChoice::trigger_label)
            .map(str::to_owned)
            .or_else(|| {
                self.workspace
                    .route(tile)
                    .map(|route| route.selected_engine().to_owned())
            })
            .unwrap_or_else(|| "Auto".to_owned());
        let diagnostic = self.workspace.content_rect(tile).and_then(|rect| {
            let controller = controller?;
            match controller.document_state() {
                PeltDocumentState::Ready => None,
                PeltDocumentState::Loading { address } => Some(ChromeDocument {
                    kind: ChromeDocumentKind::Loading,
                    tile,
                    rect,
                    address: address.clone(),
                    message: None,
                }),
                PeltDocumentState::Error { address, message } => Some(ChromeDocument {
                    kind: ChromeDocumentKind::Error,
                    tile,
                    rect,
                    address: address.clone(),
                    message: Some(message.clone()),
                }),
            }
        });
        let status = match controller.map(PeltController::document_state) {
            Some(PeltDocumentState::Loading { .. }) => "Loading".to_owned(),
            Some(PeltDocumentState::Error { message, .. }) => message.clone(),
            Some(PeltDocumentState::Ready) | None => self.chrome_status.label(),
        };
        WorkspaceChrome {
            title,
            address,
            route,
            status,
            theme: self.chrome_theme,
            address_focused: self.chrome_address.is_some(),
            can_go_back: controller.is_some_and(PeltController::can_go_back),
            can_go_forward: controller.is_some_and(PeltController::can_go_forward),
            engine_label,
            engine_menu_open: self
                .chrome_engine_menu
                .is_some_and(|menu| menu.tile == tile),
            engine_selected,
            engine_choices: Self::chrome_engine_choices(),
            inspector: self
                .chrome_inspector_open
                .then(|| self.chrome_inspector(tile)),
            appearance: self.chrome_appearance_open.then_some(ChromeAppearance {
                theme: self.chrome_theme,
            }),
            diagnostic,
        }
    }

    fn focused_address(&self) -> String {
        let Some(tile) = self.workspace.focused_tile() else {
            return String::new();
        };
        self.workspace
            .controller(tile)
            .map(PeltController::address)
            .map(str::to_owned)
            .or_else(|| {
                self.workspace
                    .tree()
                    .tiles()
                    .into_iter()
                    .find(|candidate| candidate.id == tile)
                    .and_then(|candidate| match &candidate.content {
                        ContentSource::Document(DocumentRef(address)) => Some(address.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_default()
    }

    fn begin_address_edit(&mut self) {
        self.chrome_address = Some(ChromeAddressInput {
            value: self.focused_address(),
            replace_on_insert: true,
        });
        self.chrome_status = ChromeStatus::Ready;
        self.set_chrome_ime_allowed(true);
    }

    fn clear_chrome_address(&mut self) {
        self.chrome_address = None;
        self.set_chrome_ime_allowed(false);
    }

    fn set_chrome_ime_allowed(&self, allowed: bool) {
        if let Some(window) = &self.window {
            window.set_ime_allowed(allowed);
        }
    }

    fn append_chrome_address(&mut self, text: &str) {
        if self.chrome_address.is_none() {
            self.begin_address_edit();
        }
        let input = self
            .chrome_address
            .as_mut()
            .expect("address edit was installed above");
        if input.replace_on_insert {
            input.value.clear();
            input.replace_on_insert = false;
        }
        input.value.push_str(text);
    }

    fn backspace_chrome_address(&mut self) {
        let Some(input) = self.chrome_address.as_mut() else {
            return;
        };
        if input.replace_on_insert {
            input.value.clear();
            input.replace_on_insert = false;
        } else {
            input.value.pop();
        }
    }

    fn submit_chrome_address(&mut self) -> bool {
        let Some(input) = self.chrome_address.take() else {
            return false;
        };
        self.set_chrome_ime_allowed(false);
        let address = input.value.trim();
        if address.is_empty() {
            self.chrome_status = ChromeStatus::Error("Address is empty".to_owned());
            return true;
        }
        let effect = self
            .workspace
            .command(SessionNavigationCommand::Address(address.to_owned()));
        self.apply_effect(effect);
        true
    }

    fn toggle_chrome_engine_menu(&mut self) -> bool {
        let Some(tile) = self.workspace.focused_tile() else {
            return false;
        };
        if self.chrome_engine_menu == Some(ChromeEngineMenu { tile }) {
            self.clear_chrome_engine_menu();
        } else {
            self.chrome_engine_menu = Some(ChromeEngineMenu { tile });
        }
        true
    }

    fn choose_chrome_engine(&mut self, choice: ChromeEngineChoice) -> bool {
        let Some(menu) = self.chrome_engine_menu.take() else {
            return false;
        };
        if self.workspace.focused_tile() != Some(menu.tile) {
            self.chrome_status = ChromeStatus::Message(
                "Engine chooser closed because the focused tile changed".to_owned(),
            );
            return true;
        }
        let next = match choice {
            ChromeEngineChoice::Automatic => None,
            ChromeEngineChoice::Livery => Some(inker::routing::ENGINE_GENET_LIVERY.to_owned()),
            ChromeEngineChoice::Scripted => {
                #[cfg(feature = "scripted")]
                {
                    Some(inker::routing::ENGINE_GENET_SCRIPTED.to_owned())
                }
                #[cfg(not(feature = "scripted"))]
                {
                    self.chrome_status = ChromeStatus::Error(
                        "Scripted engine is unavailable in this Pelt build".to_owned(),
                    );
                    return true;
                }
            },
        };
        match self.workspace.set_route_override(menu.tile, next.clone()) {
            Ok(changed) => {
                if !changed {
                    self.chrome_status = ChromeStatus::Message(format!(
                        "Engine already selected: {}",
                        choice.label()
                    ));
                    return true;
                }
                self.frisket.set_tree(self.workspace.tree());
                self.chrome_status = ChromeStatus::Message(match next {
                    Some(engine) => format!("Engine pinned: {engine}"),
                    None => "Automatic route restored".to_owned(),
                });
                if let Some(window) = &self.window {
                    window.set_title(&self.window_title());
                }
                true
            },
            Err(error) => {
                self.chrome_status = ChromeStatus::Error(error);
                true
            },
        }
    }

    fn toggle_chrome_inspector(&mut self) -> bool {
        if self.workspace.focused_tile().is_none() {
            return false;
        }
        self.chrome_inspector_open = !self.chrome_inspector_open;
        if self.chrome_inspector_open {
            self.clear_chrome_appearance();
        }
        true
    }

    fn toggle_chrome_appearance(&mut self) -> bool {
        if self.workspace.focused_tile().is_none() {
            return false;
        }
        self.chrome_appearance_open = !self.chrome_appearance_open;
        if self.chrome_appearance_open {
            self.chrome_inspector_open = false;
        }
        true
    }

    fn choose_chrome_theme(&mut self, theme: ChromeTheme) -> bool {
        if !self.chrome_appearance_open {
            return false;
        }
        self.chrome_theme = theme;
        self.chrome_status = ChromeStatus::Message(format!("Chrome theme: {}", theme.label()));
        true
    }

    fn apply_chrome_action(&mut self, action: ChromeAction) -> bool {
        if action != ChromeAction::Address {
            self.clear_chrome_address();
        }
        if !matches!(
            action,
            ChromeAction::ToggleEngineMenu | ChromeAction::ChooseEngine(_)
        ) {
            self.clear_chrome_engine_menu();
        }
        if !matches!(
            action,
            ChromeAction::ToggleAppearance | ChromeAction::ChooseTheme(_)
        ) {
            self.clear_chrome_appearance();
        }
        match action {
            ChromeAction::Back => {
                let effect = self.workspace.command(SessionNavigationCommand::Back);
                let redraw = effect.redraw || effect.navigated;
                self.apply_effect(effect);
                redraw
            },
            ChromeAction::Forward => {
                let effect = self.workspace.command(SessionNavigationCommand::Forward);
                let redraw = effect.redraw || effect.navigated;
                self.apply_effect(effect);
                redraw
            },
            ChromeAction::Reload => {
                let effect = self.workspace.command(SessionNavigationCommand::Reload);
                let redraw = effect.redraw || effect.navigated;
                self.apply_effect(effect);
                redraw
            },
            ChromeAction::Address => {
                self.begin_address_edit();
                true
            },
            ChromeAction::ToggleEngineMenu => self.toggle_chrome_engine_menu(),
            ChromeAction::ChooseEngine(choice) => self.choose_chrome_engine(choice),
            ChromeAction::ToggleInspector => self.toggle_chrome_inspector(),
            ChromeAction::ToggleAppearance => self.toggle_chrome_appearance(),
            ChromeAction::ChooseTheme(theme) => self.choose_chrome_theme(theme),
        }
    }

    fn handle_chrome_key(&mut self, key: &Key, state: ElementState) -> bool {
        if self.chrome_address.is_none() || state != ElementState::Pressed {
            return false;
        }
        match key {
            Key::Named(NamedKey::Enter) => self.submit_chrome_address(),
            Key::Named(NamedKey::Escape) => {
                self.clear_chrome_address();
                self.chrome_status = ChromeStatus::Ready;
                true
            },
            Key::Named(NamedKey::Backspace) => {
                self.backspace_chrome_address();
                true
            },
            Key::Character(text)
                if !self.modifiers.control && !self.modifiers.alt && !self.modifiers.meta =>
            {
                self.append_chrome_address(text);
                true
            },
            _ => true,
        }
    }

    fn handle_chrome_ime(&mut self, ime: &winit::event::Ime) -> bool {
        if self.chrome_address.is_none() {
            return false;
        }
        if let winit::event::Ime::Commit(text) = ime {
            self.append_chrome_address(text);
        }
        true
    }

    fn logical_size(&self) -> (u32, u32) {
        (
            static_viewer::logical_extent(self.width, self.scale_factor),
            static_viewer::logical_extent(self.height, self.scale_factor),
        )
    }

    /// Route a physical winit coordinate through the same DPI conversion the
    /// live window uses before giving it to retained Chrome or Frisket.
    fn pointer_move_physical(&mut self, x: f32, y: f32) -> bool {
        self.pointer_move(
            static_viewer::logical_position(x, self.scale_factor),
            static_viewer::logical_position(y, self.scale_factor),
        )
    }

    fn render(&mut self, event_loop: &ActiveEventLoop) {
        if self.config.workspace_receipt.is_some() && self.redraws > 0 && !self.receipt_complete {
            match self.drive_workspace_receipt() {
                Ok(Some(assertion)) => {
                    self.pending_workspace_assertion = Some(assertion);
                    self.receipt_complete = true;
                },
                Ok(None) => {},
                Err(error) => {
                    self.receipt_error = Some(error);
                    event_loop.exit();
                    return;
                },
            }
        }
        if self.config.capability_receipt
            && self.redraws > 0
            && !self.receipt_complete
            && self.capability_receipt_ready()
        {
            if let Err(error) = self.validate_capability_receipt() {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            }
            self.receipt_complete = true;
        }
        if self.config.interaction_receipt && self.redraws > 0 && !self.receipt_complete {
            if let Err(error) = self.drive_receipt_step() {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            }
        }

        self.refresh_chrome();
        let (logical_width, logical_height) = self.logical_size();
        let mut pane_frame = match self.frisket.frame(logical_width, logical_height) {
            Ok(frame) => frame,
            Err(error) => {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            },
        };
        self.workspace
            .set_content_rects(pane_frame.content_rects.iter().copied());
        // A diagnostic document is positioned from the actual Frisket content
        // hole. The first layout obtains that geometry; the second carries the
        // host-owned overlay without changing Frisket's own layout.
        if self.config.chrome && self.chrome_model().diagnostic.is_some() {
            self.refresh_chrome();
            pane_frame = match self.frisket.frame(logical_width, logical_height) {
                Ok(frame) => frame,
                Err(error) => {
                    self.receipt_error = Some(error);
                    event_loop.exit();
                    return;
                },
            };
            self.workspace
                .set_content_rects(pane_frame.content_rects.iter().copied());
        }
        self.last_chrome_document = (self.config.chrome && pane_frame.diagnostic_rect.is_some())
            .then(|| self.chrome_model().diagnostic)
            .flatten();
        if self.sync_accessibility() {
            self.request_redraw();
        }
        self.workspace.set_surface_scale_factor(self.scale_factor);
        let more = self.workspace.pump();
        // Once the Chrome receipt has asserted its final state, keep composing
        // its already-imported native layer through capture. That gives this
        // native inspector receipt a stable visual boundary without advancing
        // an external producer after the evidence is complete.
        let capture_stable_workspace = self.config.workspace_receipt
            == Some(WorkspaceReceipt::Chrome)
            && self.receipt_complete;
        let workspace_frame = if capture_stable_workspace {
            self.workspace.frame_with_cached_surfaces()
        } else {
            self.workspace.frame()
        };
        #[cfg(target_os = "windows")]
        {
            let live_surfaces = self
                .workspace
                .routes()
                .filter(|route| matches!(route.state, PeltRouteState::Surface))
                .map(|route| route.tile)
                .collect::<Vec<_>>();
            self.native_surfaces
                .retain_tiles(|tile| live_surfaces.contains(&tile));
        }
        #[cfg(target_os = "windows")]
        let mut native_layers = Vec::new();
        for surface in workspace_frame.surfaces {
            match surface.frame {
                Ok(None) => {},
                Ok(Some(frame)) => {
                    #[cfg(target_os = "windows")]
                    {
                        let Some(host) = self.host.as_ref() else {
                            discard_unimported_surface_frame(frame);
                            return;
                        };
                        if let Err(error) =
                            self.native_surfaces
                                .accept_frame(surface.tile, frame, host.device())
                        {
                            self.receipt_error = Some(format!(
                                "tile {} native surface import failed: {error}",
                                surface.tile.0
                            ));
                            event_loop.exit();
                            return;
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        discard_unimported_surface_frame(frame);
                        self.receipt_error = Some(format!(
                            "tile {} produced a native surface frame, but this platform has no shared-handle importer",
                            surface.tile.0
                        ));
                        event_loop.exit();
                        return;
                    }
                },
                Err(error) => {
                    self.receipt_error =
                        Some(format!("tile {} surface failed: {error}", surface.tile.0));
                    event_loop.exit();
                    return;
                },
            }
            #[cfg(target_os = "windows")]
            if self.native_surfaces.view(surface.tile).is_some() {
                native_layers.push((surface.tile, surface.rect));
            }
        }
        let Some(host) = self.host.as_ref() else {
            return;
        };
        let (_frame_texture, frame_view) = host.rasterize_scaled(
            &pane_frame.scene,
            self.width,
            self.height,
            ColorLoad::Clear(wgpu::Color {
                r: 0.10,
                g: 0.10,
                b: 0.12,
                a: 1.0,
            }),
            self.scale_factor,
        );
        let tile_layers = workspace_frame
            .tiles
            .into_iter()
            .map(|layer| {
                let (width, height) = (
                    physical_extent(layer.rect.width, self.scale_factor),
                    physical_extent(layer.rect.height, self.scale_factor),
                );
                let (texture, view) = host.rasterize_scaled(
                    &layer.frame,
                    width,
                    height,
                    ColorLoad::Clear(wgpu::Color::WHITE),
                    self.scale_factor,
                );
                (texture, view, layer.rect)
            })
            .collect::<Vec<_>>();
        let inspector_overlay = pane_frame
            .inspector_rect
            .map(|rect| fragment_placement(rect, (self.width, self.height), self.scale_factor));
        let diagnostic_overlay = pane_frame
            .diagnostic_rect
            .map(|rect| fragment_placement(rect, (self.width, self.height), self.scale_factor));
        let appearance_overlay = pane_frame
            .appearance_rect
            .map(|rect| fragment_placement(rect, (self.width, self.height), self.scale_factor));
        let capture_now = self.config.workspace_receipt.is_some()
            && self.receipt_complete
            && self.workspace_receipt_outcome.is_none()
            && (matches!(
                self.config.workspace_receipt,
                Some(
                    WorkspaceReceipt::Chrome
                        | WorkspaceReceipt::LoadingError
                        | WorkspaceReceipt::Appearance
                        | WorkspaceReceipt::Accessibility
                        | WorkspaceReceipt::NarrowChrome
                        | WorkspaceReceipt::ChromeDpi
                )
            ) || self
                .config
                .frames
                .is_some_and(|limit| self.workspace_receipt_redraws.saturating_add(1) >= limit));
        let receipt_canvas = capture_now.then(|| {
            host.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("pelt workspace receipt composition"),
                size: wgpu::Extent3d {
                    width: self.width.max(1),
                    height: self.height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: host.format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        });
        let receipt_view = receipt_canvas
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let Some(swap) = host.acquire() else {
            if let Some(error) = self.workspace_receipt_timeout_error() {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            }
            // An outdated or lost swapchain is deliberately skipped by the
            // shared host. A named receipt must drive one recovery frame,
            // otherwise a geometry change can leave its native surface
            // waiting without another redraw.
            if self.config.workspace_receipt.is_some() && !self.receipt_complete {
                self.request_redraw();
            }
            return;
        };
        let swap_target = swap
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let target = receipt_view.as_ref().unwrap_or(&swap_target);
        host.renderer().compose_external_texture(
            &frame_view,
            &target,
            host.format(),
            self.width,
            self.height,
            ExternalTexturePlacement::new([0.0, 0.0, self.width as f32, self.height as f32]),
        );
        for (_texture, view, rect) in &tile_layers {
            host.renderer().compose_external_texture(
                view,
                &target,
                host.format(),
                self.width,
                self.height,
                placement(*rect, self.scale_factor),
            );
        }
        #[cfg(target_os = "windows")]
        for (tile, rect) in native_layers {
            if let Err(error) = self.native_surfaces.stage_wait(tile, host.queue()) {
                self.receipt_error = Some(format!(
                    "tile {} native surface synchronization failed: {error}",
                    tile.0
                ));
                event_loop.exit();
                return;
            }
            let view = self
                .native_surfaces
                .view(tile)
                .expect("native layer was retained above");
            host.renderer().compose_external_texture(
                view,
                &target,
                host.format(),
                self.width,
                self.height,
                placement(rect, self.scale_factor),
            );
            // The normal loop must return this shared resource before the
            // producer's next acquire. A frozen receipt takes no later acquire
            // and exits after capture, so avoid a needless whole-device wait.
            if !capture_stable_workspace {
                if let Err(error) =
                    self.native_surfaces
                        .return_to_common(tile, host.device(), host.queue())
                {
                    self.receipt_error = Some(format!(
                        "tile {} native surface release failed: {error}",
                        tile.0
                    ));
                    event_loop.exit();
                    return;
                }
            }
            self.native_surfaces.mark_composed();
        }
        if let Some(inspector_overlay) = inspector_overlay {
            host.renderer().compose_external_texture(
                &frame_view,
                &target,
                host.format(),
                self.width,
                self.height,
                inspector_overlay,
            );
        }
        if let Some(diagnostic_overlay) = diagnostic_overlay {
            host.renderer().compose_external_texture(
                &frame_view,
                &target,
                host.format(),
                self.width,
                self.height,
                diagnostic_overlay,
            );
        }
        if let Some(appearance_overlay) = appearance_overlay {
            host.renderer().compose_external_texture(
                &frame_view,
                &target,
                host.format(),
                self.width,
                self.height,
                appearance_overlay,
            );
        }
        let captured = if let Some(source) = receipt_view.as_ref() {
            let Some(path) = self.config.artifact.as_deref() else {
                self.receipt_error = Some("workspace receipt needs an artifact path".to_owned());
                event_loop.exit();
                return;
            };
            match crate::receipt_capture::capture_composition(
                host,
                source,
                self.width,
                self.height,
                path,
            ) {
                Ok(captured) => Some(captured),
                Err(error) => {
                    self.receipt_error = Some(error);
                    event_loop.exit();
                    return;
                },
            }
        } else {
            None
        };
        if let Some(captured) = captured.as_ref() {
            let receipt = self
                .config
                .workspace_receipt
                .expect("capture is only allocated for a workspace receipt");
            let assertion = self
                .pending_workspace_assertion
                .clone()
                .expect("semantic assertion precedes workspace capture");
            self.workspace_receipt_outcome = Some(WorkspaceReceiptOutcome {
                id: receipt.id(),
                assertion,
                artifact: captured.path.clone(),
                digest: captured.digest,
                verified_sizes: {
                    #[cfg(target_os = "windows")]
                    {
                        if self.mixed_verified_sizes.is_empty() {
                            vec![(self.width, self.height)]
                        } else {
                            self.mixed_verified_sizes.clone()
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        vec![(self.width, self.height)]
                    }
                },
            });
            host.renderer().compose_external_texture(
                &captured.view,
                &swap_target,
                host.format(),
                self.width,
                self.height,
                ExternalTexturePlacement::new([0.0, 0.0, self.width as f32, self.height as f32]),
            );
        }
        host.queue().present(swap);
        self.workspace.mark_visible_documents_presented();
        self.redraws += 1;
        if self.config.workspace_receipt.is_some() && self.receipt_complete {
            self.workspace_receipt_redraws += 1;
        }
        let chrome_loading_settled =
            self.config.chrome && self.chrome_status == ChromeStatus::Loading;
        if chrome_loading_settled {
            self.chrome_status = ChromeStatus::Ready;
        }

        if let Some(error) = self.workspace_receipt_timeout_error() {
            self.receipt_error = Some(error);
            event_loop.exit();
            return;
        }

        let workspace_receipt_finished = match self.config.workspace_receipt {
            Some(
                WorkspaceReceipt::Chrome
                | WorkspaceReceipt::LoadingError
                | WorkspaceReceipt::Appearance
                | WorkspaceReceipt::Accessibility
                | WorkspaceReceipt::NarrowChrome
                | WorkspaceReceipt::ChromeDpi,
            ) => self.workspace_receipt_outcome.is_some(),
            Some(_) => {
                self.receipt_complete
                    && self
                        .config
                        .frames
                        .is_some_and(|limit| self.workspace_receipt_redraws >= limit)
            },
            None => false,
        };
        if workspace_receipt_finished
            || (self.receipt_complete
                && !self.config.capability_receipt
                && self.config.workspace_receipt.is_none())
            || self.config.frames.is_some_and(|limit| {
                self.config.workspace_receipt.is_none() && self.redraws >= limit
            })
        {
            event_loop.exit();
        } else if self.config.interaction_receipt
            || more
            || self.config.frames.is_some()
            || chrome_loading_settled
        {
            self.request_redraw();
        }
    }

    fn drive_workspace_receipt(&mut self) -> Result<Option<String>, String> {
        match self
            .config
            .workspace_receipt
            .expect("receipt checked above")
        {
            WorkspaceReceipt::Mixed => self.drive_mixed_workspace_receipt_step(),
            WorkspaceReceipt::Fallback => self.drive_fallback_workspace_receipt_step(),
            WorkspaceReceipt::Chrome => self.drive_chrome_workspace_receipt_step(),
            WorkspaceReceipt::LoadingError => self.drive_loading_error_workspace_receipt_step(),
            WorkspaceReceipt::Appearance => self.drive_appearance_workspace_receipt_step(),
            WorkspaceReceipt::Accessibility => self.drive_accessibility_workspace_receipt_step(),
            WorkspaceReceipt::NarrowChrome => self.drive_narrow_chrome_workspace_receipt_step(),
            WorkspaceReceipt::ChromeDpi => self.drive_chrome_dpi_workspace_receipt_step(),
        }
    }

    fn workspace_receipt_timeout_error(&self) -> Option<String> {
        if matches!(
            self.config.workspace_receipt,
            Some(
                WorkspaceReceipt::LoadingError
                    | WorkspaceReceipt::Appearance
                    | WorkspaceReceipt::Accessibility
                    | WorkspaceReceipt::NarrowChrome
                    | WorkspaceReceipt::ChromeDpi
            )
        ) && !self.receipt_complete
            && self.workspace_receipt_stage_started.elapsed()
                >= self.config.workspace_receipt_stage_timeout
        {
            let receipt = self
                .config
                .workspace_receipt
                .expect("the matching receipt was present above");
            return Some(format!(
                "{} workspace receipt timed out after {}s at {}x{}",
                receipt.id(),
                self.config.workspace_receipt_stage_timeout.as_secs_f32(),
                self.width,
                self.height
            ));
        }
        if !matches!(
            self.config.workspace_receipt,
            Some(WorkspaceReceipt::Mixed | WorkspaceReceipt::Chrome)
        ) || self.receipt_complete
            || self.workspace_receipt_stage_started.elapsed()
                < self.config.workspace_receipt_stage_timeout
        {
            return None;
        }
        #[cfg(target_os = "windows")]
        let progress = {
            let stats = self.native_surfaces.stats();
            format!(
                "frames={} imports={} waits={} compositions={} verified_sizes={:?}",
                stats.frames,
                stats.imports,
                stats.waits,
                stats.compositions,
                self.mixed_verified_sizes
            )
        };
        #[cfg(not(target_os = "windows"))]
        let progress = "native surface unavailable on this platform".to_owned();
        Some(format!(
            "native workspace receipt timed out after {}s at {}x{} ({progress})",
            self.config.workspace_receipt_stage_timeout.as_secs_f32(),
            self.width,
            self.height
        ))
    }

    fn drive_mixed_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        #[cfg(target_os = "windows")]
        if self.scrying_host.is_some() && !self.mixed_size_matrix_ready()? {
            return Ok(None);
        }
        if self.receipt_step == 0 {
            self.drive_mixed_workspace_receipt()?;
            self.receipt_step = 1;
        }
        Ok(Some(MIXED_WORKSPACE_ASSERTION.to_owned()))
    }

    #[cfg(target_os = "windows")]
    fn mixed_size_matrix_ready(&mut self) -> Result<bool, String> {
        let Some(sizes) = self.config.workspace_size_matrix.clone() else {
            return Ok(self.mixed_native_receipt_ready());
        };
        let Some(&initial) = sizes.first() else {
            return Err("mixed workspace size matrix is empty".to_owned());
        };

        if self.mixed_verified_sizes.is_empty() {
            if !self.mixed_native_receipt_ready() || !self.mixed_size_is_composed(initial, None)? {
                return Ok(false);
            }
            self.mixed_verified_sizes.push(initial);
            if sizes.len() == 1 {
                return Ok(true);
            }
            self.request_mixed_resize(sizes[1]);
            return Ok(false);
        }

        let Some(pending) = self.mixed_pending_resize else {
            return Err("mixed workspace size matrix lost its pending resize".to_owned());
        };
        if !self.mixed_size_is_composed(pending.target, Some(pending))? {
            return Ok(false);
        }
        self.mixed_verified_sizes.push(pending.target);
        self.mixed_pending_resize = None;
        if let Some(&next) = sizes.get(self.mixed_verified_sizes.len()) {
            self.request_mixed_resize(next);
            Ok(false)
        } else {
            Ok(true)
        }
    }

    #[cfg(target_os = "windows")]
    fn mixed_size_is_composed(
        &self,
        target: (u32, u32),
        baseline: Option<MixedPendingResize>,
    ) -> Result<bool, String> {
        if (self.width, self.height) != target {
            return Ok(false);
        }
        let route = self
            .workspace
            .route(TileId(4))
            .ok_or("mixed resize receipt lost surface tile 4")?;
        if !matches!(route.state, PeltRouteState::Surface) {
            return Err(format!(
                "mixed resize receipt changed tile 4 from a native surface: {route:?}"
            ));
        }
        let rect = self
            .workspace
            .content_rect(TileId(4))
            .ok_or("mixed resize receipt has no content geometry for tile 4")?;
        let (logical_width, logical_height) = self.logical_size();
        if !rect.x.is_finite()
            || !rect.y.is_finite()
            || !rect.width.is_finite()
            || !rect.height.is_finite()
            || rect.width <= 0.0
            || rect.height <= 0.0
            || rect.x < 0.0
            || rect.y < 0.0
            || rect.x + rect.width > logical_width as f32 + 1.0
            || rect.y + rect.height > logical_height as f32 + 1.0
        {
            return Err(format!(
                "mixed resize receipt produced invalid tile 4 geometry at {}x{}: {rect:?}",
                target.0, target.1
            ));
        }
        let expected = (
            physical_extent(rect.width, self.scale_factor),
            physical_extent(rect.height, self.scale_factor),
        );
        if self.native_surfaces.dimensions(TileId(4)) != Some(expected) {
            return Ok(false);
        }
        let Some(baseline) = baseline else {
            return Ok(true);
        };
        let stats = self.native_surfaces.stats();
        Ok(stats.frames > baseline.frames
            && stats.imports > baseline.imports
            && stats.waits > baseline.waits
            && stats.compositions > baseline.compositions)
    }

    #[cfg(target_os = "windows")]
    fn request_mixed_resize(&mut self, target: (u32, u32)) {
        let stats = self.native_surfaces.stats();
        self.mixed_pending_resize = Some(MixedPendingResize {
            target,
            frames: stats.frames,
            imports: stats.imports,
            waits: stats.waits,
            compositions: stats.compositions,
        });
        self.workspace_receipt_stage_started = Instant::now();
        if let Some(window) = self.window.as_ref() {
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(target.0, target.1));
        }
        self.request_redraw();
    }

    fn drive_mixed_workspace_receipt(&mut self) -> Result<String, String> {
        let tile = TileId(1);
        let rect = self
            .workspace
            .content_rect(tile)
            .ok_or("mixed receipt has no Frisket content geometry for tile 1")?;
        let link = self
            .workspace
            .controller(tile)
            .and_then(|controller| {
                controller
                    .links()
                    .into_iter()
                    .find(|link| link.url.ends_with("static.html"))
            })
            .ok_or("mixed receipt Gemtext tile has no static.html link")?;
        let x = rect.x + link.rect[0] + 2.0;
        let y = rect.y + link.rect[1] + 2.0;
        let hit = self.frisket.hit(x, y);
        if hit != Some(FrisketHit::Content(tile)) {
            return Err(format!(
                "mixed receipt link missed tile 1's Frisket content hole: {hit:?}"
            ));
        }
        self.pointer_move(x, y);
        let _ = self.pointer_down();
        // Smolweb uses Inker's compatibility click-on-press floor. Pointer-up
        // still closes capture, but the replacement session need not redraw.
        let _ = self.pointer_up();
        if !self
            .workspace
            .controller(tile)
            .is_some_and(|controller| controller.address().ends_with("static.html"))
        {
            return Err("mixed receipt Gemtext gesture did not navigate tile 1".to_owned());
        }
        // P4 pins the initial Gemtext fixture so the native proof does not
        // depend on host MIME inference. The real link has now updated the
        // held address; release only that fixture pin and let the shared
        // policy choose the HTML owner for this tile.
        if !self.workspace.set_route_override(tile, None)? {
            return Err("mixed receipt did not release tile 1's Gemtext fixture pin".to_owned());
        }
        self.frisket.set_tree(self.workspace.tree());
        let first = self
            .workspace
            .route(tile)
            .ok_or("mixed receipt lost tile 1 route")?;
        if first.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
            || !matches!(first.state, PeltRouteState::Document)
            || self.workspace.content_rect(tile) != Some(rect)
        {
            return Err(format!(
                "mixed receipt tile 1 did not reroute to Livery: {first:?}"
            ));
        }
        let title = self
            .workspace
            .controller(tile)
            .and_then(PeltController::inspect)
            .and_then(|report| report.title)
            .ok_or("mixed receipt rerouted Livery tile has no title")?;
        if title != "Static Livery" {
            return Err(format!(
                "mixed receipt tile 1 retained {title:?}, not Static Livery"
            ));
        }
        let second = self
            .workspace
            .route(TileId(2))
            .ok_or("mixed receipt lost the sibling Livery tile")?;
        let second_report = self
            .workspace
            .controller(TileId(2))
            .and_then(PeltController::inspect);
        if second.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
            || !matches!(second.state, PeltRouteState::Document)
            || !second_report.is_some_and(|report| {
                report.title.as_deref() == Some("Static Livery")
                    && report.headings == ["Static Livery"]
            })
        {
            return Err(format!(
                "mixed receipt changed its sibling Livery tile: {second:?}"
            ));
        }
        let third = self
            .workspace
            .route(TileId(3))
            .ok_or("mixed receipt lost scripted tile")?;
        if third.selected_engine() != inker::routing::ENGINE_GENET_SCRIPTED {
            return Err(format!(
                "mixed receipt changed scripted tile route: {third:?}"
            ));
        }
        let scripted = self
            .workspace
            .controller(TileId(3))
            .and_then(PeltController::inspect)
            .ok_or("mixed receipt scripted tile has no structural report")?;
        if scripted.title.as_deref() != Some("Scripted Livery")
            || !scripted
                .outline
                .iter()
                .any(|entry| entry.name == "Boa mutated this retained DOM")
        {
            return Err("mixed receipt changed its sibling scripted DOM".to_owned());
        }
        let fourth = self
            .workspace
            .route(TileId(4))
            .ok_or("mixed receipt lost surface tile")?;
        #[cfg(target_os = "windows")]
        let stable_fourth = if self.scrying_host.is_some() {
            matches!(fourth.state, PeltRouteState::Surface)
        } else {
            matches!(
                fourth.state,
                PeltRouteState::Fallback { ref active_engine, .. }
                    if active_engine == inker::routing::ENGINE_GENET_LIVERY
            )
        };
        #[cfg(not(target_os = "windows"))]
        let stable_fourth = matches!(
            fourth.state,
            PeltRouteState::Fallback { ref active_engine, .. }
                if active_engine == inker::routing::ENGINE_GENET_LIVERY
        );
        if fourth.selected_engine() != inker::routing::ENGINE_SCRYING_WEB || !stable_fourth {
            return Err(format!(
                "mixed receipt changed surface/fallback owner: {fourth:?}"
            ));
        }
        Ok(MIXED_WORKSPACE_ASSERTION.to_owned())
    }

    fn drive_chrome_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        #[cfg(target_os = "windows")]
        if self.scrying_host.is_some() && !self.mixed_native_receipt_ready() {
            return Ok(None);
        }
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 4)?;
                let address = self
                    .frisket
                    .chrome_rect("address")
                    .ok_or("chrome receipt has no retained address field")?;
                let engine = self
                    .frisket
                    .chrome_rect("engine-menu")
                    .ok_or("chrome receipt has no retained engine control")?;
                let tile = self
                    .workspace
                    .content_rect(TileId(1))
                    .ok_or("chrome receipt has no first tile geometry")?;
                if address.width <= 0.0
                    || engine.width <= 0.0
                    || tile.y <= address.y + address.height
                {
                    return Err(
                        "chrome receipt did not reserve a header above the pane frame".to_owned(),
                    );
                }
                self.click_tab(TileId(2))?;
            },
            1 => {
                let route = self
                    .workspace
                    .route(TileId(2))
                    .ok_or("chrome receipt lost focused Livery tile")?;
                if self.workspace.focused_tile() != Some(TileId(2))
                    || route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                    || route.source != PeltRouteSource::Automatic
                    || !matches!(route.state, PeltRouteState::Document)
                {
                    return Err(format!(
                        "chrome receipt did not focus ordinary Livery tile: {route:?}"
                    ));
                }
                self.click_chrome("address")?;
            },
            2 => {
                if !self.handle_chrome_key(
                    &Key::Character("surface.html".into()),
                    ElementState::Pressed,
                ) || !self.handle_chrome_key(&Key::Named(NamedKey::Enter), ElementState::Pressed)
                {
                    return Err("chrome receipt could not submit the address field".to_owned());
                }
                if !self
                    .workspace
                    .controller(TileId(2))
                    .is_some_and(|controller| controller.address().ends_with("surface.html"))
                {
                    return Err(
                        "chrome receipt address field did not navigate the focused tile".to_owned(),
                    );
                }
                if self.chrome_status != ChromeStatus::Loading {
                    return Err("chrome receipt did not expose its loading state".to_owned());
                }
            },
            3 => {
                self.click_chrome("back")?;
                let controller = self
                    .workspace
                    .controller(TileId(2))
                    .ok_or("chrome receipt lost focused controller after Back")?;
                if !controller.address().ends_with("static.html") || !controller.can_go_forward() {
                    return Err("chrome receipt Back did not retain forward history".to_owned());
                }
            },
            4 => {
                self.click_chrome("forward")?;
                let controller = self
                    .workspace
                    .controller(TileId(2))
                    .ok_or("chrome receipt lost focused controller after Forward")?;
                if !controller.address().ends_with("surface.html") || !controller.can_go_back() {
                    return Err("chrome receipt Forward did not retain back history".to_owned());
                }
            },
            5 => {
                self.click_chrome("reload")?;
                let controller = self
                    .workspace
                    .controller(TileId(2))
                    .ok_or("chrome receipt lost focused controller after Reload")?;
                if !controller.address().ends_with("surface.html") || !controller.can_go_back() {
                    return Err("chrome receipt Reload did not retain focused history".to_owned());
                }
            },
            6 => {
                self.click_chrome("engine-menu")?;
            },
            7 => {
                let automatic = self
                    .frisket
                    .chrome_rect("engine-automatic")
                    .ok_or("chrome receipt did not render an Automatic menu choice")?;
                let livery = self
                    .frisket
                    .chrome_rect("engine-livery")
                    .ok_or("chrome receipt did not render a Livery menu choice")?;
                if automatic.width <= 0.0
                    || livery.width <= 0.0
                    || !matches!(
                        self.frisket.hit(
                            livery.x + livery.width / 2.0,
                            livery.y + livery.height / 2.0
                        ),
                        Some(FrisketHit::ChromeAction(ChromeAction::ChooseEngine(
                            ChromeEngineChoice::Livery
                        )))
                    )
                    || self.chrome_model().engine_selected != Some(ChromeEngineChoice::Automatic)
                {
                    return Err(
                        "chrome receipt did not expose an explicit selected engine menu".to_owned(),
                    );
                }
                #[cfg(feature = "scripted")]
                if self.frisket.chrome_rect("engine-scripted").is_none() {
                    return Err(
                        "chrome receipt omitted the compiled Scripted menu choice".to_owned()
                    );
                }
                self.click_tab(TileId(3))?;
            },
            8 => {
                let route = self
                    .workspace
                    .route(TileId(2))
                    .ok_or("chrome receipt lost focused tile while dismissing the menu")?;
                let scripted = self
                    .workspace
                    .route(TileId(3))
                    .ok_or("chrome receipt lost scripted neighbor while dismissing the menu")?;
                if self.workspace.focused_tile() != Some(TileId(3))
                    || self.frisket.chrome_rect("engine-livery").is_some()
                    || route.source != PeltRouteSource::Automatic
                    || scripted.selected_engine() != inker::routing::ENGINE_GENET_SCRIPTED
                {
                    return Err(format!(
                        "chrome receipt did not dismiss the tile-bound engine menu: {route:?}"
                    ));
                }
                self.click_tab(TileId(2))?;
            },
            9 => {
                if self.workspace.focused_tile() != Some(TileId(2)) {
                    return Err(
                        "chrome receipt could not restore its original focused tile".to_owned()
                    );
                }
                self.click_chrome("engine-menu")?;
            },
            10 => {
                self.click_chrome("engine-livery")?;
            },
            11 => {
                let route = self
                    .workspace
                    .route(TileId(2))
                    .ok_or("chrome receipt lost route after choosing Livery")?;
                if route.source != PeltRouteSource::UserOverride
                    || route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                {
                    return Err(format!("chrome receipt did not pin Livery: {route:?}"));
                }
                self.click_chrome("engine-menu")?;
            },
            12 => {
                #[cfg(feature = "scripted")]
                self.click_chrome("engine-scripted")?;
                #[cfg(not(feature = "scripted"))]
                self.click_chrome("engine-automatic")?;
            },
            13 => {
                let route = self
                    .workspace
                    .route(TileId(2))
                    .ok_or("chrome receipt lost route after its second engine choice")?;
                #[cfg(feature = "scripted")]
                if route.source != PeltRouteSource::UserOverride
                    || route.selected_engine() != inker::routing::ENGINE_GENET_SCRIPTED
                {
                    return Err(format!(
                        "chrome receipt did not select scripted engine: {route:?}"
                    ));
                }
                #[cfg(not(feature = "scripted"))]
                if route.source != PeltRouteSource::Automatic
                    || route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                {
                    return Err(format!(
                        "chrome receipt did not restore automatic Livery: {route:?}"
                    ));
                }
                #[cfg(feature = "scripted")]
                self.click_chrome("engine-menu")?;
                #[cfg(not(feature = "scripted"))]
                {
                    self.validate_chrome_workspace_receipt()?;
                    self.receipt_step = self.receipt_step.saturating_add(1);
                    return Ok(Some(CHROME_WORKSPACE_ASSERTION.to_owned()));
                }
            },
            14 => {
                self.click_chrome("engine-automatic")?;
            },
            15 => {
                let route = self
                    .workspace
                    .route(TileId(2))
                    .ok_or("chrome receipt lost route while restoring automatic selection")?;
                if route.source != PeltRouteSource::Automatic
                    || route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                    || !matches!(route.state, PeltRouteState::Document)
                {
                    return Err(format!(
                        "chrome receipt did not restore automatic Livery: {route:?}"
                    ));
                }
                self.click_chrome("engine-menu")?;
            },
            16 => {
                let automatic = self
                    .frisket
                    .chrome_rect("engine-automatic")
                    .ok_or("chrome receipt did not keep its final Automatic choice visible")?;
                if automatic.width <= 0.0
                    || self.chrome_model().engine_selected != Some(ChromeEngineChoice::Automatic)
                {
                    return Err(
                        "chrome receipt did not expose its restored automatic selection".to_owned(),
                    );
                }
                self.click_chrome("inspect")?;
            },
            17 => {
                let inspector = self
                    .chrome_model()
                    .inspector
                    .ok_or("chrome receipt did not open its content inspector")?;
                if inspector.status != "Partial structural report"
                    || inspector.title.as_deref() != Some("Scrying native surface")
                    || !inspector.sections.iter().any(|section| {
                        section.label == "Headings (1)"
                            && section
                                .entries
                                .iter()
                                .any(|entry| entry == "Scrying native surface")
                    })
                    || self.frisket.chrome_rect("engine-automatic").is_some()
                {
                    return Err(
                        "chrome receipt did not expose the focused Livery structural report"
                            .to_owned(),
                    );
                }
                self.click_tab(TileId(4))?;
            },
            18 => {
                let inspector = self
                    .chrome_model()
                    .inspector
                    .ok_or("chrome receipt lost its content inspector after focus change")?;
                if self.workspace.focused_tile() != Some(TileId(4)) {
                    return Err(
                        "chrome receipt could not focus its external surface tile".to_owned()
                    );
                }
                #[cfg(target_os = "windows")]
                let opaque_surface = self.scrying_host.is_some();
                #[cfg(not(target_os = "windows"))]
                let opaque_surface = false;
                if opaque_surface {
                    if inspector.status != "Opaque surface"
                        || inspector.summary != "Contents not inspectable on this surface."
                        || !inspector.sections.is_empty()
                    {
                        return Err(
                            "chrome receipt did not disclose the opaque surface honestly"
                                .to_owned(),
                        );
                    }
                } else if inspector.status != "Partial structural report"
                    || inspector.title.as_deref() != Some("Scrying native surface")
                {
                    return Err(
                        "chrome receipt did not inspect the active Livery fallback".to_owned()
                    );
                }
                self.validate_chrome_workspace_receipt()?;
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(CHROME_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(CHROME_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    fn drive_loading_error_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        let tile = TileId(1);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                let address = self
                    .frisket
                    .chrome_rect("address")
                    .ok_or("loading/error receipt has no retained address field")?;
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("loading/error receipt has no Frisket content geometry")?;
                if address.width <= 0.0 || content.y <= address.y + address.height {
                    return Err(
                        "loading/error receipt did not reserve Chrome above the content hole"
                            .to_owned(),
                    );
                }
                self.click_chrome("address")?;
            },
            1 => {
                if !self
                    .handle_chrome_key(&Key::Character("next.html".into()), ElementState::Pressed)
                    || !self.handle_chrome_key(&Key::Named(NamedKey::Enter), ElementState::Pressed)
                {
                    return Err(
                        "loading/error receipt could not submit its initial document address"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("loading/error receipt lost its focused controller")?;
                if !controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/p6-load-error/next.html")
                    || !controller.can_go_back()
                    || !matches!(
                        controller.document_state(),
                        PeltDocumentState::Loading { address }
                            if address.replace('\\', "/").ends_with("/p6-load-error/next.html")
                    )
                {
                    return Err(
                        "loading/error receipt did not retain its successful loading transition"
                            .to_owned(),
                    );
                }
            },
            2 => {
                let diagnostic = self
                    .last_chrome_document
                    .clone()
                    .ok_or("loading/error receipt did not compose its loading document")?;
                if diagnostic.kind != ChromeDocumentKind::Loading
                    || diagnostic.tile != tile
                    || !diagnostic
                        .address
                        .replace('\\', "/")
                        .ends_with("/p6-load-error/next.html")
                    || self.workspace.content_rect(tile) != Some(diagnostic.rect)
                {
                    return Err(
                        "loading/error receipt did not place the loading document in the focused content hole"
                            .to_owned(),
                    );
                }
                self.click_chrome("address")?;
            },
            3 => {
                if !self.handle_chrome_key(
                    &Key::Character("missing.html".into()),
                    ElementState::Pressed,
                ) || !self.handle_chrome_key(&Key::Named(NamedKey::Enter), ElementState::Pressed)
                {
                    return Err(
                        "loading/error receipt could not submit its missing document address"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("loading/error receipt lost its prior controller after failure")?;
                let PeltDocumentState::Error { address, message } = controller.document_state()
                else {
                    return Err(
                        "loading/error receipt did not expose its failed navigation state"
                            .to_owned(),
                    );
                };
                if !address
                    .replace('\\', "/")
                    .ends_with("/p6-load-error/missing.html")
                    || !message.contains("could not load")
                    || !controller
                        .address()
                        .replace('\\', "/")
                        .ends_with("/p6-load-error/next.html")
                    || !controller.can_go_back()
                {
                    return Err(
                        "loading/error receipt replaced the prior document or hid its failed address"
                            .to_owned(),
                    );
                }
            },
            4 => {
                let diagnostic = self
                    .last_chrome_document
                    .clone()
                    .ok_or("loading/error receipt did not compose its error document")?;
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("loading/error receipt lost its retained controller")?;
                if diagnostic.kind != ChromeDocumentKind::Error
                    || diagnostic.tile != tile
                    || !diagnostic
                        .address
                        .replace('\\', "/")
                        .ends_with("/p6-load-error/missing.html")
                    || !diagnostic
                        .message
                        .as_deref()
                        .is_some_and(|message| message.contains("could not load"))
                    || self.workspace.content_rect(tile) != Some(diagnostic.rect)
                    || !controller
                        .address()
                        .replace('\\', "/")
                        .ends_with("/p6-load-error/next.html")
                    || !controller.can_go_back()
                {
                    return Err(
                        "loading/error receipt did not preserve the prior document beneath its error projection"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(LOADING_ERROR_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(LOADING_ERROR_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    fn drive_narrow_chrome_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        let tile = TileId(1);
        let viewport = self.logical_size();
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                if viewport != (360, 480) {
                    return Err(format!(
                        "narrow Chrome receipt needs a 360x480 logical viewport, got {}x{} at {:.2}x",
                        viewport.0, viewport.1, self.scale_factor
                    ));
                }
                let back = self
                    .frisket
                    .chrome_rect("back")
                    .ok_or("narrow Chrome receipt has no Back control")?;
                let address = self
                    .frisket
                    .chrome_rect("address")
                    .ok_or("narrow Chrome receipt has no retained address field")?;
                for action in ["forward", "reload", "engine-menu", "inspect", "appearance"] {
                    let rect = self.frisket.chrome_rect(action).ok_or_else(|| {
                        format!("narrow Chrome receipt has no {action:?} geometry")
                    })?;
                    if !rect_fits_viewport(rect, viewport) {
                        return Err(format!(
                            "narrow Chrome control {action:?} escaped its {}x{} logical viewport: {rect:?}",
                            viewport.0, viewport.1
                        ));
                    }
                }
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("narrow Chrome receipt has no Frisket content geometry")?;
                let tab = self
                    .frisket
                    .tab_rect(tile)
                    .ok_or("narrow Chrome receipt has no retained tab geometry")?;
                let label = self
                    .frisket
                    .tab_label_rect(tile)
                    .ok_or("narrow Chrome receipt has no visible tab label geometry")?;
                let close = self
                    .frisket
                    .close_rect(tile)
                    .ok_or("narrow Chrome receipt has no retained tab close target")?;
                if !rect_fits_viewport(back, viewport)
                    || !rect_fits_viewport(address, viewport)
                    || !rect_fits_viewport(tab, viewport)
                    || !rect_fits_viewport(label, viewport)
                    || !rect_fits_viewport(close, viewport)
                    || address.y < back.y + back.height
                    || address.width < 300.0
                    || content.y <= address.y + address.height
                    || label.width < 48.0
                    || close.x < tab.x
                    || close.x + close.width > tab.x + tab.width
                    || self
                        .frisket
                        .hit(close.x + close.width / 2.0, close.y + close.height / 2.0)
                        != Some(FrisketHit::Close(tile))
                {
                    return Err(
                        "narrow Chrome receipt lost its readable address row, tab label, or independent close target"
                            .to_owned(),
                    );
                }
                self.click_chrome("address")?;
            },
            1 => {
                if !self
                    .handle_chrome_key(&Key::Character("next.html".into()), ElementState::Pressed)
                    || !self.handle_chrome_key(&Key::Named(NamedKey::Enter), ElementState::Pressed)
                {
                    return Err(
                        "narrow Chrome receipt could not submit its initial document address"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("narrow Chrome receipt lost its focused controller")?;
                if !controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/p6-load-error/next.html")
                    || !controller.can_go_back()
                    || !matches!(
                        controller.document_state(),
                        PeltDocumentState::Loading { address }
                            if address.replace('\\', "/").ends_with("/p6-load-error/next.html")
                    )
                {
                    return Err(
                        "narrow Chrome receipt did not retain its successful loading transition"
                            .to_owned(),
                    );
                }
            },
            2 => {
                let diagnostic = self
                    .last_chrome_document
                    .clone()
                    .ok_or("narrow Chrome receipt did not compose its loading document")?;
                if diagnostic.kind != ChromeDocumentKind::Loading
                    || diagnostic.tile != tile
                    || self.workspace.content_rect(tile) != Some(diagnostic.rect)
                {
                    return Err(
                        "narrow Chrome receipt did not keep its loading document in the content hole"
                            .to_owned(),
                    );
                }
                self.click_chrome("engine-menu")?;
            },
            3 => {
                let content = self.workspace.content_rect(tile).ok_or(
                    "narrow Chrome receipt lost its content hole while the engine menu opened",
                )?;
                let automatic = self
                    .frisket
                    .chrome_rect("engine-automatic")
                    .ok_or("narrow Chrome receipt did not expose Automatic")?;
                let livery = self
                    .frisket
                    .chrome_rect("engine-livery")
                    .ok_or("narrow Chrome receipt did not expose Livery")?;
                if !rect_fits_viewport(automatic, viewport)
                    || !rect_fits_viewport(livery, viewport)
                    || content.y <= automatic.y + automatic.height
                    || content.y <= livery.y + livery.height
                {
                    return Err(
                        "narrow Chrome receipt let its engine choices cover content or escape the viewport"
                            .to_owned(),
                    );
                }
                self.click_chrome("engine-menu")?;
            },
            4 => {
                if self.frisket.chrome_rect("engine-automatic").is_some() {
                    return Err("narrow Chrome receipt did not dismiss its engine menu".to_owned());
                }
                self.click_chrome("address")?;
            },
            5 => {
                if !self.handle_chrome_key(
                    &Key::Character("missing.html".into()),
                    ElementState::Pressed,
                ) || !self.handle_chrome_key(&Key::Named(NamedKey::Enter), ElementState::Pressed)
                {
                    return Err(
                        "narrow Chrome receipt could not submit its missing document address"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("narrow Chrome receipt lost its prior controller after failure")?;
                let PeltDocumentState::Error { address, message } = controller.document_state()
                else {
                    return Err(
                        "narrow Chrome receipt did not expose its failed navigation state"
                            .to_owned(),
                    );
                };
                if !address
                    .replace('\\', "/")
                    .ends_with("/p6-load-error/missing.html")
                    || !message.contains("could not load")
                    || !controller
                        .address()
                        .replace('\\', "/")
                        .ends_with("/p6-load-error/next.html")
                    || !controller.can_go_back()
                {
                    return Err(
                        "narrow Chrome receipt replaced its retained document after a failed navigation"
                            .to_owned(),
                    );
                }
            },
            6 => {
                let diagnostic = self
                    .last_chrome_document
                    .clone()
                    .ok_or("narrow Chrome receipt did not compose its error document")?;
                if diagnostic.kind != ChromeDocumentKind::Error
                    || diagnostic.tile != tile
                    || self.workspace.content_rect(tile) != Some(diagnostic.rect)
                    || !diagnostic
                        .address
                        .replace('\\', "/")
                        .ends_with("/p6-load-error/missing.html")
                {
                    return Err(
                        "narrow Chrome receipt did not preserve its error document in the content hole"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(NARROW_CHROME_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(NARROW_CHROME_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    fn drive_chrome_dpi_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        let tile = TileId(1);
        let viewport = self.logical_size();
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                if self.scale_factor < 1.25 {
                    return Err(format!(
                        "Chrome DPI receipt needs an actual high-DPI monitor (at least 1.25x), got {:.2}x",
                        self.scale_factor
                    ));
                }
                let expected_physical = (
                    physical_extent(960.0, self.scale_factor),
                    physical_extent(640.0, self.scale_factor),
                );
                if viewport != (960, 640) || (self.width, self.height) != expected_physical {
                    return Err(format!(
                        "Chrome DPI receipt needs a 960x640 logical viewport at {}x{}, got {}x{} logical at {}x{} physical and {:.2}x",
                        expected_physical.0,
                        expected_physical.1,
                        viewport.0,
                        viewport.1,
                        self.width,
                        self.height,
                        self.scale_factor
                    ));
                }
                let appearance = self
                    .frisket
                    .chrome_rect("appearance")
                    .ok_or("Chrome DPI receipt has no Theme control")?;
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("Chrome DPI receipt has no Frisket content geometry")?;
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("Chrome DPI receipt lost its focused controller")?;
                if !rect_fits_viewport(appearance, viewport)
                    || content.y <= appearance.y + appearance.height
                    || self.chrome_theme != ChromeTheme::Dark
                    || self.chrome_appearance_open
                {
                    return Err(
                        "Chrome DPI receipt did not begin with a usable Theme control and content hole"
                            .to_owned(),
                    );
                }
                self.appearance_receipt_baseline = Some(AppearanceReceiptBaseline {
                    content,
                    address: controller.address().to_owned(),
                    can_go_back: controller.can_go_back(),
                });
                self.click_chrome_physical("appearance")?;
            },
            1 => {
                let light = self
                    .frisket
                    .chrome_rect("appearance-light")
                    .ok_or("Chrome DPI receipt did not render a Light choice")?;
                if !self.chrome_appearance_open
                    || !rect_fits_viewport(light, viewport)
                    || !matches!(
                        self.frisket
                            .hit(light.x + light.width / 2.0, light.y + light.height / 2.0),
                        Some(FrisketHit::ChromeAction(ChromeAction::ChooseTheme(
                            ChromeTheme::Light
                        )))
                    )
                {
                    return Err(
                        "Chrome DPI receipt did not expose an interactive retained Light choice"
                            .to_owned(),
                    );
                }
                self.click_chrome_physical("appearance-light")?;
            },
            2 => {
                let baseline = self
                    .appearance_receipt_baseline
                    .as_ref()
                    .ok_or("Chrome DPI receipt lost its baseline document state")?;
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("Chrome DPI receipt lost its focused controller")?;
                let drawer = self
                    .frisket
                    .frame(viewport.0, viewport.1)
                    .map_err(|error| {
                        format!("Chrome DPI receipt could not lay out its drawer: {error}")
                    })?
                    .appearance_rect
                    .ok_or("Chrome DPI receipt did not retain its appearance drawer")?;
                let placement =
                    fragment_placement(drawer, (self.width, self.height), self.scale_factor);
                let [x0, y0, x1, y1] = placement.dest_rect;
                if self.chrome_theme != ChromeTheme::Light
                    || self.workspace.content_rect(tile) != Some(baseline.content)
                    || controller.address() != baseline.address.as_str()
                    || controller.can_go_back() != baseline.can_go_back
                    || x0 < 0.0
                    || y0 < 0.0
                    || x1 > self.width as f32
                    || y1 > self.height as f32
                {
                    return Err(
                        "Chrome DPI receipt changed document state or produced an out-of-bounds physical drawer crop"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(format!(
                    "{CHROME_DPI_WORKSPACE_ASSERTION_PREFIX} at {:.2}x; the 960x640 logical shell captured at {}x{} physical without moving its content aperture",
                    self.scale_factor, self.width, self.height
                )));
            },
            _ => {
                return Ok(Some(format!(
                    "{CHROME_DPI_WORKSPACE_ASSERTION_PREFIX} at {:.2}x; the 960x640 logical shell captured at {}x{} physical without moving its content aperture",
                    self.scale_factor, self.width, self.height
                )));
            },
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    fn drive_appearance_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        let tile = TileId(1);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                let trigger = self
                    .frisket
                    .chrome_rect("appearance")
                    .ok_or("appearance receipt has no retained Theme control")?;
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("appearance receipt has no Frisket content geometry")?;
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("appearance receipt lost its focused controller")?;
                if trigger.width <= 0.0
                    || trigger.height <= 0.0
                    || content.y <= trigger.y + trigger.height
                    || self.chrome_theme != ChromeTheme::Dark
                    || self.chrome_appearance_open
                {
                    return Err(
                        "appearance receipt did not begin with a usable dark Chrome control and content hole"
                            .to_owned(),
                    );
                }
                self.appearance_receipt_baseline = Some(AppearanceReceiptBaseline {
                    content,
                    address: controller.address().to_owned(),
                    can_go_back: controller.can_go_back(),
                });
                self.click_chrome("appearance")?;
            },
            1 => {
                let chrome = self.chrome_model();
                let light = self
                    .frisket
                    .chrome_rect("appearance-light")
                    .ok_or("appearance receipt did not render a Light choice")?;
                if chrome.theme != ChromeTheme::Dark
                    || chrome.appearance
                        != Some(ChromeAppearance {
                            theme: ChromeTheme::Dark,
                        })
                    || light.width <= 0.0
                    || light.height <= 0.0
                    || !matches!(
                        self.frisket
                            .hit(light.x + light.width / 2.0, light.y + light.height / 2.0),
                        Some(FrisketHit::ChromeAction(ChromeAction::ChooseTheme(
                            ChromeTheme::Light
                        )))
                    )
                {
                    return Err(
                        "appearance receipt did not expose an interactive dark/light drawer"
                            .to_owned(),
                    );
                }
                self.click_chrome("appearance-light")?;
            },
            2 => {
                let baseline = self
                    .appearance_receipt_baseline
                    .as_ref()
                    .ok_or("appearance receipt lost its baseline document state")?;
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("appearance receipt lost its focused controller")?;
                let chrome = self.chrome_model();
                if self.chrome_theme != ChromeTheme::Light
                    || chrome.theme != ChromeTheme::Light
                    || chrome.appearance
                        != Some(ChromeAppearance {
                            theme: ChromeTheme::Light,
                        })
                    || self.workspace.content_rect(tile) != Some(baseline.content)
                    || controller.address() != baseline.address.as_str()
                    || controller.can_go_back() != baseline.can_go_back
                {
                    return Err(
                        "appearance receipt changed document state or did not apply the light Chrome palette"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(APPEARANCE_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(APPEARANCE_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    fn drive_accessibility_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        let tile = TileId(1);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                if self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "accessibility receipt began without an installed platform bridge"
                            .to_owned(),
                    );
                }
                let tree = self.prepare_accessibility_tree()?;
                let content = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.role() == Role::Region
                            && node
                                .label()
                                .is_some_and(|label| label.ends_with(" content"))
                    })
                    .map(|(_, node)| node)
                    .ok_or("accessibility receipt did not expose a named content aperture")?;
                if !content
                    .description()
                    .is_some_and(|description| description.contains("partial accessibility"))
                {
                    return Err(
                        "accessibility receipt did not declare the document engine's partial child-tree boundary"
                            .to_owned(),
                    );
                }
                let theme = a11y_node(&tree, "Toggle session appearance settings", Role::Button)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Focus,
                    target_node: theme,
                }) || self.chrome_appearance_open
                    || self.chrome_theme != ChromeTheme::Dark
                {
                    return Err(
                        "accessibility Focus activated Pelt's appearance control instead of only moving virtual focus"
                            .to_owned(),
                    );
                }
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let theme = a11y_node(&tree, "Toggle session appearance settings", Role::Button)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: theme,
                }) || !self.chrome_appearance_open
                {
                    return Err(
                        "accessibility Click did not open Pelt's retained appearance controls"
                            .to_owned(),
                    );
                }
            },
            2 => {
                let tree = self.prepare_accessibility_tree()?;
                let light = a11y_node(&tree, "Light", Role::RadioButton)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Focus,
                    target_node: light,
                }) || self.chrome_theme != ChromeTheme::Dark
                    || !self.chrome_appearance_open
                {
                    return Err(
                        "accessibility Focus selected a Pelt theme instead of only moving virtual focus"
                            .to_owned(),
                    );
                }
            },
            3 => {
                let tree = self.prepare_accessibility_tree()?;
                let light = a11y_node(&tree, "Light", Role::RadioButton)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: light,
                }) || self.chrome_theme != ChromeTheme::Light
                    || !self.chrome_appearance_open
                {
                    return Err(
                        "accessibility Click did not select Pelt's Light session theme".to_owned(),
                    );
                }
            },
            4 => {
                let tree = self.prepare_accessibility_tree()?;
                let light = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.role() == Role::RadioButton && node.label() == Some("Light")
                    })
                    .map(|(_, node)| node)
                    .ok_or("accessibility receipt lost its Light radio after selection")?;
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility receipt lost its focused controller")?;
                if light.toggled() != Some(accesskit::Toggled::True)
                    || controller.address().is_empty()
                    || self.accessibility.focus.is_none()
                {
                    return Err(
                        "accessibility receipt lost the selected radio state, document, or virtual focus"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(ACCESSIBILITY_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    #[cfg(target_os = "windows")]
    fn mixed_native_receipt_ready(&mut self) -> bool {
        if !self.capability_receipt_ready() {
            return false;
        }
        let stats = self.native_surfaces.stats();
        if let Some(ready_frame) = self.mixed_content_ready_frame {
            return stats.frames > ready_frame;
        }
        let script = "Number(document.querySelector('#tick')?.textContent ?? 0) >= 1";
        let content_ready = self
            .workspace
            .execute_surface_script(TileId(4), script)
            .ok()
            .flatten()
            .is_some_and(|result| matches!(result.trim(), "true" | "\"true\""));
        if content_ready {
            self.mixed_content_ready_frame = Some(stats.frames);
        }
        false
    }

    fn drive_fallback_workspace_receipt_step(&mut self) -> Result<Option<String>, String> {
        if self.receipt_step == 0 {
            let initial = self
                .workspace
                .route(TileId(1))
                .ok_or("fallback receipt has no initial Livery route")?;
            if initial.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                || !matches!(initial.state, PeltRouteState::Document)
            {
                return Err(format!(
                    "fallback receipt did not start as ordinary Livery: {initial:?}"
                ));
            }
            if !self.workspace.set_route_override(
                TileId(1),
                Some(inker::routing::ENGINE_SCRYING_WEB.to_owned()),
            )? {
                return Err("fallback receipt did not apply its live Scrying override".to_owned());
            }
            self.frisket.set_tree(self.workspace.tree());
            self.validate_fallback_route("Pelt fallback receipt", "Fallback stayed visible")?;
            self.receipt_step = 1;
            return Ok(None);
        }

        let tile = TileId(1);
        let rect = self
            .workspace
            .content_rect(tile)
            .ok_or("fallback receipt has no Frisket content geometry")?;
        let link = self
            .workspace
            .controller(tile)
            .and_then(|controller| {
                controller
                    .links()
                    .into_iter()
                    .find(|link| link.url.ends_with("next.html"))
            })
            .ok_or("fallback receipt retained document has no next.html link")?;
        let x = rect.x + link.rect[0] + link.rect[2] * 0.5;
        let y = rect.y + link.rect[1] + link.rect[3] * 0.5;
        let hit = self.frisket.hit(x, y);
        if hit != Some(FrisketHit::Content(tile)) {
            return Err(format!(
                "fallback receipt link missed tile 1's Frisket content hole: {hit:?}"
            ));
        }
        self.pointer_move(x, y);
        let _ = self.pointer_down();
        let _ = self.pointer_up();
        self.validate_fallback_route(
            "Pelt fallback destination",
            "Fallback navigation stayed local",
        )?;
        if !self.workspace.controller(tile).is_some_and(|controller| {
            controller
                .address()
                .replace('\\', "/")
                .ends_with("/p5-fallback/next.html")
        }) {
            return Err("fallback receipt link did not retain its destination address".to_owned());
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(Some(
            "explicit Scrying pin fell back visibly to Livery; retained link navigation stayed interactive"
                .to_owned(),
        ))
    }

    fn validate_fallback_route(
        &self,
        expected_title: &str,
        expected_heading: &str,
    ) -> Result<(), String> {
        let route = self
            .workspace
            .route(TileId(1))
            .ok_or("fallback receipt has no tile route")?;
        let PeltRouteState::Fallback {
            active_engine,
            reason,
        } = &route.state
        else {
            return Err(format!(
                "fallback receipt did not retain fallback state: {route:?}"
            ));
        };
        if route.selected_engine() != inker::routing::ENGINE_SCRYING_WEB
            || active_engine != inker::routing::ENGINE_GENET_LIVERY
            || reason != "surface engine is not registered on this host"
        {
            return Err(format!("fallback receipt route drifted: {route:?}"));
        }
        let report = self
            .workspace
            .controller(TileId(1))
            .and_then(PeltController::inspect)
            .ok_or("fallback receipt Livery document has no report")?;
        if report.title.as_deref() != Some(expected_title) || report.headings != [expected_heading]
        {
            return Err(format!(
                "fallback receipt document report drifted: {report:?}"
            ));
        }
        let title = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .find(|tile| tile.id == TileId(1))
            .map(|tile| tile.title.as_str())
            .ok_or("fallback receipt lost its visible Frisket tile title")?;
        if !title.contains("[scrying.web → genet.livery]") {
            return Err(format!("fallback receipt hid its route title: {title:?}"));
        }
        Ok(())
    }

    fn validate_capability_receipt(&self) -> Result<(), String> {
        self.validate_mixed_workspace("P4", "Static Livery", "Static Livery")
    }

    fn validate_chrome_workspace_receipt(&self) -> Result<(), String> {
        self.validate_mixed_workspace(
            "chrome receipt",
            "Scrying native surface",
            "Scrying native surface",
        )
    }

    fn validate_mixed_workspace(
        &self,
        receipt: &str,
        expected_second_title: &str,
        expected_second_heading: &str,
    ) -> Result<(), String> {
        let expected = [
            (1, inker::routing::ENGINE_NEMATIC_GEMTEXT),
            (2, inker::routing::ENGINE_GENET_LIVERY),
            (3, inker::routing::ENGINE_GENET_SCRIPTED),
            (4, inker::routing::ENGINE_SCRYING_WEB),
        ];
        for (tile, engine) in expected {
            let route = self
                .workspace
                .route(TileId(tile))
                .ok_or_else(|| format!("P4 receipt is missing tile {tile}"))?;
            if route.selected_engine() != engine {
                return Err(format!(
                    "P4 tile {tile} selected {} instead of {engine}",
                    route.selected_engine()
                ));
            }
        }
        #[cfg(target_os = "windows")]
        if self.scrying_host.is_some()
            && !matches!(
                self.workspace.route(TileId(4)).map(|route| &route.state),
                Some(PeltRouteState::Surface)
            )
        {
            return Err("P4 external surface tile did not retain its native producer".to_owned());
        }
        #[cfg(target_os = "windows")]
        if self.scrying_host.is_none()
            && !matches!(
                self.workspace.route(TileId(4)).map(|route| &route.state),
                Some(PeltRouteState::Fallback { active_engine, .. })
                    if active_engine == inker::routing::ENGINE_GENET_LIVERY
            )
        {
            return Err("P4 external surface tile did not expose its Livery fallback".to_owned());
        }
        #[cfg(not(target_os = "windows"))]
        if !matches!(
            self.workspace.route(TileId(4)).map(|route| &route.state),
            Some(PeltRouteState::Fallback { active_engine, .. })
                if active_engine == inker::routing::ENGINE_GENET_LIVERY
        ) {
            return Err("P4 external surface tile did not expose its Livery fallback".to_owned());
        }
        let first = self
            .workspace
            .controller(TileId(1))
            .ok_or("P4 smolweb tile is not live")?;
        let second = self
            .workspace
            .controller(TileId(2))
            .ok_or("P4 static tile is not live")?;
        let third = self
            .workspace
            .controller(TileId(3))
            .ok_or("P4 scripted tile is not live")?;
        if !first.shares_registries_with(second) || !second.shares_registries_with(third) {
            return Err("P4 document tiles did not share their long-lived registries".to_owned());
        }
        let native = first
            .inspect()
            .ok_or("P4 smolweb tile did not expose a structural report")?;
        if !native
            .links
            .iter()
            .any(|link| link.ends_with("static.html"))
        {
            return Err("P4 smolweb tile did not parse its gemtext link".to_owned());
        }
        let static_report = second
            .inspect()
            .ok_or("P4 static tile did not expose a structural report")?;
        if static_report.title.as_deref() != Some(expected_second_title)
            || static_report.headings != [expected_second_heading]
        {
            return Err(format!(
                "{receipt} focused Livery tile did not retain {expected_second_title:?}: {static_report:?}"
            ));
        }
        let scripted = third
            .inspect()
            .ok_or("P4 scripted tile did not expose a structural report")?;
        if scripted.title.as_deref() != Some("Scripted Livery")
            || !scripted
                .outline
                .iter()
                .any(|entry| entry.name == "Boa mutated this retained DOM")
        {
            return Err("P4 scripted tile did not expose its post-Boa DOM".to_owned());
        }
        Ok(())
    }

    fn capability_receipt_ready(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            let stats = self.native_surfaces.stats();
            self.native_surfaces.view(TileId(4)).is_some()
                && stats.frames > stats.imports
                && stats.imports > 0
                && stats.waits >= 2
                && stats.compositions >= 2
        }
        #[cfg(not(target_os = "windows"))]
        {
            true
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn apply_effect(&mut self, effect: PeltHostEffect) {
        let PeltHostEffect {
            redraw,
            cursor,
            editable,
            navigated,
            error,
            ..
        } = effect;
        self.dismiss_chrome_engine_menu_if_focus_changed();
        if navigated {
            self.clear_chrome_engine_menu();
            self.clear_chrome_appearance();
        }
        if let Some(error) = error {
            eprintln!("[pelt-workspace] {error}");
            self.chrome_status = ChromeStatus::Error(error);
        } else if navigated {
            self.chrome_status = ChromeStatus::Loading;
        }
        if let Some(window) = &self.window {
            if let Some(cursor) = cursor {
                window.set_cursor(match cursor {
                    SessionCursor::Default => winit::window::CursorIcon::Default,
                    SessionCursor::Pointer => winit::window::CursorIcon::Pointer,
                    SessionCursor::Text => winit::window::CursorIcon::Text,
                });
            }
            window.set_ime_allowed(editable);
            if navigated {
                window.set_title(&self.window_title());
            }
        }
        if navigated {
            self.frisket.set_tree(self.workspace.tree());
        }
        if redraw {
            self.request_redraw();
        }
    }

    fn apply_tile_event(&mut self, event: TileEvent) -> bool {
        let focused_before = self.workspace.focused_tile();
        if self.workspace.apply(&event) {
            if self.workspace.focused_tile() != focused_before {
                self.clear_chrome_engine_menu();
                self.clear_chrome_appearance();
            }
            self.frisket.set_tree(self.workspace.tree());
            if let Some(window) = &self.window {
                window.set_title(&self.window_title());
            }
            true
        } else {
            false
        }
    }

    fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.cursor = (x, y);
        let mut redraw = false;
        if let Some(PointerGesture::Divider(drag)) = &self.gesture {
            let position = if drag.horizontal { x } else { y };
            let delta = (position - drag.start) / drag.extent.max(1.0);
            let minimum = drag.pair_total.min(0.05);
            let first =
                (drag.init_first + delta).clamp(minimum, (drag.pair_total - minimum).max(minimum));
            if let Some(mut fractions) = self.workspace.tree().fractions_at(&drag.target.path) {
                fractions[drag.target.index] = first;
                fractions[drag.target.index + 1] = drag.pair_total - first;
                let event = TileEvent::DividerMoved {
                    split: drag.target.path.clone(),
                    fractions,
                };
                redraw |= self.apply_tile_event(event);
            }
            return redraw;
        }
        if let Some(PointerGesture::Tab(drag)) = &mut self.gesture {
            if (x - drag.start.0).abs() + (y - drag.start.1).abs() > 6.0 {
                drag.moved = true;
            }
            return drag.moved;
        }
        if matches!(
            self.frisket.hit(x, y),
            Some(FrisketHit::Appearance | FrisketHit::ChromeAction(_))
        ) {
            return false;
        }

        let effect = self.workspace.input(SessionInput::PointerMoved {
            x,
            y,
            modifiers: self.modifiers,
        });
        redraw |= effect.redraw;
        self.apply_effect(effect);
        redraw
    }

    fn pointer_down(&mut self) -> bool {
        let (x, y) = self.cursor;
        match self.frisket.hit(x, y) {
            Some(FrisketHit::ChromeAction(action)) => self.apply_chrome_action(action),
            Some(FrisketHit::Close(tile)) => {
                self.clear_chrome_address();
                self.clear_chrome_engine_menu();
                self.clear_chrome_appearance();
                self.apply_tile_event(TileEvent::Closed(tile))
            },
            Some(FrisketHit::Divider { target, split_rect }) => {
                self.clear_chrome_address();
                self.clear_chrome_engine_menu();
                self.clear_chrome_appearance();
                let Some(fractions) = self.workspace.tree().fractions_at(&target.path) else {
                    return false;
                };
                if target.index + 1 >= fractions.len() {
                    return false;
                }
                let index = target.index;
                let horizontal =
                    self.workspace.tree().axis_at(&target.path) == Some(SplitAxis::Row);
                self.gesture = Some(PointerGesture::Divider(DividerDrag {
                    target,
                    horizontal,
                    extent: if horizontal {
                        split_rect.width
                    } else {
                        split_rect.height
                    },
                    start: if horizontal { x } else { y },
                    init_first: fractions[index],
                    pair_total: fractions[index] + fractions[index + 1],
                }));
                true
            },
            Some(FrisketHit::Tab(tile)) => {
                self.clear_chrome_address();
                self.clear_chrome_engine_menu();
                self.clear_chrome_appearance();
                self.gesture = Some(PointerGesture::Tab(TabDrag {
                    tile,
                    start: self.cursor,
                    moved: false,
                }));
                true
            },
            Some(FrisketHit::Content(_)) => {
                self.clear_chrome_address();
                self.clear_chrome_engine_menu();
                self.clear_chrome_appearance();
                self.gesture = Some(PointerGesture::Content);
                let effect = self.workspace.input(SessionInput::PointerButton {
                    x,
                    y,
                    button: SessionPointerButton::Primary,
                    state: SessionButtonState::Pressed,
                    modifiers: self.modifiers,
                });
                let redraw = effect.redraw;
                self.apply_effect(effect);
                redraw
            },
            Some(FrisketHit::Chrome) => {
                self.clear_chrome_address();
                let changed =
                    self.chrome_engine_menu.take().is_some() || self.chrome_appearance_open;
                self.clear_chrome_appearance();
                changed
            },
            Some(FrisketHit::Appearance) => true,
            None => false,
        }
    }

    fn pointer_up(&mut self) -> bool {
        let gesture = self.gesture.take();
        match gesture {
            Some(PointerGesture::Divider(_)) => true,
            Some(PointerGesture::Tab(drag)) if drag.moved => {
                let to = self.resolve_drop(drag.tile);
                to.is_some_and(|to| {
                    self.apply_tile_event(TileEvent::Dragged {
                        tile: drag.tile,
                        to,
                    })
                })
            },
            Some(PointerGesture::Tab(drag)) => {
                self.apply_tile_event(TileEvent::Activated(drag.tile))
            },
            Some(PointerGesture::Content) => {
                let (x, y) = self.cursor;
                let effect = self.workspace.input(SessionInput::PointerButton {
                    x,
                    y,
                    button: SessionPointerButton::Primary,
                    state: SessionButtonState::Released,
                    modifiers: self.modifiers,
                });
                let redraw = effect.redraw;
                self.apply_effect(effect);
                redraw
            },
            None => false,
        }
    }

    fn resolve_drop(&self, dragged: TileId) -> Option<DropTarget> {
        let (x, y) = self.cursor;
        if let Some((stack, index)) = self.frisket.tabbar_drop(x, y) {
            return Some(DropTarget::Stack { stack, index });
        }
        let target = self.workspace.tree().tiles().into_iter().find_map(|tile| {
            self.workspace
                .content_rect(tile.id)
                .filter(|rect| rect.contains(x, y))
                .map(|rect| (tile.id, rect))
        });
        match target {
            Some((tile, _)) if tile == dragged => None,
            Some((tile, rect)) => Some(DropTarget::Edge {
                tile,
                edge: nearest_edge((x, y), rect),
            }),
            None => Some(DropTarget::Outside),
        }
    }

    fn drive_receipt_step(&mut self) -> Result<(), String> {
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 4)?;
                let tile1 = TileId(1);
                let first = self.workspace.command_for(
                    tile1,
                    SessionNavigationCommand::Address("next.html".to_owned()),
                );
                if !first.navigated {
                    return Err(format!(
                        "P3 receipt could not navigate tile 1: {:?}",
                        first.error
                    ));
                }
                self.apply_effect(first);
                if let Some(rect) = self.workspace.content_rect(tile1) {
                    let _ = self.workspace.scroll_at(
                        rect.x + rect.width / 2.0,
                        rect.y + rect.height / 2.0,
                        0.0,
                        120.0,
                    );
                }
            },
            1 => {
                self.click_tab(TileId(2))?;
            },
            2 => {
                self.click_tab(TileId(1))?;
                let controller = self
                    .workspace
                    .controller(TileId(1))
                    .ok_or("tile 1 closed")?;
                if !controller.can_go_back() || !controller.address().ends_with("next.html") {
                    return Err("tile 1 lost history across tab activation".to_owned());
                }
            },
            3 => {
                self.click_tab(TileId(2))?;
            },
            4 => {
                let target = cambium::DividerTarget {
                    path: genet_host_api::tile::TilePath(Vec::new()),
                    index: 0,
                };
                let rect = self
                    .frisket
                    .divider_rect(&target)
                    .ok_or("root divider has no Frisket geometry")?;
                let start = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                self.pointer_move(start.0, start.1);
                self.pointer_down();
                self.pointer_move(start.0 + 60.0, start.1);
                self.pointer_up();
                let fractions = self
                    .workspace
                    .tree()
                    .fractions_at(&target.path)
                    .ok_or("root divider disappeared after resize")?;
                if (fractions[0] - 0.5).abs() < 0.01 {
                    return Err("root divider drag did not change its fractions".to_owned());
                }
            },
            5 => {
                let tab = self
                    .frisket
                    .tab_rect(TileId(2))
                    .ok_or("tile 2 tab has no Frisket geometry")?;
                let target = self
                    .workspace
                    .content_rect(TileId(4))
                    .ok_or("tile 4 content has no Frisket geometry")?;
                self.pointer_move(tab.x + 8.0, tab.y + tab.height / 2.0);
                self.pointer_down();
                self.pointer_move(
                    target.x + target.width * 0.9,
                    target.y + target.height / 2.0,
                );
                self.pointer_up();
            },
            6 => {
                let fourth = self.workspace.command_for(
                    TileId(4),
                    SessionNavigationCommand::Address("next.html".to_owned()),
                );
                if !fourth.navigated {
                    return Err(format!(
                        "P3 receipt could not navigate tile 4: {:?}",
                        fourth.error
                    ));
                }
                self.apply_effect(fourth);
            },
            7 => {
                let close = self
                    .frisket
                    .close_rect(TileId(3))
                    .ok_or("tile 3 close control has no Frisket geometry")?;
                self.pointer_move(close.x + close.width / 2.0, close.y + close.height / 2.0);
                self.pointer_down();
                if self.workspace.controller(TileId(3)).is_some() {
                    return Err("tile 3 remained live after its close control".to_owned());
                }
                if !self
                    .workspace
                    .controller(TileId(1))
                    .is_some_and(PeltController::can_go_back)
                    || !self
                        .workspace
                        .controller(TileId(4))
                        .is_some_and(PeltController::can_go_back)
                {
                    return Err("independent tile histories did not survive P3 gestures".to_owned());
                }
                self.receipt_complete = true;
            },
            _ => self.receipt_complete = true,
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(())
    }

    fn click_tab(&mut self, tile: TileId) -> Result<(), String> {
        let rect = self
            .frisket
            .tab_rect(tile)
            .ok_or_else(|| format!("tile {} tab has no Frisket geometry", tile.0))?;
        self.pointer_move(rect.x + 8.0, rect.y + rect.height / 2.0);
        self.pointer_down();
        self.pointer_up();
        Ok(())
    }

    fn click_chrome(&mut self, action: &str) -> Result<(), String> {
        let rect = self
            .frisket
            .chrome_rect(action)
            .ok_or_else(|| format!("chrome control {action:?} has no retained geometry"))?;
        self.pointer_move(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        if !self.pointer_down() {
            return Err(format!(
                "chrome control {action:?} did not handle its pointer press"
            ));
        }
        let _ = self.pointer_up();
        Ok(())
    }

    fn click_chrome_physical(&mut self, action: &str) -> Result<(), String> {
        let rect = self
            .frisket
            .chrome_rect(action)
            .ok_or_else(|| format!("chrome control {action:?} has no retained geometry"))?;
        let physical = (
            (rect.x + rect.width / 2.0) * self.scale_factor,
            (rect.y + rect.height / 2.0) * self.scale_factor,
        );
        self.pointer_move_physical(physical.0, physical.1);
        let intended = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        if (self.cursor.0 - intended.0).abs() > 0.01 || (self.cursor.1 - intended.1).abs() > 0.01 {
            return Err(format!(
                "physical pointer conversion missed Chrome control {action:?}: expected {intended:?}, got {:?}",
                self.cursor
            ));
        }
        if !self.pointer_down() {
            return Err(format!(
                "physical pointer input did not handle Chrome control {action:?}"
            ));
        }
        let _ = self.pointer_up();
        Ok(())
    }
}

fn a11y_node(tree: &TreeUpdate, label: &str, role: Role) -> Result<AccessNodeId, String> {
    tree.nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(label))
        .map(|(id, _)| *id)
        .ok_or_else(|| format!("accessibility tree has no {role:?} named {label:?}"))
}

fn discard_unimported_surface_frame(frame: SurfaceFrame) {
    #[cfg(target_os = "windows")]
    if let NativeTextureHandle::D3d12Shared {
        handle,
        ownership: FrameHandleOwnership::Transferred,
    } = frame.texture
    {
        // SAFETY: Inker transferred this one-shot Win32 handle to the host,
        // and this rejection path consumes the frame without importing it.
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(handle as _);
        }
    }

    #[cfg(not(target_os = "windows"))]
    let _ = frame;
}

impl ApplicationHandler for WorkspaceApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attributes =
            static_viewer::pelt_window_attributes(self.window_title(), self.width, self.height)
                .with_visible(false);
        if let Some((width, height)) = self
            .config
            .workspace_receipt
            .and_then(WorkspaceReceipt::logical_viewport)
        {
            attributes = attributes.with_inner_size(winit::dpi::LogicalSize::new(
                f64::from(width),
                f64::from(height),
            ));
        }
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.receipt_error =
                    Some(format!("could not create Pelt workspace window: {error}"));
                event_loop.exit();
                return;
            },
        };
        let size = window.inner_size();
        self.width = size.width.max(1);
        self.height = size.height.max(1);
        self.scale_factor = window.scale_factor() as f32;
        self.window = Some(Arc::clone(&window));
        let mut options = NetrenderOptions {
            tile_cache_size: Some(64),
            enable_vello: true,
            ..Default::default()
        };
        #[cfg(target_os = "windows")]
        if self
            .workspace
            .routes()
            .any(|route| matches!(route.state, PeltRouteState::Surface))
        {
            options.backends = Some(wgpu::Backends::DX12);
        }
        match SurfaceHost::boot(window.clone(), self.width, self.height, options) {
            Ok(host) => {
                #[cfg(target_os = "windows")]
                if self
                    .workspace
                    .routes()
                    .any(|route| matches!(route.state, PeltRouteState::Surface))
                    && let Some(scrying_host) = &self.scrying_host
                {
                    let hwnd = match window.window_handle().map(|handle| handle.as_raw()) {
                        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as usize,
                        Ok(other) => {
                            self.receipt_error = Some(format!(
                                "Pelt expected a Win32 window handle, got {other:?}"
                            ));
                            event_loop.exit();
                            return;
                        },
                        Err(error) => {
                            self.receipt_error =
                                Some(format!("could not borrow Pelt's Win32 handle: {error}"));
                            event_loop.exit();
                            return;
                        },
                    };
                    if let Err(error) = scrying_host.install(hwnd, host.device()) {
                        self.receipt_error = Some(error);
                        event_loop.exit();
                        return;
                    }
                }
                self.host = Some(host);
            },
            Err(error) => {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            },
        }
        if let Err(error) = self.install_accessibility_before_show() {
            self.receipt_error = Some(error);
            event_loop.exit();
            return;
        }
        self.workspace_receipt_stage_started = Instant::now();
        window.request_redraw();
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.accessibility.take_wake() {
            self.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.width = size.width.max(1);
                self.height = size.height.max(1);
                if let Some(host) = self.host.as_mut() {
                    host.resize(self.width, self.height);
                }
                self.request_redraw();
            },
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.width = size.width.max(1);
                    self.height = size.height.max(1);
                }
                if let Some(host) = self.host.as_mut() {
                    host.resize(self.width, self.height);
                }
                self.request_redraw();
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = SessionModifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    meta: state.super_key(),
                };
            },
            WindowEvent::CursorMoved { position, .. } => {
                let redraw = self.pointer_move_physical(position.x as f32, position.y as f32);
                if redraw {
                    self.request_redraw();
                }
            },
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let redraw = match state {
                    ElementState::Pressed => self.pointer_down(),
                    ElementState::Released => self.pointer_up(),
                };
                if redraw {
                    self.request_redraw();
                }
            },
            WindowEvent::MouseWheel { delta, .. } => {
                if matches!(
                    self.frisket.hit(self.cursor.0, self.cursor.1),
                    Some(FrisketHit::Appearance | FrisketHit::ChromeAction(_))
                ) {
                    return;
                }
                let (dx, dy) = wheel_delta_from_winit(delta);
                if self.workspace.scroll_at(
                    self.cursor.0,
                    self.cursor.1,
                    dx / self.scale_factor,
                    dy / self.scale_factor,
                ) {
                    self.request_redraw();
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if self.handle_chrome_key(&event.logical_key, event.state) {
                    self.request_redraw();
                    return;
                }
                let navigation =
                    navigation_command(&event.logical_key, event.state, self.modifiers);
                if let Some(command) = navigation {
                    let effect = self.workspace.command(command);
                    self.apply_effect(effect);
                    return;
                }
                let effect = self.workspace.input(SessionInput::Key {
                    key: session_key(&event.logical_key),
                    state: button_state(event.state),
                    modifiers: self.modifiers,
                    repeat: event.repeat,
                });
                let handled = effect.handled;
                let editable = effect.editable;
                self.apply_effect(effect);
                if event.state == ElementState::Pressed
                    && !handled
                    && !editable
                    && let Some(key) = scroll_key(&event.logical_key, self.modifiers.shift)
                    && self.workspace.scroll_for_key(key)
                {
                    self.request_redraw();
                }
            },
            WindowEvent::Ime(ime) => {
                if self.handle_chrome_ime(&ime) {
                    self.request_redraw();
                    return;
                }
                let effect = self.workspace.input(SessionInput::Ime(session_ime(ime)));
                self.apply_effect(effect);
            },
            WindowEvent::Focused(focused) => {
                self.accessibility.update_window_focus(focused);
                let effect = self.workspace.input(SessionInput::Focus(focused));
                self.apply_effect(effect);
            },
            WindowEvent::RedrawRequested => self.render(event_loop),
            _ => {},
        }
    }
}

fn require_tile(tree: &TileTree, count: usize) -> Result<(), String> {
    if tree.tiles().len() >= count {
        Ok(())
    } else {
        Err(format!(
            "P3 interaction receipt needs at least {count} document URLs"
        ))
    }
}

fn nearest_edge(point: (f32, f32), rect: WorkspaceRect) -> Edge {
    let x = if rect.width > 0.0 {
        (point.0 - rect.x) / rect.width
    } else {
        0.5
    };
    let y = if rect.height > 0.0 {
        (point.1 - rect.y) / rect.height
    } else {
        0.5
    };
    let distances = [x, 1.0 - x, y, 1.0 - y];
    let index = distances
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map_or(0, |(index, _)| index);
    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom][index]
}

fn physical_extent(logical: f32, scale_factor: f32) -> u32 {
    ((logical.max(1.0) * scale_factor.max(1.0)).round() as u32).max(1)
}

fn rect_fits_viewport(rect: WorkspaceRect, viewport: (u32, u32)) -> bool {
    rect.width > 0.0
        && rect.height > 0.0
        && rect.x >= 0.0
        && rect.y >= 0.0
        && rect.x + rect.width <= viewport.0 as f32
        && rect.y + rect.height <= viewport.1 as f32
}

fn placement(rect: WorkspaceRect, scale_factor: f32) -> ExternalTexturePlacement {
    ExternalTexturePlacement::new([
        rect.x * scale_factor,
        rect.y * scale_factor,
        (rect.x + rect.width) * scale_factor,
        (rect.y + rect.height) * scale_factor,
    ])
}

fn fragment_placement(
    rect: WorkspaceRect,
    viewport: (u32, u32),
    scale_factor: f32,
) -> ExternalTexturePlacement {
    let placement = placement(rect, scale_factor);
    let [x0, y0, x1, y1] = placement.dest_rect;
    let (width, height) = (viewport.0.max(1) as f32, viewport.1.max(1) as f32);
    placement.with_uv([
        (x0 / width).clamp(0.0, 1.0),
        (y0 / height).clamp(0.0, 1.0),
        (x1 / width).clamp(0.0, 1.0),
        (y1 / height).clamp(0.0, 1.0),
    ])
}

fn button_state(state: ElementState) -> SessionButtonState {
    match state {
        ElementState::Pressed => SessionButtonState::Pressed,
        ElementState::Released => SessionButtonState::Released,
    }
}

fn session_key(key: &Key) -> SessionKey {
    match key {
        Key::Character(text) => SessionKey::Character(text.to_string()),
        Key::Named(NamedKey::Enter) => SessionKey::Enter,
        Key::Named(NamedKey::Tab) => SessionKey::Tab,
        Key::Named(NamedKey::Backspace) => SessionKey::Backspace,
        Key::Named(NamedKey::Delete) => SessionKey::Delete,
        Key::Named(NamedKey::Escape) => SessionKey::Escape,
        Key::Named(NamedKey::Space) => SessionKey::Space,
        Key::Named(NamedKey::ArrowLeft) => SessionKey::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => SessionKey::ArrowRight,
        Key::Named(NamedKey::ArrowUp) => SessionKey::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => SessionKey::ArrowDown,
        Key::Named(NamedKey::Home) => SessionKey::Home,
        Key::Named(NamedKey::End) => SessionKey::End,
        Key::Named(NamedKey::PageUp) => SessionKey::PageUp,
        Key::Named(NamedKey::PageDown) => SessionKey::PageDown,
        _ => SessionKey::Unidentified,
    }
}

fn scroll_key(key: &Key, shift: bool) -> Option<SessionScrollKey> {
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => SessionScrollKey::LineUp,
        Key::Named(NamedKey::ArrowDown) => SessionScrollKey::LineDown,
        Key::Named(NamedKey::PageUp) => SessionScrollKey::PageUp,
        Key::Named(NamedKey::PageDown) => SessionScrollKey::PageDown,
        Key::Named(NamedKey::Home) => SessionScrollKey::Home,
        Key::Named(NamedKey::End) => SessionScrollKey::End,
        Key::Named(NamedKey::Space) if shift => SessionScrollKey::PageUp,
        Key::Named(NamedKey::Space) => SessionScrollKey::PageDown,
        _ => return None,
    })
}

fn navigation_command(
    key: &Key,
    state: ElementState,
    modifiers: SessionModifiers,
) -> Option<SessionNavigationCommand> {
    if state != ElementState::Pressed {
        return None;
    }
    match key {
        Key::Named(NamedKey::BrowserBack) => Some(SessionNavigationCommand::Back),
        Key::Named(NamedKey::BrowserForward) => Some(SessionNavigationCommand::Forward),
        Key::Named(NamedKey::BrowserRefresh) | Key::Named(NamedKey::F5) => {
            Some(SessionNavigationCommand::Reload)
        },
        Key::Named(NamedKey::ArrowLeft) if modifiers.alt => Some(SessionNavigationCommand::Back),
        Key::Named(NamedKey::ArrowRight) if modifiers.alt => {
            Some(SessionNavigationCommand::Forward)
        },
        Key::Character(text)
            if (modifiers.control || modifiers.meta) && text.eq_ignore_ascii_case("r") =>
        {
            Some(SessionNavigationCommand::Reload)
        },
        _ => None,
    }
}

fn session_ime(ime: winit::event::Ime) -> SessionIme {
    match ime {
        winit::event::Ime::Enabled => SessionIme::Enabled,
        winit::event::Ime::Preedit(text, selection) => SessionIme::Preedit { text, selection },
        winit::event::Ime::Commit(text) => SessionIme::Commit(text),
        winit::event::Ime::Disabled => SessionIme::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_capability_blocks_even_an_accidental_structural_report() {
        let inspector = inspector_snapshot(
            "opaque.test".to_owned(),
            false,
            Some(PeltTileInspection {
                capability: inker::A11yCapability::Opaque,
                report: Some(inker::ContentReport {
                    title: Some("Must not leak".to_owned()),
                    headings: vec!["Also must not leak".to_owned()],
                    ..Default::default()
                }),
            }),
        );
        assert_eq!(inspector.status, "Opaque document");
        assert_eq!(inspector.title.as_deref(), Some("opaque.test"));
        assert_eq!(
            inspector.summary,
            "Contents not inspectable for this document."
        );
        assert!(inspector.sections.is_empty());
    }

    #[test]
    fn four_urls_build_tabs_beside_a_nested_split() {
        let urls = (1..=4)
            .map(|id| format!("tile-{id}.html"))
            .collect::<Vec<_>>();
        let tree = tree_from_urls(&urls);
        let TileTree::Split { axis, children } = tree else {
            panic!("root split");
        };
        assert_eq!(axis, SplitAxis::Row);
        assert_eq!(children.len(), 2);
        assert!(matches!(&children[0].tree, TileTree::Stack(stack) if stack.tabs.len() == 2));
        assert!(matches!(
            &children[1].tree,
            TileTree::Split {
                axis: SplitAxis::Column,
                ..
            }
        ));
    }

    #[test]
    fn nearest_edge_uses_the_content_hole_geometry() {
        let rect = WorkspaceRect::new(100.0, 100.0, 200.0, 100.0);
        assert_eq!(nearest_edge((105.0, 140.0), rect), Edge::Left);
        assert_eq!(nearest_edge((295.0, 140.0), rect), Edge::Right);
        assert_eq!(nearest_edge((200.0, 102.0), rect), Edge::Top);
        assert_eq!(nearest_edge((200.0, 198.0), rect), Edge::Bottom);
    }

    #[test]
    fn inspector_overlay_reuses_the_matching_physical_frame_crop() {
        let placement = fragment_placement(
            WorkspaceRect::new(392.0, 70.0, 248.0, 300.0),
            (960, 640),
            1.5,
        );
        assert_eq!(placement.dest_rect, [588.0, 105.0, 960.0, 555.0]);
        assert_eq!(
            placement.uv,
            [0.612_5, 0.164_062_5, 1.0, 0.867_187_5],
            "the source crop stays aligned with the scaled destination"
        );
    }

    #[test]
    fn receipt_tree_exposes_every_tab_to_frisket_geometry() {
        let urls = [
            "a/index.html",
            "b/index.html",
            "c/index.html",
            "d/index.html",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let tree = tree_from_urls(&urls);
        let mut surface = FrisketSurface::new(&tree);
        surface.frame(1000, 700).unwrap();
        assert!(surface.tab_rect(TileId(1)).is_some());
        assert!(surface.tab_rect(TileId(2)).is_some());
        assert!(surface.tab_rect(TileId(3)).is_some());
        assert!(surface.tab_rect(TileId(4)).is_some());
    }

    #[test]
    fn named_workspace_receipts_pin_their_capture_contract() {
        let config =
            WorkspaceViewerConfig::new(vec!["fallback.html".to_owned()], WindowingMode::Headed)
                .with_workspace_receipt(WorkspaceReceipt::Fallback, "receipt.png");
        assert_eq!(config.workspace_receipt, Some(WorkspaceReceipt::Fallback));
        assert_eq!(config.size, Some((960, 640)));
        assert_eq!(config.frames, Some(3));
        assert_eq!(config.artifact.as_deref(), Some(Path::new("receipt.png")));
        let caller_geometry = config.with_size(800, 500).with_frame_limit(5);
        assert_eq!(caller_geometry.size, Some((800, 500)));
        assert_eq!(caller_geometry.frames, Some(5));

        let narrow =
            WorkspaceViewerConfig::new(vec!["narrow.html".to_owned()], WindowingMode::Headed)
                .with_workspace_receipt(WorkspaceReceipt::NarrowChrome, "narrow.png");
        assert_eq!(narrow.size, Some((360, 480)));
        assert_eq!(
            WorkspaceReceipt::NarrowChrome.logical_viewport(),
            Some((360, 480))
        );
        let dpi = WorkspaceViewerConfig::new(vec!["dpi.html".to_owned()], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::ChromeDpi, "dpi.png");
        assert_eq!(dpi.size, Some((960, 640)));
        assert_eq!(
            WorkspaceReceipt::ChromeDpi.logical_viewport(),
            Some((960, 640))
        );
    }

    #[test]
    fn named_workspace_receipts_reject_the_older_receipt_drivers() {
        let fallback = || {
            WorkspaceViewerConfig::new(vec!["fallback.html".to_owned()], WindowingMode::Headless)
                .with_workspace_receipt(WorkspaceReceipt::Fallback, "receipt.png")
        };
        let p3 = run_livery_workspace_viewer(fallback().with_interaction_receipt())
            .expect_err("P3 and P5 receipt drivers are mutually exclusive");
        assert!(p3.contains("P3 or P4"));
        let p4 = run_livery_workspace_viewer(fallback().with_capability_receipt())
            .expect_err("P4 and P5 receipt drivers are mutually exclusive");
        assert!(p4.contains("P3 or P4"));
    }

    #[test]
    fn fallback_receipt_keeps_held_html_in_livery_without_scrying() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p5-fallback/index.html")
            .to_string_lossy()
            .into_owned();
        let tree = tree_from_urls(&[fixture.clone()]);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let mut workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("receipt tree contains only documents");
                };
                Ok(PeltTileRequest::new(address, (960, 640)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("fallback receipt routes through the owned document engine");
        let initial = workspace.route(TileId(1)).expect("ordinary Livery route");
        assert_eq!(
            initial.selected_engine(),
            inker::routing::ENGINE_GENET_LIVERY
        );
        assert!(matches!(initial.state, PeltRouteState::Document));
        let mut frisket = FrisketSurface::new(workspace.tree());
        let pane = frisket.frame(960, 640).expect("fallback Frisket frame");
        workspace.set_content_rects(pane.content_rects);
        let _ = workspace.frame();
        let config = WorkspaceViewerConfig::new(vec![fixture], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::Fallback, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        assert_eq!(
            app.drive_fallback_workspace_receipt_step()
                .expect("fallback policy step"),
            None
        );
        let pane = app
            .frisket
            .frame(960, 640)
            .expect("fallback Frisket frame after route selection");
        app.workspace.set_content_rects(pane.content_rects);
        let _ = app.workspace.frame();
        let assertion = app
            .drive_fallback_workspace_receipt_step()
            .expect("fallback interaction step")
            .expect("fallback receipt completes after navigation");
        assert_eq!(
            assertion,
            "explicit Scrying pin fell back visibly to Livery; retained link navigation stayed interactive"
        );
        let route = app.workspace.route(TileId(1)).expect("fallback route");
        assert_eq!(route.selected_engine(), inker::routing::ENGINE_SCRYING_WEB);
        assert!(matches!(
            route.state,
            PeltRouteState::Fallback {
                ref active_engine,
                reason: ref actual_reason,
            } if active_engine == inker::routing::ENGINE_GENET_LIVERY
                && actual_reason == "surface engine is not registered on this host"
        ));
        let report = app
            .workspace
            .controller(TileId(1))
            .and_then(PeltController::inspect)
            .expect("fallback keeps its navigated Livery controller");
        assert_eq!(report.title.as_deref(), Some("Pelt fallback destination"));
        assert!(
            report
                .headings
                .iter()
                .any(|heading| heading == "Fallback navigation stayed local")
        );
    }

    #[test]
    fn loading_error_receipt_projects_host_documents_and_recovers_to_ready() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p6-load-error/index.html")
            .to_string_lossy()
            .into_owned();
        let tree = tree_from_urls(&[fixture.clone()]);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("loading/error receipt tree contains only documents");
                };
                Ok(PeltTileRequest::new(address, (960, 640)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("loading/error receipt opens its seed document");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(vec![fixture.clone()], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::LoadingError, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        let compose = |app: &mut WorkspaceApp| {
            app.refresh_chrome();
            let mut pane = app
                .frisket
                .frame(960, 640)
                .expect("loading/error Frisket frame");
            app.workspace
                .set_content_rects(pane.content_rects.iter().copied());
            if app.config.chrome && app.chrome_model().diagnostic.is_some() {
                app.refresh_chrome();
                pane = app
                    .frisket
                    .frame(960, 640)
                    .expect("diagnostic Frisket frame");
                app.workspace
                    .set_content_rects(pane.content_rects.iter().copied());
            }
            app.last_chrome_document = (app.config.chrome && pane.diagnostic_rect.is_some())
                .then(|| app.chrome_model().diagnostic)
                .flatten();
            let _ = app.workspace.pump();
            let _ = app.workspace.frame();
            app.workspace.mark_visible_documents_presented();
        };

        compose(&mut app);
        let mut assertion = None;
        for _ in 0..8 {
            assertion = app
                .drive_loading_error_workspace_receipt_step()
                .expect("loading/error semantic receipt");
            compose(&mut app);
            if assertion.is_some() {
                break;
            }
        }
        assert_eq!(
            assertion.as_deref(),
            Some(LOADING_ERROR_WORKSPACE_ASSERTION)
        );
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("failed navigation retains its controller");
        assert!(
            controller
                .address()
                .replace('\\', "/")
                .ends_with("/p6-load-error/next.html")
        );
        assert!(controller.can_go_back());
        assert!(matches!(
            controller.document_state(),
            PeltDocumentState::Error { address, .. }
                if address.replace('\\', "/").ends_with("/p6-load-error/missing.html")
        ));

        let recovered = app
            .workspace
            .command(SessionNavigationCommand::Address(fixture));
        app.apply_effect(recovered);
        assert!(matches!(
            app.workspace
                .controller(TileId(1))
                .expect("recovered controller")
                .document_state(),
            PeltDocumentState::Loading { .. }
        ));
        compose(&mut app);
        assert!(matches!(
            app.last_chrome_document
                .as_ref()
                .map(|document| document.kind),
            Some(ChromeDocumentKind::Loading)
        ));
        compose(&mut app);
        assert_eq!(app.last_chrome_document, None);
        assert_eq!(
            app.workspace
                .controller(TileId(1))
                .expect("settled controller")
                .document_state(),
            &PeltDocumentState::Ready
        );
    }

    #[test]
    fn narrow_chrome_receipt_keeps_controls_tabs_and_failure_documents_usable() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p6-load-error/index.html")
            .to_string_lossy()
            .into_owned();
        let tree = tree_from_urls(&[fixture.clone()]);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("narrow Chrome receipt tree contains only documents");
                };
                Ok(PeltTileRequest::new(address, (360, 480)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("narrow Chrome receipt opens its seed document");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(vec![fixture], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::NarrowChrome, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        let compose = |app: &mut WorkspaceApp| {
            app.refresh_chrome();
            let mut pane = app
                .frisket
                .frame(360, 480)
                .expect("narrow Chrome Frisket frame");
            app.workspace
                .set_content_rects(pane.content_rects.iter().copied());
            if app.config.chrome && app.chrome_model().diagnostic.is_some() {
                app.refresh_chrome();
                pane = app
                    .frisket
                    .frame(360, 480)
                    .expect("narrow Chrome diagnostic frame");
                app.workspace
                    .set_content_rects(pane.content_rects.iter().copied());
            }
            app.last_chrome_document = (app.config.chrome && pane.diagnostic_rect.is_some())
                .then(|| app.chrome_model().diagnostic)
                .flatten();
            let _ = app.workspace.pump();
            let _ = app.workspace.frame();
            app.workspace.mark_visible_documents_presented();
        };

        assert_eq!(app.logical_size(), (360, 480));
        compose(&mut app);
        let mut assertion = None;
        for _ in 0..10 {
            assertion = app
                .drive_narrow_chrome_workspace_receipt_step()
                .expect("narrow Chrome semantic receipt");
            compose(&mut app);
            if assertion.is_some() {
                break;
            }
        }
        assert_eq!(
            assertion.as_deref(),
            Some(NARROW_CHROME_WORKSPACE_ASSERTION)
        );
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("narrow Chrome failure retains its controller");
        assert!(
            controller
                .address()
                .replace('\\', "/")
                .ends_with("/p6-load-error/next.html")
        );
        assert!(matches!(
            controller.document_state(),
            PeltDocumentState::Error { address, .. }
                if address.replace('\\', "/").ends_with("/p6-load-error/missing.html")
        ));
    }

    #[test]
    fn chrome_dpi_receipt_converts_physical_pointer_input_without_moving_content() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p6-appearance/index.html")
            .to_string_lossy()
            .into_owned();
        let tree = tree_from_urls(&[fixture.clone()]);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("Chrome DPI receipt tree contains only documents");
                };
                Ok(PeltTileRequest::new(address, (960, 640)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("Chrome DPI receipt opens its seed document");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(vec![fixture.clone()], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::ChromeDpi, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        app.scale_factor = 2.0;
        app.width = 1920;
        app.height = 1280;
        let compose = |app: &mut WorkspaceApp| {
            app.refresh_chrome();
            let pane = app
                .frisket
                .frame(960, 640)
                .expect("Chrome DPI Frisket frame");
            app.workspace
                .set_content_rects(pane.content_rects.iter().copied());
            let _ = app.workspace.pump();
            let _ = app.workspace.frame();
            app.workspace.mark_visible_documents_presented();
        };

        assert_eq!(app.logical_size(), (960, 640));
        compose(&mut app);
        let mut assertion = None;
        for _ in 0..5 {
            assertion = app
                .drive_chrome_dpi_workspace_receipt_step()
                .expect("Chrome DPI semantic receipt");
            compose(&mut app);
            if assertion.is_some() {
                break;
            }
        }
        assert!(
            assertion
                .as_deref()
                .is_some_and(|value| value.starts_with(CHROME_DPI_WORKSPACE_ASSERTION_PREFIX))
        );
        assert_eq!(app.chrome_theme, ChromeTheme::Light);
        assert!(app.chrome_appearance_open);
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("Chrome DPI receipt retains its controller");
        assert_eq!(controller.address(), fixture.as_str());
    }

    #[test]
    fn appearance_receipt_changes_session_theme_without_replacing_the_document() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p6-appearance/index.html")
            .to_string_lossy()
            .into_owned();
        let tree = tree_from_urls(&[fixture.clone()]);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("appearance receipt tree contains only documents");
                };
                Ok(PeltTileRequest::new(address, (960, 640)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("appearance receipt opens its seed document");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(vec![fixture.clone()], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::Appearance, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        let compose = |app: &mut WorkspaceApp| {
            app.refresh_chrome();
            let pane = app
                .frisket
                .frame(960, 640)
                .expect("appearance Frisket frame");
            app.workspace
                .set_content_rects(pane.content_rects.iter().copied());
            let _ = app.workspace.pump();
            let _ = app.workspace.frame();
            app.workspace.mark_visible_documents_presented();
        };

        compose(&mut app);
        let mut assertion = None;
        for _ in 0..6 {
            assertion = app
                .drive_appearance_workspace_receipt_step()
                .expect("appearance semantic receipt");
            compose(&mut app);
            if assertion.is_some() {
                break;
            }
        }
        assert_eq!(assertion.as_deref(), Some(APPEARANCE_WORKSPACE_ASSERTION));
        assert_eq!(app.chrome_theme, ChromeTheme::Light);
        assert!(app.chrome_appearance_open);
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("appearance receipt retains its controller");
        assert_eq!(controller.address(), fixture.as_str());

        assert!(app.apply_chrome_action(ChromeAction::ToggleAppearance));
        compose(&mut app);
        assert!(!app.chrome_appearance_open);
        let content = app
            .workspace
            .content_rect(TileId(1))
            .expect("appearance receipt keeps its content hole after dismissal");
        assert_eq!(
            app.frisket.hit(
                content.x + content.width / 2.0,
                content.y + content.height / 2.0
            ),
            Some(FrisketHit::Content(TileId(1)))
        );
    }

    #[test]
    fn accessibility_focus_and_click_route_through_the_retained_shell_separately() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p6-accessibility/index.html")
            .to_string_lossy()
            .into_owned();
        let tree = tree_from_urls(&[fixture.clone()]);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("accessibility receipt tree contains only documents");
                };
                Ok(PeltTileRequest::new(address, (960, 640)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("accessibility receipt opens its seed document");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(vec![fixture.clone()], WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::Accessibility, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        app.scale_factor = 1.25;
        let initial = app
            .prepare_accessibility_tree()
            .expect("initial retained accessibility tree");
        let root = initial
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Window)
            .map(|(_, node)| node)
            .expect("window root");
        assert_eq!(root.transform(), Some(&Affine::scale(1.25)));
        let theme = a11y_node(&initial, "Toggle session appearance settings", Role::Button)
            .expect("appearance toggle");
        assert!(app.apply_accessibility_request(A11yActionRequest {
            action: Action::Focus,
            target_node: theme,
        }));
        assert_eq!(app.chrome_theme, ChromeTheme::Dark);
        assert!(!app.chrome_appearance_open);
        assert!(app.accessibility.focus.is_some());

        assert!(app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: theme,
        }));
        assert!(app.chrome_appearance_open);
        let drawer = app
            .prepare_accessibility_tree()
            .expect("appearance accessibility tree");
        let light = a11y_node(&drawer, "Light", Role::RadioButton).expect("Light radio");
        assert!(app.apply_accessibility_request(A11yActionRequest {
            action: Action::Focus,
            target_node: light,
        }));
        assert_eq!(app.chrome_theme, ChromeTheme::Dark);

        assert!(app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: light,
        }));
        assert_eq!(app.chrome_theme, ChromeTheme::Light);
        let selected = app
            .prepare_accessibility_tree()
            .expect("selected appearance accessibility tree");
        let selected_light =
            a11y_node(&selected, "Light", Role::RadioButton).expect("selected Light radio");
        let light_node = selected
            .nodes
            .iter()
            .find(|(id, _)| *id == selected_light)
            .map(|(_, node)| node)
            .expect("selected Light radio node");
        assert_eq!(light_node.toggled(), Some(accesskit::Toggled::True));
        assert_eq!(selected.focus, selected_light);
        assert_eq!(
            app.accessibility.focus,
            Some(FrisketA11yTarget::ChromeAction(ChromeAction::ChooseTheme(
                ChromeTheme::Light
            )))
        );
        assert_eq!(
            app.workspace
                .controller(TileId(1))
                .expect("focused document survives chrome action")
                .address(),
            fixture
        );
    }

    #[test]
    fn appearance_overlay_reuses_the_matching_physical_frame_crop() {
        let placement = fragment_placement(
            WorkspaceRect::new(380.0, 70.0, 260.0, 300.0),
            (960, 640),
            1.5,
        );
        assert_eq!(placement.dest_rect, [570.0, 105.0, 960.0, 555.0]);
        assert_eq!(placement.uv, [0.593_75, 0.164_062_5, 1.0, 0.867_187_5]);
    }

    #[cfg(all(feature = "scripted", feature = "smolweb"))]
    #[test]
    fn mixed_receipt_reroutes_only_the_clicked_gemtext_tile() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p4");
        let urls = ["native.gmi", "static.html", "scripted.html", "surface.html"]
            .into_iter()
            .map(|name| root.join(name).to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let tree = tree_from_urls(&urls);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let overrides = HashMap::from([
            (1, inker::routing::ENGINE_NEMATIC_GEMTEXT.to_owned()),
            (2, inker::routing::ENGINE_GENET_LIVERY.to_owned()),
            (3, inker::routing::ENGINE_GENET_SCRIPTED.to_owned()),
            (4, inker::routing::ENGINE_SCRYING_WEB.to_owned()),
        ]);
        let mut workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("mixed receipt tree contains only documents");
                };
                let engine = overrides
                    .get(&tile.id.0)
                    .expect("every mixed receipt tile has a pinned engine");
                Ok(PeltTileRequest::new(address, (960, 640)).with_engine_override(engine.clone()))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("mixed receipt routes all four capabilities");
        let mut frisket = FrisketSurface::new(workspace.tree());
        let pane = frisket.frame(960, 640).expect("mixed Frisket frame");
        workspace.set_content_rects(pane.content_rects);
        let _ = workspace.pump();
        let _ = workspace.frame();
        let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::Mixed, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        let assertion = app
            .drive_mixed_workspace_receipt_step()
            .expect("mixed semantic receipt")
            .expect("GPU-free fallback needs no native import wait");
        assert_eq!(assertion, MIXED_WORKSPACE_ASSERTION);
    }

    #[cfg(all(feature = "scripted", feature = "smolweb"))]
    #[test]
    fn chrome_receipt_controls_one_focused_tile_without_disturbing_neighbors() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("pelt desktop has a parent")
            .join("examples/workspace/p4");
        let urls = ["native.gmi", "static.html", "scripted.html", "surface.html"]
            .into_iter()
            .map(|name| root.join(name).to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let tree = tree_from_urls(&urls);
        #[cfg(target_os = "windows")]
        let registries = workspace_registries(None);
        #[cfg(not(target_os = "windows"))]
        let registries = workspace_registries();
        let overrides = HashMap::from([
            (1, inker::routing::ENGINE_NEMATIC_GEMTEXT.to_owned()),
            (3, inker::routing::ENGINE_GENET_SCRIPTED.to_owned()),
            (4, inker::routing::ENGINE_SCRYING_WEB.to_owned()),
        ]);
        let workspace = PeltWorkspace::try_routed(
            tree,
            registries,
            |tile| {
                let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                    unreachable!("chrome receipt tree contains only documents");
                };
                let request = PeltTileRequest::new(address, (960, 640));
                Ok(overrides.get(&tile.id.0).map_or(request.clone(), |engine| {
                    request.with_engine_override(engine.clone())
                }))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("chrome receipt routes all four capabilities");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
            .with_workspace_receipt(WorkspaceReceipt::Chrome, "unused.png");
        #[cfg(target_os = "windows")]
        let mut app = WorkspaceApp::new(config, workspace, frisket, None);
        #[cfg(not(target_os = "windows"))]
        let mut app = WorkspaceApp::new(config, workspace, frisket);

        for _ in 0..24 {
            app.refresh_chrome();
            let pane = app.frisket.frame(960, 640).expect("chrome Frisket frame");
            app.workspace.set_content_rects(pane.content_rects);
            let _ = app.workspace.pump();
            let _ = app.workspace.frame();
            if let Some(assertion) = app
                .drive_chrome_workspace_receipt_step()
                .expect("chrome semantic receipt")
            {
                assert_eq!(assertion, CHROME_WORKSPACE_ASSERTION);
                return;
            }
        }
        panic!("chrome receipt did not complete its bounded interaction sequence");
    }
}
