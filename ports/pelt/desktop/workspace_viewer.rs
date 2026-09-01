//! Headed recursive Pelt workspace over TileTree and Frisket.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use accesskit::{
    Action, ActionData, Affine, Node as AccessNode, NodeId as AccessNodeId, Rect as AccessRect,
    Role, TreeUpdate,
};
use genet_host_api::ResourceFetcher;
use genet_host_api::settings::{SettingValue, SettingsProvider};
use genet_winit_host::{
    A11yActionRequest, AccessKitBridge, BridgeStatus, RenderCore, SurfaceHost, WindowSurface,
    wheel_delta_from_winit,
};
use inker::{
    A11yCapability, EngineProfileBinding, SessionButtonState, SessionCursor, SessionIme,
    SessionInput, SessionKey, SessionModifiers, SessionNavigationCommand, SessionPointerButton,
    SessionRegistry, SessionScrollKey, SessionSpawnRequest, SurfaceEngineRegistry, SurfaceFrame,
};
#[cfg(target_os = "windows")]
use inker::{FrameHandleOwnership, NativeTextureHandle};
use netrender::external_texture::ExternalTexturePlacement;
use netrender::{ColorLoad, NetrenderOptions, Scene};
use pelt_core::{
    PeltController, PeltDocumentState, PeltHostEffect, PeltRegistries, PeltRouteSource,
    PeltRouteState, PeltTileInspection, PeltTileRequest, PeltWorkspace, PeltWorkspaceFrame,
    WorkspaceRect,
};
#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};
use workbench::{
    ContentSource, DocumentRef, DropTarget, Edge, SettingsRef, SplitAxis, Tile, TileBranch,
    TileEvent, TileId, TileTree, WorkbenchEffect,
};

use crate::appearance::{
    APPEARANCE_REFERENCE, AppearanceSettingsProvider, AppearanceStore, AppearanceTheme,
    CHROME_THEME_SETTING, InMemoryAppearanceStore,
};
#[cfg(target_os = "windows")]
use crate::dx12_surface::Dx12SurfaceCache;
use crate::frisket_surface::{
    ChromeAction, ChromeAppearance, ChromeDocument, ChromeDocumentKind, ChromeEngineChoice,
    ChromeInspector, ChromeInspectorSection, FrisketA11yProjection, FrisketA11yTarget,
    FrisketContentA11y, FrisketHit, FrisketSurface, WorkspaceChrome,
};
#[cfg(target_os = "windows")]
use crate::scrying_receipt::{ScryingReceiptEngine, ScryingReceiptHost};
use crate::{WindowingMode, static_viewer};

mod accessibility;
mod receipts;
#[cfg(test)]
mod tests;

const RECEIPT_STEPS: u8 = 8;
const WORKSPACE_RECEIPT_STAGE_TIMEOUT: Duration = Duration::from_secs(20);
const MIXED_WORKSPACE_ASSERTION: &str =
    "Gemtext navigation rerouted only tile 1; Livery, scripted, and external neighbors held";
const CHROME_WORKSPACE_ASSERTION: &str = "focused-tile chrome navigated history, bound an explicit engine choice menu, applied a per-tile override, and exposed truthful structural inspection while the mixed workspace held";
const LOADING_ERROR_WORKSPACE_ASSERTION: &str =
    "host-owned loading and error documents preserved the focused tile's prior session and history";
const APPEARANCE_WORKSPACE_ASSERTION: &str =
    "Pelt-owned appearance changed the live chrome theme while the focused document held";
const ACCESSIBILITY_WORKSPACE_ASSERTION: &str = "AccessKit installed before the Pelt window became visible; typed Focus held state while Click opened and selected the Pelt appearance controls";
const ACCESSIBILITY_ADDRESS_WORKSPACE_ASSERTION: &str = "installed AccessKit address SetValue routing navigated only the focused Pelt tile through loading and retained error content while preserving the successful address and history";
const ACCESSIBILITY_CHILDREN_WORKSPACE_ASSERTION: &str = "Pelt composed the focused Livery child tree through its retained content hole; Focus stayed virtual and Click navigated only that session";
const ACCESSIBILITY_EDIT_WORKSPACE_ASSERTION: &str = "Pelt routed a Livery text SetValue through its live child namespace, reprojected the value, and submitted only the focused tile";
const ACCESSIBILITY_SCROLL_WORKSPACE_ASSERTION: &str = "Pelt routed Livery ScrollIntoView through the focused Livery nested scrollport and preserved the sibling tile";
const ACCESSIBILITY_CLICK_WORKSPACE_ASSERTION: &str = "Pelt revealed a nested Livery target, routed its clip-aware Click through ordinary pointer input, and rejected stale tile actions";
const ACCESSIBILITY_INPUT_WORKSPACE_ASSERTION: &str = "Pelt preserved a nested textarea action boundary and routed physical selection replacement, Text, and IME only to the focused tile";
#[cfg(feature = "reader")]
const READER_ACCESSIBILITY_WORKSPACE_ASSERTION: &str = "Pelt composed partial Reader link trees with distinct tile namespaces, kept Focus virtual, and preserved the sibling Reader session";
const NARROW_CHROME_WORKSPACE_ASSERTION: &str = "single fixed-height Chrome row shed its secondary controls and kept navigation, address, tab text, and close targets usable while loading and error documents held their content hole";
const CHROME_DPI_WORKSPACE_ASSERTION_PREFIX: &str =
    "high-DPI Chrome converted physical pointer input into its retained logical controls";
#[cfg(feature = "reader")]
const READER_WORKSPACE_ASSERTION: &str = "Reader reused the focused tile's held Livery response, exposed Fleece lineage, and restored the original Livery document without a second fetch";
#[cfg(feature = "tabard-preview")]
const TABARD_PREVIEW_WORKSPACE_ASSERTION: &str = "Tabard changed the computed Pelt Chrome color while the focused document, session history, tabs, and content aperture held";
#[cfg(feature = "tabard-reader-preview")]
const TABARD_READER_PREVIEW_WORKSPACE_ASSERTION: &str = "Tabard recolored Reader's Fleece article through Pelt's host palette while the held response, neighboring Livery tile, and route restoration stayed intact";
#[cfg(feature = "reader")]
const READER_FIXTURE_SOURCE: &str = include_str!("../examples/workspace/reader/index.html");
#[cfg(all(feature = "reader", test))]
const READER_ACCESSIBILITY_FIXTURE_SOURCE: &str =
    include_str!("../examples/workspace/p7-reader-accessibility/index.html");
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
    /// P6's Pelt-owned appearance drawer. This named receipt uses in-memory
    /// storage, while callers may supply a durable Pelt store.
    Appearance,
    /// P6's retained Chrome/Frisket AccessKit tree and typed action routing.
    Accessibility,
    /// P6's address TextInput SetValue route through the installed platform
    /// bridge, including retained loading and error documents.
    AccessibilityAddress,
    /// P7's one-tree Livery child semantics, namespaced and routed through the
    /// same Pelt input path as its content aperture.
    AccessibilityChildren,
    /// P7's native Livery text-control SetValue route, including retained
    /// value projection and ordinary form submission.
    AccessibilityEdit,
    /// P7's nested-scroll Livery ScrollIntoView route through Pelt's
    /// composite accessibility namespace.
    AccessibilityScroll,
    /// P7's clip-aware nested Livery Click route after an explicit reveal.
    AccessibilityClick,
    /// P7's nested Livery editor input and physical text-selection route.
    AccessibilityInput,
    /// P6's compact Chrome layout at a small logical viewport.
    NarrowChrome,
    /// P6's actual high-DPI Chrome input and capture alignment.
    ChromeDpi,
    /// Reader extracts a focused tile's already-held source response in the
    /// shared workspace, then releases it back to its original Livery route.
    Reader,
    /// Reader's already-presented partial link tree is namespaced under each
    /// Pelt content aperture. Focus is virtual until a later host source
    /// handoff can supply destination bodies for Reader navigation.
    ReaderAccessibility,
    /// A developer-only Tabard palette preview over the retained Pelt Chrome.
    /// This does not persist a Pelt appearance preference or recolor documents.
    TabardPreview,
    /// A developer-only Tabard palette preview over Reader's existing
    /// host-theme seam. This does not alter Fleece or persist a Pelt theme.
    TabardReaderPreview,
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
            Self::AccessibilityAddress => "accessibility-address",
            Self::AccessibilityChildren => "accessibility-children",
            Self::AccessibilityEdit => "accessibility-edit",
            Self::AccessibilityScroll => "accessibility-scroll",
            Self::AccessibilityClick => "accessibility-click",
            Self::AccessibilityInput => "accessibility-input",
            Self::NarrowChrome => "narrow-chrome",
            Self::ChromeDpi => "chrome-dpi",
            Self::Reader => "reader",
            Self::ReaderAccessibility => "reader-accessibility",
            Self::TabardPreview => "tabard-preview",
            Self::TabardReaderPreview => "tabard-reader-preview",
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
                | Self::AccessibilityAddress
                | Self::AccessibilityChildren
                | Self::AccessibilityEdit
                | Self::AccessibilityScroll
                | Self::AccessibilityClick
                | Self::AccessibilityInput
                | Self::NarrowChrome
                | Self::ChromeDpi
                | Self::Reader
                | Self::ReaderAccessibility
                | Self::TabardPreview
                | Self::TabardReaderPreview
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
    /// Drive one live document-controller tearout through the native
    /// two-window acceptance path.
    pub tearout_receipt: bool,
    /// Drive a hidden native tearout preflight, then deliberately decline it
    /// before Pelt moves controller custody to the destination.
    pub tearout_cancellation_receipt: bool,
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
    /// Caller-owned Pelt appearance storage. Without it Chrome selection stays
    /// in this process only; Pelt does not invent a config-directory owner.
    pub appearance_store: Option<Box<dyn AppearanceStore>>,
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
            tearout_receipt: false,
            tearout_cancellation_receipt: false,
            workspace_receipt: None,
            chrome: true,
            workspace_size_matrix: None,
            workspace_receipt_stage_timeout: WORKSPACE_RECEIPT_STAGE_TIMEOUT,
            artifact: None,
            route_overrides: HashMap::new(),
            appearance_store: None,
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

    /// Keep Pelt Chrome appearance in a caller-selected store.
    pub fn with_appearance_store(mut self, store: impl AppearanceStore + 'static) -> Self {
        self.appearance_store = Some(Box::new(store));
        self
    }

    pub fn with_capability_receipt(mut self) -> Self {
        self.capability_receipt = true;
        self.chrome = false;
        self.frames = Some(self.frames.unwrap_or(0).max(600));
        self
    }

    /// Drive the native W4 receipt: a source-owned frame presents on a second
    /// shared-device surface before the live controller moves there.
    pub fn with_tearout_receipt(mut self) -> Self {
        self.tearout_receipt = true;
        self.chrome = false;
        self.frames = Some(self.frames.unwrap_or(0).max(180));
        self
    }

    /// Drive W4's headed cancellation receipt through a real hidden native
    /// surface preflight, deliberately before `accept_tearout`.
    pub fn with_tearout_cancellation_receipt(mut self) -> Self {
        self.tearout_cancellation_receipt = true;
        self.chrome = false;
        self.frames = Some(self.frames.unwrap_or(0).max(180));
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
    pub tearout_receipt: bool,
    pub tearout_cancellation_receipt: bool,
    pub workspace_receipt: Option<WorkspaceReceiptOutcome>,
    pub routes: Vec<String>,
}

pub fn run_livery_workspace_viewer(
    config: WorkspaceViewerConfig,
) -> Result<WorkspaceViewerOutcome, String> {
    if config.workspace_receipt.is_some()
        && (config.interaction_receipt
            || config.capability_receipt
            || config.tearout_receipt
            || config.tearout_cancellation_receipt)
    {
        return Err(
            "a named workspace receipt cannot be combined with the P3, P4, or W4 receipt driver"
                .to_owned(),
        );
    }
    if config.tearout_receipt && config.tearout_cancellation_receipt {
        return Err("W4 acceptance and cancellation receipts are separate runs".to_owned());
    }
    if (config.tearout_receipt || config.tearout_cancellation_receipt) && config.urls.len() < 2 {
        return Err(
            "W4 tearout receipt needs a sibling source tile to keep the primary host live"
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
            tearout_receipt: false,
            tearout_cancellation_receipt: false,
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
                | WorkspaceReceipt::AccessibilityAddress
                | WorkspaceReceipt::AccessibilityChildren
                | WorkspaceReceipt::AccessibilityEdit
                | WorkspaceReceipt::AccessibilityScroll
                | WorkspaceReceipt::AccessibilityClick
                | WorkspaceReceipt::AccessibilityInput
                | WorkspaceReceipt::NarrowChrome
                | WorkspaceReceipt::ChromeDpi
                | WorkspaceReceipt::Reader
                | WorkspaceReceipt::ReaderAccessibility
                | WorkspaceReceipt::TabardPreview
                | WorkspaceReceipt::TabardReaderPreview
        )
    );
    #[cfg(target_os = "windows")]
    let scrying_host = (!omit_scrying).then(ScryingReceiptHost::new);
    let fetcher = genet_documents::LocalFetcher::with_resource_policy(
        genet_documents::ResourceFetchPolicy::default(),
    );
    let engine_options = WorkspaceEngineOptions::for_receipt(config.workspace_receipt);
    #[cfg(target_os = "windows")]
    let registries =
        workspace_registries_with_fetcher(fetcher.clone(), engine_options, scrying_host.clone());
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries_with_fetcher(fetcher.clone(), engine_options);
    let overrides = config.route_overrides.clone();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                return Err("standalone Pelt only routes document tile sources".to_owned());
            };
            let engine = overrides.get(&tile.id.0);
            let mut request = if cfg!(feature = "reader")
                && engine.is_some_and(|engine| engine == inker::routing::ENGINE_GENET_READER)
            {
                let resource_address = address
                    .split_once('#')
                    .map_or(address.as_str(), |(resource, _)| resource);
                let response = fetcher.fetch_response(resource_address).ok_or_else(|| {
                    format!("could not acquire the held source response for Reader at {address}")
                })?;
                held_response_request(response, address, initial_size)
            } else {
                PeltTileRequest::new(address, initial_size)
            };
            if let Some(engine) = engine {
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
    if app.config.tearout_receipt && !app.receipt_complete {
        return Err("W4 tearout receipt ended before destination acceptance".to_owned());
    }
    if app.config.tearout_cancellation_receipt && !app.receipt_complete {
        return Err("W4 tearout cancellation receipt ended before host decline".to_owned());
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

/// Per-run document-engine choices. This remains Pelt-local: Tabard supplies
/// a portable palette, while each document engine continues to own how it
/// applies its host colors.
#[derive(Default)]
struct WorkspaceEngineOptions {
    #[cfg(feature = "reader")]
    reader_theme: Option<genet_documents::SmolwebTheme>,
}

impl WorkspaceEngineOptions {
    fn for_receipt(receipt: Option<WorkspaceReceipt>) -> Self {
        #[cfg(feature = "tabard-reader-preview")]
        {
            return Self {
                reader_theme: (receipt == Some(WorkspaceReceipt::TabardReaderPreview))
                    .then(tabard_reader_preview_theme),
            };
        }
        #[cfg(not(feature = "tabard-reader-preview"))]
        {
            let _ = receipt;
            Self::default()
        }
    }
}

fn held_response_request(
    response: genet_host_api::ResourceResponse,
    requested_address: &str,
    viewport: (u32, u32),
) -> PeltTileRequest {
    let genet_host_api::ResourceResponse {
        final_url,
        content_type,
        bytes,
    } = response;
    let address = if final_url.contains('#') {
        final_url
    } else if let Some((_, fragment)) = requested_address.split_once('#') {
        format!("{final_url}#{fragment}")
    } else {
        final_url
    };
    let mut request = SessionSpawnRequest::new(address)
        .with_body(String::from_utf8_lossy(&bytes).into_owned())
        .with_viewport(viewport.0, viewport.1);
    if let Some(content_type) = content_type {
        request = request.with_content_type(content_type);
    }
    PeltTileRequest::from_request(request)
}

#[cfg(test)]
fn workspace_registries(
    #[cfg(target_os = "windows")] scrying_host: Option<ScryingReceiptHost>,
) -> PeltRegistries<Scene> {
    workspace_registries_with_fetcher(
        genet_documents::LocalFetcher::with_resource_policy(
            genet_documents::ResourceFetchPolicy::default(),
        ),
        WorkspaceEngineOptions::default(),
        #[cfg(target_os = "windows")]
        scrying_host,
    )
}

fn workspace_registries_with_fetcher(
    fetcher: genet_documents::ConfiguredLocalFetcher,
    engine_options: WorkspaceEngineOptions,
    #[cfg(target_os = "windows")] scrying_host: Option<ScryingReceiptHost>,
) -> PeltRegistries<Scene> {
    let mut sessions: SessionRegistry<Scene> = SessionRegistry::new();
    sessions.register(Box::new(genet_documents::LiverySessionEngine::new(
        fetcher.clone(),
    )));
    #[cfg(feature = "reader")]
    sessions.register(Box::new(match engine_options.reader_theme {
        Some(theme) => genet_documents::ReaderSessionEngine::new(theme),
        None => genet_documents::ReaderSessionEngine::default(),
    }));
    #[cfg(not(feature = "reader"))]
    let _ = engine_options;
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

/// The Tabard receipt changes only the retained Chrome stylesheet. Keep the
/// document-facing state and Frisket geometry explicit so the preview cannot
/// accidentally become a navigation or layout path.
#[cfg(feature = "tabard-preview")]
#[derive(Clone, Debug, PartialEq)]
struct TabardPreviewBaseline {
    focused_tile: TileId,
    tile_count: usize,
    content: WorkspaceRect,
    tab: WorkspaceRect,
    address: String,
    can_go_back: bool,
    can_go_forward: bool,
    chrome_background: String,
}

#[cfg(any(feature = "tabard-preview", feature = "tabard-reader-preview"))]
fn tabard_preview_theme() -> tabard::Theme {
    tabard::Theme::new(
        "Pelt Tabard preview",
        tinct::Seeds {
            primary: tinct::Srgb::rgb(0x33, 0x66, 0xc8),
            secondary: tinct::Srgb::rgb(0x2e, 0x9d, 0xa6),
            tertiary: tinct::Srgb::rgb(0xe0, 0xa8, 0x46),
            neutral: tinct::Srgb::rgb(0x10, 0x14, 0x22),
            text_header: None,
            text_body: None,
            success: tinct::Srgb::rgb(0x4f, 0xb3, 0x6e),
            danger: tinct::Srgb::rgb(0xd5, 0x4e, 0x4e),
            dark: true,
        },
    )
}

#[cfg(feature = "tabard-reader-preview")]
fn tabard_reader_preview_theme() -> genet_documents::SmolwebTheme {
    let palette = tabard_preview_theme().palette();
    genet_documents::SmolwebTheme::App(genet_documents::SmolwebPalette {
        bg: tinct::color_to_hex(palette.bg),
        fg: tinct::color_to_hex(palette.text),
        link: tinct::color_to_hex(palette.primary),
        quote: tinct::color_to_hex(palette.text_dim),
        pre_bg: tinct::color_to_hex(palette.surface_2),
    })
}

#[cfg(feature = "tabard-preview")]
fn tabard_preview_stylesheet() -> String {
    let theme = tabard_preview_theme();
    let mut stylesheet = theme.css_custom_properties().replacen(
        ":root",
        ".pelt-workspace, .pelt-workspace.pelt-theme-light",
        1,
    );
    // Pelt's Light appearance declares the same roles on the compound
    // selector. Keep equal specificity here so a Tabard preview owns either
    // Pelt appearance without making Tabard responsible for Chrome names.
    stylesheet.push_str(
        "\
.pelt-workspace, .pelt-workspace.pelt-theme-light { \
--pelt-chrome-workspace: var(--tabard-color-bg); --pelt-chrome-surface: var(--tabard-color-surface); --pelt-chrome-border: var(--tabard-color-surface-2); \
--pelt-chrome-control-text: var(--tabard-color-text); --pelt-chrome-control-surface: var(--tabard-color-surface-2); --pelt-chrome-control-border: var(--tabard-color-surface-hover); \
--pelt-chrome-disabled-text: var(--tabard-color-text-disabled); --pelt-chrome-disabled-surface: var(--tabard-color-surface); --pelt-chrome-disabled-border: var(--tabard-color-surface-2); \
--pelt-chrome-accent-text: var(--tabard-color-on-primary); --pelt-chrome-accent-surface: var(--tabard-color-primary); --pelt-chrome-accent-border: var(--tabard-color-secondary); \
--pelt-chrome-address-text: var(--tabard-color-text); --pelt-chrome-address-surface: var(--tabard-color-bg); \
--pelt-chrome-context-text: var(--tabard-color-on-secondary); --pelt-chrome-context-surface: var(--tabard-color-secondary); --pelt-chrome-context-border: var(--tabard-color-tertiary); \
--pelt-chrome-selection-text: var(--tabard-color-on-primary); --pelt-chrome-selection-surface: var(--tabard-color-primary); --pelt-chrome-selection-border: var(--tabard-color-tertiary); \
--pelt-chrome-heading: var(--tabard-color-text-header); --pelt-chrome-route: var(--tabard-color-secondary); --pelt-chrome-status: var(--tabard-color-success); \
--pelt-chrome-panel-text: var(--tabard-color-text); --pelt-chrome-panel-surface: var(--tabard-color-surface); --pelt-chrome-panel-border: var(--tabard-color-surface-2); \
--pelt-chrome-summary: var(--tabard-color-text); --pelt-chrome-section: var(--tabard-color-secondary); --pelt-chrome-entry: var(--tabard-color-text); --pelt-chrome-muted: var(--tabard-color-text-dim); \
--pelt-chrome-diagnostic-border: var(--tabard-color-surface-hover); --pelt-chrome-loading-text: var(--tabard-color-on-secondary); --pelt-chrome-loading-surface: var(--tabard-color-secondary); --pelt-chrome-loading-border: var(--tabard-color-primary); \
--pelt-chrome-error-text: var(--tabard-color-on-tertiary); --pelt-chrome-error-surface: var(--tabard-color-danger); --pelt-chrome-error-border: var(--tabard-color-tertiary); \
--pelt-chrome-diagnostic-address: var(--tabard-color-secondary); --pelt-chrome-diagnostic-note: var(--tabard-color-text-dim); \
--pelt-chrome-tabbar: var(--tabard-color-surface-2); --pelt-chrome-tab-text: var(--tabard-color-text-dim); --pelt-chrome-tab-surface: var(--tabard-color-surface); \
--pelt-chrome-tab-active-text: var(--tabard-color-on-primary); --pelt-chrome-tab-active-surface: var(--tabard-color-primary); --pelt-chrome-tab-close: var(--tabard-color-text); \
--pelt-chrome-content-surface: var(--tabard-color-bg); --pelt-chrome-divider: var(--tabard-color-surface-hover); }\n",
    );
    stylesheet
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
    /// A hidden, already-booted destination that has not yet earned custody.
    /// Source controllers remain in `workspace` until this surface presents
    /// its preflight composition.
    pending_tearouts: HashMap<WindowId, PendingTearout>,
    /// Live, composed secondary windows. Every entry shares the main window's
    /// `RenderCore`, but owns its own `WindowSurface` and Pelt workspace.
    tearouts: HashMap<WindowId, TearoutWindow>,
    width: u32,
    height: u32,
    scale_factor: f32,
    redraws: u32,
    modifiers: SessionModifiers,
    cursor: (f32, f32),
    gesture: Option<PointerGesture>,
    /// A caption Close click; honoured by the event loop on the next event,
    /// which owns `event_loop.exit()`.
    close_requested: bool,
    /// Double-click clock for the CSD drag surface.
    drag_cadence: cambium_genet_winit_host::ClickCadence,
    /// The border resize direction under the cursor, deduped for cursor swaps.
    resize_hint: Option<winit::window::ResizeDirection>,
    #[cfg(target_os = "windows")]
    snap_bridge: Option<cambium_genet_winit_host::SnapLayoutBridge>,
    receipt_step: u8,
    receipt_complete: bool,
    tearout_receipt_tile: Option<TileId>,
    tearout_cancellation_receipt_tile: Option<TileId>,
    tearout_cancellation_preflight_presented: bool,
    tearout_cancellation_hidden_preflight: bool,
    tearout_receipt_started: Option<Instant>,
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
    appearance: AppearanceSettingsProvider<Box<dyn AppearanceStore>>,
    chrome_appearance_open: bool,
    appearance_receipt_baseline: Option<AppearanceReceiptBaseline>,
    #[cfg(feature = "tabard-preview")]
    tabard_preview_baseline: Option<TabardPreviewBaseline>,
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

/// One destination that has a configured swapchain and a source-owned snapshot
/// ready to compose. It remains hidden until that composition succeeds.
struct PendingTearout {
    tile: TileId,
    window: Arc<Window>,
    core: Arc<RenderCore>,
    surface: WindowSurface,
    frisket: FrisketSurface,
    pane_scene: Scene,
    frame: PeltWorkspaceFrame<Scene>,
    width: u32,
    height: u32,
    scale_factor: f32,
    disposition: TearoutDisposition,
}

/// A fully accepted Pelt tearout window. The shared render core preserves the
/// host-wide wgpu device while this window owns its swapchain and input state.
struct TearoutWindow {
    tile: TileId,
    window: Arc<Window>,
    core: Arc<RenderCore>,
    surface: WindowSurface,
    workspace: PeltWorkspace<Scene>,
    frisket: FrisketSurface,
    width: u32,
    height: u32,
    scale_factor: f32,
    cursor: (f32, f32),
    modifiers: SessionModifiers,
    /// Recorded only after a source-owned preflight frame presented, the host
    /// requested visibility, and Winit delivered native focus to this window.
    native_focus_observed: bool,
    visibility_requested: bool,
    visible_frame_presented: bool,
}

/// One stable virtual focus target in the composite workspace tree.
#[derive(Clone, Debug, Eq, PartialEq)]
enum WorkspaceA11yFocus {
    Frisket(FrisketA11yTarget),
    /// A virtual focus target in an engine-owned child subtree. The child
    /// engine remains responsible for its semantics; Pelt owns only its
    /// one-tree namespace and host focus state.
    Document(AccessNodeId),
}

/// A Pelt-owned action target for one node in the composite tree.
#[derive(Clone, Debug)]
enum WorkspaceA11yActionTarget {
    Frisket(genet_scripted_dom::NodeId),
    Livery(LiveryA11yAction),
    #[cfg(feature = "reader")]
    Reader(ReaderA11yAction),
}

impl WorkspaceA11yActionTarget {
    fn is_document(&self) -> bool {
        match self {
            Self::Frisket(_) => false,
            Self::Livery(_) => true,
            #[cfg(feature = "reader")]
            Self::Reader(_) => true,
        }
    }
}

/// The session identity and workspace point needed to route a retained Livery
/// semantic action through the ordinary Pelt input path.
#[derive(Clone, Copy, Debug)]
struct LiveryA11yAction {
    tile: TileId,
    session_generation: u64,
    local_node: AccessNodeId,
    content_rect: WorkspaceRect,
    /// Whether the composite tree advertised Click for this action. A pointer
    /// point alone is never authority to activate a disabled or hidden node.
    click_enabled: bool,
    /// Whether the current composite tree advertised Focus for this node.
    focus_enabled: bool,
    /// Whether the current composite tree advertised ScrollIntoView for this
    /// node.
    scroll_enabled: bool,
    /// Whether the current composite tree advertised SetValue for this
    /// retained node. Accessibility actions are checked against this snapshot
    /// before Pelt asks Livery to mutate a value.
    set_value_enabled: bool,
    /// The point published with the composite tree proves that Click was
    /// advertised. Dispatch queries Livery again instead of trusting these
    /// coordinates after a later scroll in the same session.
    click_point: Option<(f32, f32)>,
}

/// The session identity and logical Reader link needed to keep a retained
/// semantic Focus action virtual and stale-safe.
#[cfg(feature = "reader")]
#[derive(Clone, Debug)]
struct ReaderA11yAction {
    tile: TileId,
    session_generation: u64,
    local_node: AccessNodeId,
    /// The retained Reader record carries the opaque document-canvas identity.
    /// It is compared against the current partial snapshot before Pelt accepts
    /// a virtual Focus action.
    link: genet_documents::ReaderAccessibilityLink,
    focus_enabled: bool,
}

/// One tile-local namespace inside Pelt's root AccessKit tree.
///
/// AccessKit node IDs are only local-tree unique. Pelt owns these assignments
/// so independently constructed `ScriptedDom`s cannot collide when their
/// retained trees become siblings below Frisket's content apertures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentA11ySource {
    Livery,
    #[cfg(feature = "reader")]
    Reader,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DocumentA11ySession {
    source: DocumentA11ySource,
    generation: u64,
}

struct DocumentA11yNamespace {
    source: DocumentA11ySource,
    session_generation: u64,
    global_ids: HashMap<AccessNodeId, AccessNodeId>,
    /// Reader's opaque link tokens cannot be represented as accesskit IDs and
    /// must not be replaced with output order. Retain a tile/session-local
    /// association so a removed link's global ID becomes inert rather than
    /// being aliased to its next sibling after a reflow.
    #[cfg(feature = "reader")]
    reader_local_ids: Vec<(genet_documents::ReaderAccessibilityLink, AccessNodeId)>,
    #[cfg(feature = "reader")]
    next_reader_local_node_id: u64,
}

impl DocumentA11yNamespace {
    fn new(source: DocumentA11ySource, session_generation: u64) -> Self {
        Self {
            source,
            session_generation,
            global_ids: HashMap::new(),
            #[cfg(feature = "reader")]
            reader_local_ids: Vec::new(),
            #[cfg(feature = "reader")]
            // The Reader root owns local ID 1. Link tokens begin at 2.
            next_reader_local_node_id: 2,
        }
    }
}

/// The tree and action map delivered together to the one-tree platform bridge.
struct WorkspaceA11yProjection {
    tree: TreeUpdate,
    root: AccessNodeId,
    actions: HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
}

/// A completed retained Livery tree ready to attach below one Frisket hole.
struct LiveryA11yChild {
    tile: TileId,
    session_generation: u64,
    aperture: AccessNodeId,
    root: AccessNodeId,
    tree: TreeUpdate,
    transform: Affine,
    content_rect: WorkspaceRect,
    content_origin: (f32, f32),
    page_zoom: f32,
    /// Livery-owned CSS hit points for semantic Click targets. AccessKit
    /// bounds remain presentation data and are not a pointer-routing oracle.
    pointer_targets: HashMap<AccessNodeId, (f32, f32)>,
}

/// A partial Reader semantic tree assembled from its already-presented,
/// renderer-neutral snapshot. It exposes only visible links and never claims a
/// complete semantic rendering of Fleece's article.
#[cfg(feature = "reader")]
struct ReaderA11yChild {
    tile: TileId,
    session_generation: u64,
    aperture: AccessNodeId,
    root: AccessNodeId,
    nodes: Vec<(AccessNodeId, AccessNode)>,
    transform: Affine,
    links: HashMap<AccessNodeId, genet_documents::ReaderAccessibilityLink>,
}

#[cfg(feature = "reader")]
fn reader_link_bounds(rects: &[[f32; 4]]) -> Option<AccessRect> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for &[x, y, width, height] in rects {
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            continue;
        }
        let right = x + width;
        let bottom = y + height;
        if !right.is_finite() || !bottom.is_finite() {
            continue;
        }
        let rect = (
            f64::from(x),
            f64::from(y),
            f64::from(right),
            f64::from(bottom),
        );
        bounds = Some(match bounds {
            Some((left, top, old_right, old_bottom)) => (
                left.min(rect.0),
                top.min(rect.1),
                old_right.max(rect.2),
                old_bottom.max(rect.3),
            ),
            None => rect,
        });
    }
    bounds.map(|(left, top, right, bottom)| AccessRect::new(left, top, right, bottom))
}

/// Per-window platform bridge and retained composite action map.
struct WorkspaceAccessibility {
    bridge: AccessKitBridge,
    window_revealed: bool,
    last_install_error: Option<String>,
    action_map: HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
    focus: Option<WorkspaceA11yFocus>,
    child_namespaces: HashMap<TileId, DocumentA11yNamespace>,
    assigned_child_ids: HashSet<AccessNodeId>,
    next_child_node_id: u64,
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
            child_namespaces: HashMap::new(),
            assigned_child_ids: HashSet::new(),
            // Keep Pelt-owned IDs in a distinct range while still checking
            // every shell ID. The allocator never recycles an issued child ID,
            // which makes a stale platform action inert rather than aliased.
            next_child_node_id: 1_u64 << 63,
            wake,
        }
    }

    fn prepare(&mut self, projection: WorkspaceA11yProjection, scale_factor: f64) -> TreeUpdate {
        self.action_map = projection.actions;
        let mut tree = projection.tree;
        if let Some(WorkspaceA11yFocus::Document(id)) = self.focus.as_ref()
            && self
                .action_map
                .get(id)
                .is_some_and(WorkspaceA11yActionTarget::is_document)
        {
            tree.focus = *id;
        }
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
        projection: WorkspaceA11yProjection,
        scale_factor: f64,
    ) -> Vec<A11yActionRequest> {
        let node_count = projection.tree.nodes.len();
        let tree = self.prepare(projection, scale_factor);
        if self.bridge.status() != BridgeStatus::Installed {
            match self.bridge.install(window, tree) {
                Ok(()) => {
                    self.last_install_error = None;
                    eprintln!(
                        "[pelt] accessibility {:?}, {node_count} retained workspace nodes projected",
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

    fn action_for(&self, id: AccessNodeId) -> Option<WorkspaceA11yActionTarget> {
        self.action_map.get(&id).cloned()
    }

    fn set_focus(&mut self, target: WorkspaceA11yFocus) -> bool {
        if self.focus.as_ref() == Some(&target) {
            return false;
        }
        self.focus = Some(target);
        true
    }

    fn frisket_focus(&self) -> Option<&FrisketA11yTarget> {
        match self.focus.as_ref() {
            Some(WorkspaceA11yFocus::Frisket(target)) => Some(target),
            Some(WorkspaceA11yFocus::Document(_)) | None => None,
        }
    }

    fn retain_document_namespaces(&mut self, live_sessions: &HashMap<TileId, DocumentA11ySession>) {
        self.child_namespaces.retain(|tile, namespace| {
            live_sessions.get(tile)
                == Some(&DocumentA11ySession {
                    source: namespace.source,
                    generation: namespace.session_generation,
                })
        });
    }

    fn child_global_id(
        &mut self,
        tile: TileId,
        source: DocumentA11ySource,
        session_generation: u64,
        local_id: AccessNodeId,
        shell_ids: &HashSet<AccessNodeId>,
    ) -> AccessNodeId {
        let reset_namespace = self.child_namespaces.get(&tile).is_some_and(|namespace| {
            namespace.source != source
                || namespace.session_generation != session_generation
                || namespace
                    .global_ids
                    .values()
                    .any(|id| shell_ids.contains(id))
        });
        if reset_namespace {
            self.child_namespaces.remove(&tile);
        }
        if let Some(id) = self
            .child_namespaces
            .get(&tile)
            .and_then(|namespace| namespace.global_ids.get(&local_id))
        {
            return *id;
        }

        let global_id = self.allocate_child_id(shell_ids);
        let namespace = self
            .child_namespaces
            .entry(tile)
            .or_insert_with(|| DocumentA11yNamespace::new(source, session_generation));
        debug_assert_eq!(namespace.source, source);
        debug_assert_eq!(namespace.session_generation, session_generation);
        namespace.global_ids.insert(local_id, global_id);
        global_id
    }

    /// Give one opaque Reader link token a stable local AccessKit ID for the
    /// life of its tile session. This map intentionally retains disappeared
    /// links until the session is replaced, so a queued action cannot acquire
    /// a different link merely because the next snapshot emitted it earlier.
    #[cfg(feature = "reader")]
    fn reader_local_node_id(
        &mut self,
        tile: TileId,
        session_generation: u64,
        link: &genet_documents::ReaderAccessibilityLink,
    ) -> AccessNodeId {
        let reset_namespace = self.child_namespaces.get(&tile).is_some_and(|namespace| {
            namespace.source != DocumentA11ySource::Reader
                || namespace.session_generation != session_generation
        });
        if reset_namespace {
            self.child_namespaces.remove(&tile);
        }
        let namespace = self.child_namespaces.entry(tile).or_insert_with(|| {
            DocumentA11yNamespace::new(DocumentA11ySource::Reader, session_generation)
        });
        debug_assert_eq!(namespace.source, DocumentA11ySource::Reader);
        debug_assert_eq!(namespace.session_generation, session_generation);
        if let Some((_, id)) = namespace
            .reader_local_ids
            .iter()
            .find(|(known, _)| known.identity == link.identity)
        {
            return *id;
        }
        let id = AccessNodeId(namespace.next_reader_local_node_id);
        namespace.next_reader_local_node_id = namespace
            .next_reader_local_node_id
            .checked_add(1)
            .expect("Pelt Reader accessibility local node IDs exhausted");
        namespace.reader_local_ids.push((link.clone(), id));
        id
    }

    fn allocate_child_id(&mut self, shell_ids: &HashSet<AccessNodeId>) -> AccessNodeId {
        loop {
            let candidate = AccessNodeId(self.next_child_node_id);
            self.next_child_node_id = self
                .next_child_node_id
                .checked_add(1)
                .expect("Pelt accessibility child node IDs exhausted");
            if !shell_ids.contains(&candidate) && self.assigned_child_ids.insert(candidate) {
                return candidate;
            }
        }
    }

    fn clear_stale_document_focus(
        &mut self,
        actions: &HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
    ) {
        let Some(WorkspaceA11yFocus::Document(id)) = self.focus.as_ref() else {
            return;
        };
        if !actions
            .get(id)
            .is_some_and(WorkspaceA11yActionTarget::is_document)
        {
            self.focus = None;
        }
    }

    fn child_id_is_reserved(&self, id: AccessNodeId) -> bool {
        self.assigned_child_ids.contains(&id)
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
        mut config: WorkspaceViewerConfig,
        workspace: PeltWorkspace<Scene>,
        frisket: FrisketSurface,
        #[cfg(target_os = "windows")] scrying_host: Option<ScryingReceiptHost>,
    ) -> Self {
        let (width, height) = config.size.unwrap_or((1100, 750));
        let appearance = AppearanceSettingsProvider::new(
            config
                .appearance_store
                .take()
                .unwrap_or_else(|| Box::new(InMemoryAppearanceStore::default())),
        );
        Self {
            config,
            workspace,
            frisket,
            window: None,
            host: None,
            pending_tearouts: HashMap::new(),
            tearouts: HashMap::new(),
            width,
            height,
            scale_factor: 1.0,
            redraws: 0,
            modifiers: SessionModifiers::default(),
            cursor: (0.0, 0.0),
            gesture: None,
            close_requested: false,
            drag_cadence: cambium_genet_winit_host::ClickCadence::new(),
            resize_hint: None,
            #[cfg(target_os = "windows")]
            snap_bridge: None,
            receipt_step: 0,
            receipt_complete: false,
            tearout_receipt_tile: None,
            tearout_cancellation_receipt_tile: None,
            tearout_cancellation_preflight_presented: false,
            tearout_cancellation_hidden_preflight: false,
            tearout_receipt_started: None,
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
            appearance,
            chrome_appearance_open: false,
            appearance_receipt_baseline: None,
            #[cfg(feature = "tabard-preview")]
            tabard_preview_baseline: None,
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

    fn chrome_theme(&self) -> AppearanceTheme {
        self.appearance.store().theme()
    }

    fn chrome_appearance(&self) -> ChromeAppearance {
        ChromeAppearance {
            theme: self.chrome_theme(),
            persistent: self.appearance.store().is_persistent(),
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
            tearout_receipt: self.config.tearout_receipt && self.receipt_complete,
            tearout_cancellation_receipt: self.config.tearout_cancellation_receipt
                && self.receipt_complete,
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

    fn chrome_engine_choices() -> Vec<ChromeEngineChoice> {
        let mut choices = vec![ChromeEngineChoice::Automatic, ChromeEngineChoice::Livery];
        #[cfg(feature = "reader")]
        choices.push(ChromeEngineChoice::Reader);
        #[cfg(feature = "scripted")]
        choices.push(ChromeEngineChoice::Scripted);
        choices
    }

    fn selected_chrome_engine(&self, tile: TileId) -> Option<ChromeEngineChoice> {
        let route = self.workspace.route(tile)?;
        if route.source == PeltRouteSource::Automatic {
            return Some(ChromeEngineChoice::Automatic);
        }
        match route.selected_engine() {
            inker::routing::ENGINE_GENET_LIVERY => Some(ChromeEngineChoice::Livery),
            inker::routing::ENGINE_GENET_READER => Some(ChromeEngineChoice::Reader),
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

    /// Whether this workspace draws its own caption controls: only when a
    /// real window exists and was created undecorated (CSD) — Windows today,
    /// other platforms when their decoration lanes land. The windowless
    /// GPU-free harness never draws them, which keeps its expectations
    /// platform-stable.
    fn draws_window_controls(&self) -> bool {
        cfg!(target_os = "windows") && self.window.is_some()
    }

    fn window_is_maximized(&self) -> bool {
        self.window
            .as_deref()
            .is_some_and(winit::window::Window::is_maximized)
    }

    fn chrome_model(&self) -> WorkspaceChrome {
        let theme = self.chrome_theme();
        let Some(tile) = self.workspace.focused_tile() else {
            return WorkspaceChrome {
                title: "No focused tile".to_owned(),
                address: String::new(),
                route: "No route".to_owned(),
                status: self.chrome_status.label(),
                theme,
                address_focused: self.chrome_address.is_some(),
                can_go_back: false,
                can_go_forward: false,
                engine_label: "Auto".to_owned(),
                engine_menu_open: false,
                engine_selected: None,
                engine_choices: Self::chrome_engine_choices(),
                inspector: None,
                appearance: self
                    .chrome_appearance_open
                    .then(|| self.chrome_appearance()),
                diagnostic: None,
                window_controls: self.draws_window_controls(),
                maximized: self.window_is_maximized(),
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
            theme,
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
            appearance: self
                .chrome_appearance_open
                .then(|| self.chrome_appearance()),
            diagnostic,
            window_controls: self.draws_window_controls(),
            maximized: self.window_is_maximized(),
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
            ChromeEngineChoice::Reader => {
                #[cfg(feature = "reader")]
                {
                    Some(inker::routing::ENGINE_GENET_READER.to_owned())
                }
                #[cfg(not(feature = "reader"))]
                {
                    self.chrome_status =
                        ChromeStatus::Error("Reader is unavailable in this Pelt build".to_owned());
                    return true;
                }
            },
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

    fn choose_chrome_theme(&mut self, theme: AppearanceTheme) -> bool {
        if !self.chrome_appearance_open {
            return false;
        }
        let reference = SettingsRef(APPEARANCE_REFERENCE.into());
        if let Err(error) = self.appearance.apply(
            &reference,
            CHROME_THEME_SETTING,
            SettingValue::Text(theme.as_str().to_owned()),
        ) {
            self.chrome_status =
                ChromeStatus::Error(format!("Could not save Chrome theme: {error:?}"));
            return true;
        }
        let persistence = if self.appearance.store().is_persistent() {
            "saved"
        } else {
            "session only"
        };
        self.chrome_status =
            ChromeStatus::Message(format!("Chrome theme: {} ({persistence})", theme.label()));
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
            ChromeAction::Minimize => {
                if let Some(window) = self.window.as_deref() {
                    window.set_minimized(true);
                }
                false
            },
            ChromeAction::ToggleMaximize => {
                if let Some(window) = self.window.as_deref() {
                    window.set_maximized(!window.is_maximized());
                }
                self.refresh_chrome();
                true
            },
            ChromeAction::CloseWindow => {
                self.close_requested = true;
                false
            },
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
        if self.config.tearout_receipt && self.redraws > 0 && !self.receipt_complete {
            match self.drive_tearout_receipt(event_loop) {
                Ok(true) => self.receipt_complete = true,
                Ok(false) => {},
                Err(error) => {
                    self.receipt_error = Some(error);
                    event_loop.exit();
                    return;
                },
            }
        }
        if self.config.tearout_cancellation_receipt && self.redraws > 0 && !self.receipt_complete {
            match self.drive_tearout_cancellation_receipt(event_loop) {
                Ok(true) => self.receipt_complete = true,
                Ok(false) => {},
                Err(error) => {
                    self.receipt_error = Some(error);
                    event_loop.exit();
                    return;
                },
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
        // The document engines own retained layout production. Update the
        // composite tree only after this frame has made their completed
        // geometry observable, so a static document gains its child semantics
        // even when it does not ask the window for a second visual redraw.
        if self.sync_accessibility() {
            self.request_redraw();
        }
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
                        | WorkspaceReceipt::AccessibilityAddress
                        | WorkspaceReceipt::AccessibilityChildren
                        | WorkspaceReceipt::AccessibilityEdit
                        | WorkspaceReceipt::AccessibilityScroll
                        | WorkspaceReceipt::AccessibilityClick
                        | WorkspaceReceipt::AccessibilityInput
                        | WorkspaceReceipt::NarrowChrome
                        | WorkspaceReceipt::ChromeDpi
                        | WorkspaceReceipt::Reader
                        | WorkspaceReceipt::ReaderAccessibility
                        | WorkspaceReceipt::TabardPreview
                        | WorkspaceReceipt::TabardReaderPreview
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
            if let Some(error) = self.tearout_receipt_timeout_error() {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            }
            // An outdated or lost swapchain is deliberately skipped by the
            // shared host. A named receipt must drive one recovery frame,
            // otherwise a geometry change can leave its native surface
            // waiting without another redraw.
            if (self.config.workspace_receipt.is_some()
                || self.config.tearout_receipt
                || self.config.tearout_cancellation_receipt)
                && !self.receipt_complete
            {
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
        if let Some(error) = self.tearout_receipt_timeout_error() {
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
                | WorkspaceReceipt::AccessibilityAddress
                | WorkspaceReceipt::AccessibilityChildren
                | WorkspaceReceipt::AccessibilityEdit
                | WorkspaceReceipt::AccessibilityScroll
                | WorkspaceReceipt::AccessibilityClick
                | WorkspaceReceipt::AccessibilityInput
                | WorkspaceReceipt::NarrowChrome
                | WorkspaceReceipt::ChromeDpi
                | WorkspaceReceipt::Reader
                | WorkspaceReceipt::ReaderAccessibility
                | WorkspaceReceipt::TabardPreview
                | WorkspaceReceipt::TabardReaderPreview,
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

    fn apply_tile_event(&mut self, event: TileEvent, event_loop: Option<&ActiveEventLoop>) -> bool {
        let focused_before = self.workspace.focused_tile();
        let outcome = self.workspace.apply_outcome(&event);
        if let Some(WorkbenchEffect::TearOut { tile }) = outcome.effect() {
            let Some(event_loop) = event_loop else {
                self.chrome_status = ChromeStatus::Message(format!(
                    "Tearout requested for tile {}; a native destination is not available in this dispatch",
                    tile.0
                ));
                return true;
            };
            return self.prepare_native_tearout(event_loop, tile, TearoutDisposition::Accept);
        }
        if outcome.changed() {
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

    /// Deterministic headed W4 hook. The first settled primary frame requests
    /// an outside drop through the same host path as a user drag, then records
    /// the live acceptance facts: a presented/visible destination window, its
    /// retained stable tile and model focus, and source custody removal.
    fn drive_tearout_receipt(&mut self, event_loop: &ActiveEventLoop) -> Result<bool, String> {
        let tile = match self.tearout_receipt_tile {
            Some(tile) => tile,
            None => {
                let tile = self
                    .workspace
                    .focused_tile()
                    .ok_or_else(|| "W4 tearout receipt needs a focused source tile".to_owned())?;
                if !self.apply_tile_event(
                    TileEvent::Dragged {
                        tile,
                        to: DropTarget::Outside,
                    },
                    Some(event_loop),
                ) {
                    return Err(
                        "W4 tearout receipt did not dispatch an outside-drop request".to_owned(),
                    );
                }
                self.tearout_receipt_tile = Some(tile);
                tile
            },
        };
        if self
            .pending_tearouts
            .values()
            .any(|pending| pending.tile == tile)
        {
            self.request_redraw();
            return Ok(false);
        }
        let accepted = self
            .tearouts
            .values()
            .find(|tearout| tearout.tile == tile)
            .ok_or_else(|| "W4 tearout receipt had no accepted destination window".to_owned())?;
        if accepted.workspace.focused_tile() != Some(tile)
            || accepted.workspace.tree().find(tile).is_none()
            || self.workspace.tree().find(tile).is_some()
            || self.workspace.controller(tile).is_some()
        {
            return Err(
                "W4 tearout receipt lost destination visibility, tile identity, model focus, or source custody"
                    .to_owned(),
            );
        }
        if !accepted.native_focus_observed || !accepted.visible_frame_presented {
            accepted.window.request_redraw();
            self.request_redraw();
            return Ok(false);
        }
        Ok(true)
    }

    /// Deterministic headed W4 cancellation hook. It creates and presents the
    /// same hidden native destination preflight as acceptance, then the host
    /// intentionally declines before `PeltWorkspace::accept_tearout` can
    /// transfer source custody.
    fn drive_tearout_cancellation_receipt(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<bool, String> {
        let tile = match self.tearout_cancellation_receipt_tile {
            Some(tile) => tile,
            None => {
                let tile = self.workspace.focused_tile().ok_or_else(|| {
                    "W4 tearout cancellation receipt needs a focused source tile".to_owned()
                })?;
                let outcome = self.workspace.apply_outcome(&TileEvent::Dragged {
                    tile,
                    to: DropTarget::Outside,
                });
                let Some(WorkbenchEffect::TearOut { tile: effect_tile }) = outcome.effect() else {
                    return Err(
                        "W4 tearout cancellation receipt did not dispatch an outside-drop request"
                            .to_owned(),
                    );
                };
                if effect_tile != tile {
                    return Err(
                        "W4 tearout cancellation receipt changed the requested tile identity"
                            .to_owned(),
                    );
                }
                self.tearout_cancellation_receipt_tile = Some(tile);
                if !self.prepare_native_tearout(
                    event_loop,
                    tile,
                    TearoutDisposition::DeclineForReceipt,
                ) {
                    return Err(
                        "W4 tearout cancellation receipt could not prepare a native destination"
                            .to_owned(),
                    );
                }
                tile
            },
        };
        if self
            .pending_tearouts
            .values()
            .any(|pending| pending.tile == tile)
        {
            self.request_redraw();
            return Ok(false);
        }
        if self.tearouts.values().any(|tearout| tearout.tile == tile) {
            return Err("W4 tearout cancellation receipt accepted destination custody".to_owned());
        }
        if !self.tearout_cancellation_hidden_preflight {
            return Err(
                "W4 tearout cancellation receipt did not keep the native destination hidden"
                    .to_owned(),
            );
        }
        if !self.tearout_cancellation_preflight_presented {
            self.request_redraw();
            return Ok(false);
        }
        if self.workspace.focused_tile() != Some(tile)
            || self.workspace.tree().find(tile).is_none()
            || self.workspace.controller(tile).is_none()
        {
            return Err(
                "W4 tearout cancellation receipt lost source tree, controller, or model focus"
                    .to_owned(),
            );
        }
        Ok(true)
    }

    fn tearout_receipt_timeout_error(&self) -> Option<String> {
        let started = self.tearout_receipt_started?;
        if self.receipt_complete || started.elapsed() < self.config.workspace_receipt_stage_timeout
        {
            return None;
        }
        let kind = if self.config.tearout_cancellation_receipt {
            "cancellation"
        } else {
            "acceptance"
        };
        Some(format!(
            "W4 {kind} receipt timed out after {}s at {}x{} (pending={} accepted={})",
            self.config.workspace_receipt_stage_timeout.as_secs_f32(),
            self.width,
            self.height,
            self.pending_tearouts.len(),
            self.tearouts.len(),
        ))
    }

    /// Create and preflight a hidden destination surface without moving the
    /// source controller. `pending_tearouts` presents one source-owned frame
    /// before it calls `PeltWorkspace::accept_tearout`.
    fn prepare_native_tearout(
        &mut self,
        event_loop: &ActiveEventLoop,
        tile: TileId,
        disposition: TearoutDisposition,
    ) -> bool {
        if self.workspace.controller(tile).is_none() {
            self.chrome_status = ChromeStatus::Message(format!(
                "Tearout requested for tile {}; native surface composition is not available yet",
                tile.0
            ));
            return true;
        }
        let Some(tile_record) = self.workspace.tree().find(tile).cloned() else {
            return false;
        };
        let Some(core) = self.host.as_ref().map(SurfaceHost::shared_core) else {
            self.chrome_status = ChromeStatus::Error(
                "Pelt cannot prepare a tearout before its primary surface is ready".to_owned(),
            );
            return true;
        };
        let title = static_viewer::pelt_window_title(Some(&tile_record.title), None);
        // This is an independent native window. Keep the OS frame until the
        // secondary host has its own CSD and accessibility bridge.
        let attributes = static_viewer::pelt_window_attributes(title, self.width, self.height)
            .with_visible(false);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.chrome_status =
                    ChromeStatus::Error(format!("Could not create tearout window: {error}"));
                return true;
            },
        };
        // Winit resolves the destination against its actual monitor. Read its
        // client size and scale after creation rather than assuming the source
        // window's DPI or physical extent.
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let scale_factor = window.scale_factor() as f32;
        let surface = match core.create_surface(Arc::clone(&window), width, height) {
            Ok(surface) => surface,
            Err(error) => {
                self.chrome_status =
                    ChromeStatus::Error(format!("Could not configure tearout surface: {error}"));
                return true;
            },
        };
        let mut frisket = FrisketSurface::new(&TileTree::single(tile_record));
        let logical_width = static_viewer::logical_extent(width, scale_factor);
        let logical_height = static_viewer::logical_extent(height, scale_factor);
        let pane = match frisket.frame(logical_width, logical_height) {
            Ok(pane) => pane,
            Err(error) => {
                self.chrome_status =
                    ChromeStatus::Error(format!("Could not lay out tearout destination: {error}"));
                return true;
            },
        };
        let Some((_, rect)) = pane.content_rects.iter().find(|(id, _)| *id == tile) else {
            self.chrome_status = ChromeStatus::Error(
                "Tearout destination did not expose its content hole".to_owned(),
            );
            return true;
        };
        self.workspace.set_surface_scale_factor(scale_factor);
        let Some(frame) = self.workspace.frame_tile(tile, *rect) else {
            self.chrome_status = ChromeStatus::Error(
                "Tearout source no longer owns a live document controller".to_owned(),
            );
            return true;
        };
        let id = window.id();
        let pending = PendingTearout {
            tile,
            window: Arc::clone(&window),
            core,
            surface,
            frisket,
            pane_scene: pane.scene,
            frame,
            width,
            height,
            scale_factor,
            disposition,
        };
        self.chrome_status =
            ChromeStatus::Message(format!("Preparing tearout for tile {}", tile.0));
        if disposition == TearoutDisposition::DeclineForReceipt {
            self.tearout_cancellation_hidden_preflight = window.is_visible() == Some(false);
            if !self.tearout_cancellation_hidden_preflight {
                let error =
                    "W4 cancellation destination became visible before its preflight".to_owned();
                self.chrome_status = ChromeStatus::Error(error.clone());
                self.restore_source_after_preflight(tile);
                self.receipt_error = Some(error);
                return true;
            }
        }
        // Hidden Winit windows do not reliably receive a redraw on every
        // platform. Try the preflight synchronously; only a temporarily
        // unavailable swapchain remains pending for an event-driven retry.
        match compose_document_workspace_frame(
            pending.core.as_ref(),
            &pending.surface,
            &pending.pane_scene,
            &pending.frame,
            pending.width,
            pending.height,
            pending.scale_factor,
        ) {
            Ok(true) => self.finish_composed_tearout(id, pending),
            Ok(false) => {
                self.pending_tearouts.insert(id, pending);
                window.request_redraw();
            },
            Err(error) => {
                self.chrome_status = ChromeStatus::Error(format!(
                    "Tearout composition failed before acceptance: {error}"
                ));
                self.restore_source_after_preflight(tile);
            },
        }
        true
    }

    /// CSD only: the border resize direction under the cursor, None when the
    /// window is maximized or still decorated.
    fn csd_resize_edge(&self) -> Option<winit::window::ResizeDirection> {
        if !self.draws_window_controls() {
            return None;
        }
        let window = self.window.as_deref()?;
        if window.is_maximized() {
            return None;
        }
        let (x, y) = self.cursor;
        cambium_genet_winit_host::resize_edge(
            x,
            y,
            self.width as f32 / self.scale_factor,
            self.height as f32 / self.scale_factor,
        )
    }

    /// CSD only: swap in the border resize arrows, deduped on transitions —
    /// an undecorated window gets no resize cursors from the OS.
    fn update_resize_cursor(&mut self) {
        if !self.draws_window_controls() {
            return;
        }
        let direction = self.csd_resize_edge();
        if direction != self.resize_hint {
            self.resize_hint = direction;
            if let Some(window) = self.window.as_deref() {
                window.set_cursor(
                    direction
                        .map(cambium_genet_winit_host::edge_cursor)
                        .unwrap_or(winit::window::CursorIcon::Default),
                );
            }
        }
    }
    /// CSD: publish the maximize button's current layout box to the native
    /// Snap Layout hit-test bridge. Cheap and deduped inside the bridge.
    fn update_snap_bridge(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(bridge) = self.snap_bridge.as_ref() {
            let logical = self
                .frisket
                .chrome_rect("maximize")
                .map(|rect| (rect.x, rect.y, rect.width, rect.height));
            bridge.update(logical, f64::from(self.scale_factor));
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
                redraw |= self.apply_tile_event(event, None);
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
        // CSD: the 8px border band resizes before anything underneath it
        // hits, exactly as an OS frame's grab border would.
        if let Some(direction) = self.csd_resize_edge()
            && let Some(window) = self.window.as_deref()
        {
            let _ = window.drag_resize_window(direction);
            return false;
        }
        match self.frisket.hit(x, y) {
            Some(FrisketHit::ChromeAction(action)) => self.apply_chrome_action(action),
            Some(FrisketHit::Close(tile)) => {
                self.clear_chrome_address();
                self.clear_chrome_engine_menu();
                self.clear_chrome_appearance();
                self.apply_tile_event(TileEvent::Closed(tile), None)
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
                // CSD: chrome with no interactive target is the drag surface.
                if self.draws_window_controls()
                    && let Some(window) = self.window.clone()
                {
                    if self
                        .drag_cadence
                        .press((x, y), cambium_genet_winit_host::Instant::now())
                    {
                        window.set_maximized(!window.is_maximized());
                        self.refresh_chrome();
                        return true;
                    }
                    let _ = window.drag_window();
                }
                changed
            },
            Some(FrisketHit::Appearance) => true,
            None => false,
        }
    }

    fn pointer_up(&mut self) -> bool {
        self.pointer_up_inner(None)
    }

    fn pointer_up_live(&mut self, event_loop: &ActiveEventLoop) -> bool {
        self.pointer_up_inner(Some(event_loop))
    }

    fn pointer_up_inner(&mut self, event_loop: Option<&ActiveEventLoop>) -> bool {
        let gesture = self.gesture.take();
        match gesture {
            Some(PointerGesture::Divider(_)) => true,
            Some(PointerGesture::Tab(drag)) if drag.moved => {
                let to = self.resolve_drop(drag.tile);
                to.is_some_and(|to| {
                    self.apply_tile_event(
                        TileEvent::Dragged {
                            tile: drag.tile,
                            to,
                        },
                        event_loop,
                    )
                })
            },
            Some(PointerGesture::Tab(drag)) => {
                self.apply_tile_event(TileEvent::Activated(drag.tile), None)
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
}

fn a11y_node(tree: &TreeUpdate, label: &str, role: Role) -> Result<AccessNodeId, String> {
    tree.nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(label))
        .map(|(id, _)| *id)
        .ok_or_else(|| format!("accessibility tree has no {role:?} named {label:?}"))
}

fn livery_a11y_node_for_tile(
    tree: &TreeUpdate,
    accessibility: &WorkspaceAccessibility,
    tile: TileId,
    label: &str,
    role: Role,
) -> Result<AccessNodeId, String> {
    tree.nodes
        .iter()
        .find(|(id, node)| {
            node.role() == role
                && node.label() == Some(label)
                && matches!(
                    accessibility.action_for(*id),
                    Some(WorkspaceA11yActionTarget::Livery(target)) if target.tile == tile
                )
        })
        .map(|(id, _)| *id)
        .ok_or_else(|| {
            format!(
                "accessibility tree has no Livery {role:?} named {label:?} in tile {}",
                tile.0
            )
        })
}

#[cfg(feature = "reader")]
fn reader_a11y_node_for_tile(
    tree: &TreeUpdate,
    accessibility: &WorkspaceAccessibility,
    tile: TileId,
    label: &str,
    role: Role,
) -> Result<AccessNodeId, String> {
    tree.nodes
        .iter()
        .find(|(id, node)| {
            node.role() == role
                && node.label() == Some(label)
                && matches!(
                    accessibility.action_for(*id),
                    Some(WorkspaceA11yActionTarget::Reader(target)) if target.tile == tile
                )
        })
        .map(|(id, _)| *id)
        .ok_or_else(|| {
            format!(
                "accessibility tree has no Reader {role:?} named {label:?} in tile {}",
                tile.0
            )
        })
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

/// Compose a document-only workspace frame through one surface of a shared
/// render core. Native surface producers deliberately stay with the source
/// until this host has a tested multi-surface import path.
fn compose_document_workspace_frame(
    core: &RenderCore,
    surface: &WindowSurface,
    pane_scene: &Scene,
    frame: &PeltWorkspaceFrame<Scene>,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> Result<bool, String> {
    if !frame.surfaces.is_empty() {
        return Err(
            "native surface tearout needs a shared-device import receipt before acceptance"
                .to_owned(),
        );
    }
    let (_pane_texture, pane_view) = core.rasterize_scaled(
        pane_scene,
        width,
        height,
        ColorLoad::Clear(wgpu::Color {
            r: 0.10,
            g: 0.10,
            b: 0.12,
            a: 1.0,
        }),
        scale_factor,
    );
    let tile_layers = frame
        .tiles
        .iter()
        .map(|layer| {
            let (tile_width, tile_height) = (
                physical_extent(layer.rect.width, scale_factor),
                physical_extent(layer.rect.height, scale_factor),
            );
            let (texture, view) = core.rasterize_scaled(
                &layer.frame,
                tile_width,
                tile_height,
                ColorLoad::Clear(wgpu::Color::WHITE),
                scale_factor,
            );
            (texture, view, layer.rect)
        })
        .collect::<Vec<_>>();
    let Some(swap) = surface.acquire(core) else {
        return Ok(false);
    };
    let target = swap
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    core.renderer().compose_external_texture(
        &pane_view,
        &target,
        surface.format(),
        width,
        height,
        ExternalTexturePlacement::new([0.0, 0.0, width as f32, height as f32]),
    );
    for (_texture, view, rect) in &tile_layers {
        core.renderer().compose_external_texture(
            view,
            &target,
            surface.format(),
            width,
            height,
            placement(*rect, scale_factor),
        );
    }
    core.queue().present(swap);
    Ok(true)
}

enum TearoutEvent {
    Keep { redraw: bool },
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TearoutDisposition {
    Accept,
    DeclineForReceipt,
}

impl TearoutWindow {
    fn logical_size(&self) -> (u32, u32) {
        (
            static_viewer::logical_extent(self.width, self.scale_factor),
            static_viewer::logical_extent(self.height, self.scale_factor),
        )
    }

    fn refresh_tree(&mut self) {
        debug_assert_eq!(self.workspace.focused_tile(), Some(self.tile));
        self.frisket.set_tree(self.workspace.tree());
        let title = self
            .workspace
            .focused_tile()
            .and_then(|tile| self.workspace.controller(tile));
        self.window.set_title(&static_viewer::pelt_window_title(
            title.and_then(PeltController::title).as_deref(),
            title.map(PeltController::address),
        ));
    }

    fn apply_effect(&mut self, effect: PeltHostEffect) -> bool {
        if let Some(error) = effect.error {
            eprintln!("[pelt-tearout] {error}");
        }
        if let Some(cursor) = effect.cursor {
            self.window.set_cursor(match cursor {
                SessionCursor::Default => winit::window::CursorIcon::Default,
                SessionCursor::Pointer => winit::window::CursorIcon::Pointer,
                SessionCursor::Text => winit::window::CursorIcon::Text,
            });
        }
        self.window.set_ime_allowed(effect.editable);
        if effect.navigated {
            self.refresh_tree();
        }
        effect.redraw || effect.navigated
    }

    fn render(&mut self) -> Result<bool, String> {
        let (logical_width, logical_height) = self.logical_size();
        let pane = self.frisket.frame(logical_width, logical_height)?;
        self.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        self.workspace.set_surface_scale_factor(self.scale_factor);
        let more = self.workspace.pump();
        let frame = self.workspace.frame();
        let composed = compose_document_workspace_frame(
            self.core.as_ref(),
            &self.surface,
            &pane.scene,
            &frame,
            self.width,
            self.height,
            self.scale_factor,
        )?;
        if composed {
            self.workspace.mark_visible_documents_presented();
            if self.visibility_requested {
                self.visible_frame_presented = true;
            }
        }
        Ok(composed && more)
    }

    fn window_event(&mut self, event: WindowEvent) -> TearoutEvent {
        match event {
            WindowEvent::CloseRequested => TearoutEvent::Close,
            WindowEvent::Resized(size) => {
                self.width = size.width.max(1);
                self.height = size.height.max(1);
                self.surface
                    .resize(self.core.as_ref(), self.width, self.height);
                TearoutEvent::Keep { redraw: true }
            },
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
                let size = self.window.inner_size();
                self.width = size.width.max(1);
                self.height = size.height.max(1);
                self.surface
                    .resize(self.core.as_ref(), self.width, self.height);
                TearoutEvent::Keep { redraw: true }
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = SessionModifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    meta: state.super_key(),
                };
                TearoutEvent::Keep { redraw: false }
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (
                    static_viewer::logical_position(position.x as f32, self.scale_factor),
                    static_viewer::logical_position(position.y as f32, self.scale_factor),
                );
                let effect = self.workspace.input(SessionInput::PointerMoved {
                    x: self.cursor.0,
                    y: self.cursor.1,
                    modifiers: self.modifiers,
                });
                TearoutEvent::Keep {
                    redraw: self.apply_effect(effect),
                }
            },
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed {
                    match self.frisket.hit(self.cursor.0, self.cursor.1) {
                        Some(FrisketHit::Tab(tile)) => {
                            let changed = self.workspace.apply(&TileEvent::Activated(tile));
                            if changed {
                                self.refresh_tree();
                            }
                            return TearoutEvent::Keep { redraw: changed };
                        },
                        Some(FrisketHit::Close(tile)) => {
                            if self.workspace.apply(&TileEvent::Closed(tile)) {
                                if self.workspace.tree().tiles().is_empty() {
                                    return TearoutEvent::Close;
                                }
                                self.refresh_tree();
                                return TearoutEvent::Keep { redraw: true };
                            }
                        },
                        _ => {},
                    }
                }
                let effect = self.workspace.input(SessionInput::PointerButton {
                    x: self.cursor.0,
                    y: self.cursor.1,
                    button: SessionPointerButton::Primary,
                    state: button_state(state),
                    modifiers: self.modifiers,
                });
                TearoutEvent::Keep {
                    redraw: self.apply_effect(effect),
                }
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = wheel_delta_from_winit(delta);
                TearoutEvent::Keep {
                    redraw: self.workspace.scroll_at(
                        self.cursor.0,
                        self.cursor.1,
                        dx / self.scale_factor,
                        dy / self.scale_factor,
                    ),
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(command) =
                    navigation_command(&event.logical_key, event.state, self.modifiers)
                {
                    let effect = self.workspace.command(command);
                    return TearoutEvent::Keep {
                        redraw: self.apply_effect(effect),
                    };
                }
                let effect = self.workspace.input(SessionInput::Key {
                    key: session_key(&event.logical_key),
                    state: button_state(event.state),
                    modifiers: self.modifiers,
                    repeat: event.repeat,
                });
                let handled = effect.handled;
                let editable = effect.editable;
                let mut redraw = self.apply_effect(effect);
                if event.state == ElementState::Pressed
                    && !handled
                    && !editable
                    && let Some(key) = scroll_key(&event.logical_key, self.modifiers.shift)
                {
                    redraw |= self.workspace.scroll_for_key(key);
                }
                TearoutEvent::Keep { redraw }
            },
            WindowEvent::Ime(ime) => {
                let effect = self.workspace.input(SessionInput::Ime(session_ime(ime)));
                TearoutEvent::Keep {
                    redraw: self.apply_effect(effect),
                }
            },
            WindowEvent::Focused(focused) => {
                self.native_focus_observed |= focused;
                let effect = self.workspace.input(SessionInput::Focus(focused));
                TearoutEvent::Keep {
                    redraw: self.apply_effect(effect),
                }
            },
            WindowEvent::RedrawRequested => match self.render() {
                Ok(redraw) => TearoutEvent::Keep { redraw },
                Err(error) => {
                    eprintln!("[pelt-tearout] destination render failed: {error}");
                    TearoutEvent::Keep { redraw: false }
                },
            },
            _ => TearoutEvent::Keep { redraw: false },
        }
    }
}

impl WorkspaceApp {
    /// A document frame updates its controller viewport. On a rejected
    /// preflight, restore the source geometry before its next visible frame;
    /// custody, tree membership, controller identity, and model focus have
    /// remained source-owned throughout.
    fn restore_source_after_preflight(&mut self, tile: TileId) {
        self.workspace.set_surface_scale_factor(self.scale_factor);
        if let Some(rect) = self.workspace.content_rect(tile) {
            let _ = self.workspace.frame_tile(tile, rect);
        }
        self.request_redraw();
    }

    /// Resolve a presented native preflight. The cancellation receipt takes
    /// the host-decline branch before the sole source-mutating accept step.
    fn finish_composed_tearout(&mut self, window_id: WindowId, pending: PendingTearout) {
        match pending.disposition {
            TearoutDisposition::Accept => self.accept_composed_tearout(window_id, pending),
            TearoutDisposition::DeclineForReceipt => self.decline_composed_tearout(pending),
        }
    }

    /// Complete the sole source-mutating step after `pending` has presented
    /// through the shared render core. A failure before this call cannot remove
    /// the source tree, controller, or model focus.
    fn accept_composed_tearout(&mut self, window_id: WindowId, pending: PendingTearout) {
        let Some(workspace) = self.workspace.accept_tearout(pending.tile) else {
            self.chrome_status = ChromeStatus::Error(
                "Tearout source changed before destination acceptance".to_owned(),
            );
            self.restore_source_after_preflight(pending.tile);
            return;
        };
        let mut frisket = pending.frisket;
        frisket.set_tree(workspace.tree());
        let mut tearout = TearoutWindow {
            tile: pending.tile,
            window: Arc::clone(&pending.window),
            core: pending.core,
            surface: pending.surface,
            workspace,
            frisket,
            width: pending.width,
            height: pending.height,
            scale_factor: pending.scale_factor,
            cursor: (0.0, 0.0),
            modifiers: SessionModifiers::default(),
            native_focus_observed: false,
            visibility_requested: false,
            visible_frame_presented: false,
        };
        tearout.refresh_tree();
        self.workspace.set_surface_scale_factor(self.scale_factor);
        self.frisket.set_tree(self.workspace.tree());
        if let Some(window) = &self.window {
            window.set_title(&self.window_title());
            window.request_redraw();
        }
        self.chrome_status = ChromeStatus::Ready;
        tearout.window.set_visible(true);
        tearout.visibility_requested = true;
        tearout.window.focus_window();
        tearout.window.request_redraw();
        self.tearouts.insert(window_id, tearout);
    }

    /// A headed receipt host decline after a real hidden native presentation.
    /// Dropping `pending` closes the destination before it ever becomes
    /// visible, while source custody remains in the primary workspace.
    fn decline_composed_tearout(&mut self, pending: PendingTearout) {
        let tile = pending.tile;
        if self.tearout_cancellation_receipt_tile == Some(tile) {
            self.tearout_cancellation_preflight_presented = true;
        }
        self.chrome_status = ChromeStatus::Message(format!(
            "Tearout for tile {} was declined after hidden native preflight",
            tile.0
        ));
        self.restore_source_after_preflight(tile);
    }

    fn refresh_pending_tearout(&mut self, pending: &mut PendingTearout) -> Result<(), String> {
        let logical_width = static_viewer::logical_extent(pending.width, pending.scale_factor);
        let logical_height = static_viewer::logical_extent(pending.height, pending.scale_factor);
        let pane = pending
            .frisket
            .frame(logical_width, logical_height)
            .map_err(|error| format!("could not lay out tearout destination: {error}"))?;
        let (_, rect) = pane
            .content_rects
            .iter()
            .find(|(id, _)| *id == pending.tile)
            .ok_or_else(|| "tearout destination did not expose its content hole".to_owned())?;
        self.workspace
            .set_surface_scale_factor(pending.scale_factor);
        let frame = self
            .workspace
            .frame_tile(pending.tile, *rect)
            .ok_or_else(|| "tearout source no longer owns a live document controller".to_owned())?;
        pending.pane_scene = pane.scene;
        pending.frame = frame;
        Ok(())
    }

    fn fail_pending_tearout(
        &mut self,
        tile: TileId,
        disposition: TearoutDisposition,
        error: String,
    ) {
        self.chrome_status = ChromeStatus::Error(error.clone());
        self.restore_source_after_preflight(tile);
        let is_receipt = match disposition {
            TearoutDisposition::Accept => {
                self.config.tearout_receipt && self.tearout_receipt_tile == Some(tile)
            },
            TearoutDisposition::DeclineForReceipt => {
                self.config.tearout_cancellation_receipt
                    && self.tearout_cancellation_receipt_tile == Some(tile)
            },
        };
        if is_receipt {
            self.receipt_error = Some(error);
        }
    }

    /// Handle a secondary window without permitting it to mutate source
    /// custody until its preflight composition has actually presented.
    fn secondary_window_event(&mut self, window_id: WindowId, event: WindowEvent) -> bool {
        if self.pending_tearouts.contains_key(&window_id) {
            let mut pending = self
                .pending_tearouts
                .remove(&window_id)
                .expect("pending tearout was checked above");
            match event {
                WindowEvent::CloseRequested => {
                    self.fail_pending_tearout(
                        pending.tile,
                        pending.disposition,
                        format!(
                            "Tearout for tile {} was closed before composition",
                            pending.tile.0
                        ),
                    );
                },
                WindowEvent::Resized(size) => {
                    pending.width = size.width.max(1);
                    pending.height = size.height.max(1);
                    pending
                        .surface
                        .resize(pending.core.as_ref(), pending.width, pending.height);
                    match self.refresh_pending_tearout(&mut pending) {
                        Ok(()) => {
                            pending.window.request_redraw();
                            self.pending_tearouts.insert(window_id, pending);
                        },
                        Err(error) => {
                            self.fail_pending_tearout(pending.tile, pending.disposition, error);
                        },
                    }
                },
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    pending.scale_factor = scale_factor as f32;
                    let size = pending.window.inner_size();
                    pending.width = size.width.max(1);
                    pending.height = size.height.max(1);
                    pending
                        .surface
                        .resize(pending.core.as_ref(), pending.width, pending.height);
                    match self.refresh_pending_tearout(&mut pending) {
                        Ok(()) => {
                            pending.window.request_redraw();
                            self.pending_tearouts.insert(window_id, pending);
                        },
                        Err(error) => {
                            self.fail_pending_tearout(pending.tile, pending.disposition, error);
                        },
                    }
                },
                WindowEvent::RedrawRequested => {
                    match compose_document_workspace_frame(
                        pending.core.as_ref(),
                        &pending.surface,
                        &pending.pane_scene,
                        &pending.frame,
                        pending.width,
                        pending.height,
                        pending.scale_factor,
                    ) {
                        Ok(false) => {
                            pending.window.request_redraw();
                            self.pending_tearouts.insert(window_id, pending);
                        },
                        Err(error) => {
                            self.fail_pending_tearout(
                                pending.tile,
                                pending.disposition,
                                format!("Tearout composition failed before acceptance: {error}"),
                            );
                        },
                        Ok(true) => self.finish_composed_tearout(window_id, pending),
                    }
                },
                _ => {
                    self.pending_tearouts.insert(window_id, pending);
                },
            }
            return true;
        }
        let Some(mut tearout) = self.tearouts.remove(&window_id) else {
            return false;
        };
        let native_focus_observed = tearout.native_focus_observed;
        let visible_frame_presented = tearout.visible_frame_presented;
        match tearout.window_event(event) {
            TearoutEvent::Close => {},
            TearoutEvent::Keep { redraw } => {
                if redraw {
                    tearout.window.request_redraw();
                }
                let receipt_progressed = self.tearout_receipt_tile == Some(tearout.tile)
                    && (tearout.native_focus_observed != native_focus_observed
                        || tearout.visible_frame_presented != visible_frame_presented);
                self.tearouts.insert(window_id, tearout);
                if receipt_progressed {
                    // The receipt driver runs from the primary render. A
                    // secondary focus or visible presentation therefore must
                    // explicitly wake the primary instead of relying on
                    // focus-loss/redraw ordering between native windows.
                    self.request_redraw();
                }
            },
        }
        true
    }
}

impl ApplicationHandler for WorkspaceApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attributes =
            static_viewer::pelt_window_attributes(self.window_title(), self.width, self.height)
                .with_visible(false);
        // CSD: the workspace chrome carries the title and caption controls,
        // so the OS frame would be a second title bar. Windows only — see
        // the P6b lane; other platforms keep native decorations for now.
        if cfg!(target_os = "windows") {
            attributes = attributes.with_decorations(false);
        }
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
        if self.config.tearout_receipt || self.config.tearout_cancellation_receipt {
            self.tearout_receipt_started = Some(Instant::now());
        }
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
        // CSD: answer WM_NCHITTEST over the laid-out maximize button so the
        // Windows 11 Snap Layout flyout appears on hover, exactly as the
        // cambium host does for its apps.
        #[cfg(target_os = "windows")]
        {
            match cambium_genet_winit_host::SnapLayoutBridge::attach(&window) {
                Ok(bridge) => self.snap_bridge = Some(bridge),
                Err(error) => {
                    // A missing flyout degrades a nicety, not correctness.
                    eprintln!("pelt: Snap Layout bridge unavailable: {error}");
                },
            }
        }
        self.workspace_receipt_stage_started = Instant::now();
        window.request_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A caption Close can arrive through any dispatch path - the pointer,
        // or an AccessKit Click on the button - and this hook runs after all
        // of them.
        if self.close_requested {
            event_loop.exit();
            return;
        }
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
            if self.secondary_window_event(window_id, event) {
                if self.receipt_error.is_some() {
                    event_loop.exit();
                }
                return;
            }
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
                self.update_resize_cursor();
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
                    ElementState::Released => self.pointer_up_live(event_loop),
                };
                if self.close_requested {
                    event_loop.exit();
                    return;
                }
                if redraw {
                    self.request_redraw();
                }
            },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                // CSD: right-click on the drag surface raises the system menu,
                // the muscle memory an OS title bar provides.
                if self.draws_window_controls()
                    && matches!(
                        self.frisket.hit(self.cursor.0, self.cursor.1),
                        Some(FrisketHit::Chrome)
                    )
                    && let Some(window) = self.window.as_deref()
                {
                    window.show_window_menu(winit::dpi::LogicalPosition::new(
                        f64::from(self.cursor.0),
                        f64::from(self.cursor.1),
                    ));
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
            WindowEvent::RedrawRequested => {
                self.render(event_loop);
                self.update_snap_bridge();
            },
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
