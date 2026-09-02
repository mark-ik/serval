use super::*;
use crate::FileAppearanceStore;
#[cfg(feature = "reader")]
use std::sync::Mutex;
use std::time::Duration;

#[cfg(feature = "reader")]
#[derive(Clone)]
struct CountingResourceFetcher {
    responses: Arc<HashMap<String, genet_host_api::ResourceResponse>>,
    calls: Arc<Mutex<Vec<String>>>,
}

#[cfg(feature = "reader")]
impl ResourceFetcher for CountingResourceFetcher {
    fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        self.fetch_response(url).map(|response| response.bytes)
    }

    fn fetch_response(&self, url: &str) -> Option<genet_host_api::ResourceResponse> {
        self.calls
            .lock()
            .expect("counting resource fetcher call lock")
            .push(url.to_owned());
        self.responses.get(url).cloned()
    }
}

#[cfg(feature = "reader")]
fn reader_test_registries(
    fetcher: CountingResourceFetcher,
    theme: genet_documents::SmolwebTheme,
) -> PeltRegistries<Scene> {
    let mut sessions = SessionRegistry::new();
    sessions.register(Box::new(genet_documents::LiverySessionEngine::new(fetcher)));
    sessions.register(Box::new(genet_documents::ReaderSessionEngine::new(theme)));
    let mut policy = inker::routing::EngineRoutePolicy::default();
    for rule in &mut policy.rules {
        if rule.engine_id == inker::routing::ENGINE_GENET_WEB {
            rule.engine_id = inker::routing::ENGINE_GENET_LIVERY.to_owned();
        }
    }
    policy.fallback.engine_id = inker::routing::ENGINE_GENET_LIVERY.to_owned();
    PeltRegistries::new(
        sessions,
        SurfaceEngineRegistry::new(),
        policy,
        "pelt-reader-test",
        inker::routing::ENGINE_GENET_LIVERY,
        EngineProfileBinding {
            user_data_dir: "pelt-reader-test-profile".to_owned(),
        },
    )
}

#[cfg(feature = "reader")]
fn assert_reader_workspace_receipt(
    receipt: WorkspaceReceipt,
    theme: genet_documents::SmolwebTheme,
    expected_assertion: &str,
) -> Scene {
    assert!(matches!(
        receipt,
        WorkspaceReceipt::Reader | WorkspaceReceipt::TabardReaderPreview
    ));
    let article_url = "https://reader.test/reader/index.html".to_owned();
    let neighbor_url = "https://reader.test/reader/neighbor.html".to_owned();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fetcher = CountingResourceFetcher {
        responses: Arc::new(HashMap::from([
            (
                article_url.clone(),
                genet_host_api::ResourceResponse::new(
                    article_url.clone(),
                    READER_FIXTURE_SOURCE.as_bytes().to_vec(),
                )
                .with_content_type("text/html; charset=utf-8"),
            ),
            (
                neighbor_url.clone(),
                genet_host_api::ResourceResponse::new(
                    neighbor_url.clone(),
                    include_str!("../../examples/workspace/reader/neighbor.html")
                        .as_bytes()
                        .to_vec(),
                )
                .with_content_type("text/html; charset=utf-8"),
            ),
        ])),
        calls: calls.clone(),
    };
    let urls = vec![article_url.clone(), neighbor_url.clone()];
    let tree = tree_from_urls(&urls);
    let workspace = PeltWorkspace::try_routed(
        tree,
        reader_test_registries(fetcher, theme),
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("Reader receipt tree contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("Reader workspace opens its two Livery tiles");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(receipt, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let pane = app.frisket.frame(960, 640).expect("Reader Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };

    compose(&mut app);
    let mut assertion = None;
    for _ in 0..8 {
        assertion = match receipt {
            WorkspaceReceipt::Reader => app.drive_reader_workspace_receipt_step(),
            WorkspaceReceipt::TabardReaderPreview => {
                app.drive_tabard_reader_preview_workspace_receipt_step()
            },
            _ => unreachable!("the assertion above limits the receipt"),
        }
        .expect("Reader semantic receipt");
        compose(&mut app);
        if assertion.is_some() {
            break;
        }
    }
    assert_eq!(assertion.as_deref(), Some(expected_assertion));
    {
        let calls = calls.lock().expect("Reader fetch count lock");
        assert_eq!(calls.len(), 2, "Reader did not fetch a second document");
        assert_eq!(
            calls.iter().filter(|url| *url == &article_url).count(),
            1,
            "the article response was acquired exactly once"
        );
        assert_eq!(
            calls.iter().filter(|url| *url == &neighbor_url).count(),
            1,
            "the neighbor response was acquired exactly once"
        );
    }
    app.workspace
        .frame()
        .tiles
        .into_iter()
        .find(|frame| frame.tile == TileId(1))
        .expect("Reader route retains its visible tile frame")
        .frame
}

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

    let narrow = WorkspaceViewerConfig::new(vec!["narrow.html".to_owned()], WindowingMode::Headed)
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
    let reader = WorkspaceViewerConfig::new(
        vec!["reader.html".to_owned(), "neighbor.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_workspace_receipt(WorkspaceReceipt::Reader, "reader.png");
    assert_eq!(reader.size, Some((960, 640)));
    assert_eq!(reader.frames, Some(3));
    assert!(WorkspaceReceipt::Reader.keeps_chrome());
    let reader_accessibility = WorkspaceViewerConfig::new(
        vec!["reader.html".to_owned(), "reader.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_workspace_receipt(
        WorkspaceReceipt::ReaderAccessibility,
        "reader-accessibility.png",
    );
    assert_eq!(reader_accessibility.size, Some((960, 640)));
    assert_eq!(reader_accessibility.frames, Some(3));
    assert!(WorkspaceReceipt::ReaderAccessibility.keeps_chrome());
    assert_eq!(
        WorkspaceReceipt::ReaderAccessibility.id(),
        "reader-accessibility"
    );

    let accessibility_children =
        WorkspaceViewerConfig::new(vec!["children.html".to_owned()], WindowingMode::Headed)
            .with_workspace_receipt(
                WorkspaceReceipt::AccessibilityChildren,
                "accessibility-children.png",
            );
    assert_eq!(accessibility_children.size, Some((960, 640)));
    assert_eq!(accessibility_children.frames, Some(3));
    assert!(WorkspaceReceipt::AccessibilityChildren.keeps_chrome());
    assert!(WorkspaceReceipt::AccessibilityAddress.keeps_chrome());
    assert_eq!(
        WorkspaceReceipt::AccessibilityAddress.id(),
        "accessibility-address"
    );
    let accessibility_address = WorkspaceViewerConfig::new(
        vec!["address.html".to_owned(), "sibling.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_workspace_receipt(WorkspaceReceipt::AccessibilityAddress, "address.png");
    assert_eq!(accessibility_address.size, Some((960, 640)));
    assert_eq!(accessibility_address.frames, Some(3));
    assert_eq!(
        WorkspaceReceipt::AccessibilityChildren.id(),
        "accessibility-children"
    );
    let accessibility_edit = WorkspaceViewerConfig::new(
        vec!["edit.html".to_owned(), "sibling.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_workspace_receipt(
        WorkspaceReceipt::AccessibilityEdit,
        "accessibility-edit.png",
    );
    assert_eq!(accessibility_edit.size, Some((960, 640)));
    assert_eq!(accessibility_edit.frames, Some(3));
    assert!(WorkspaceReceipt::AccessibilityEdit.keeps_chrome());
    assert_eq!(
        WorkspaceReceipt::AccessibilityEdit.id(),
        "accessibility-edit"
    );
    let accessibility_input = WorkspaceViewerConfig::new(
        vec!["input.html".to_owned(), "sibling.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_workspace_receipt(
        WorkspaceReceipt::AccessibilityInput,
        "accessibility-input.png",
    );
    assert_eq!(accessibility_input.size, Some((960, 640)));
    assert_eq!(accessibility_input.frames, Some(3));
    assert!(WorkspaceReceipt::AccessibilityInput.keeps_chrome());
    assert_eq!(
        WorkspaceReceipt::AccessibilityInput.id(),
        "accessibility-input"
    );

    let tabard = WorkspaceViewerConfig::new(vec!["tabard.html".to_owned()], WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::TabardPreview, "tabard.png");
    assert_eq!(tabard.size, Some((960, 640)));
    assert_eq!(tabard.frames, Some(3));
    assert!(WorkspaceReceipt::TabardPreview.keeps_chrome());
    assert_eq!(WorkspaceReceipt::TabardPreview.id(), "tabard-preview");

    let tabard_reader = WorkspaceViewerConfig::new(
        vec!["reader.html".to_owned(), "neighbor.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_workspace_receipt(WorkspaceReceipt::TabardReaderPreview, "tabard-reader.png");
    assert_eq!(tabard_reader.size, Some((960, 640)));
    assert_eq!(tabard_reader.frames, Some(3));
    assert!(WorkspaceReceipt::TabardReaderPreview.keeps_chrome());
    assert_eq!(
        WorkspaceReceipt::TabardReaderPreview.id(),
        "tabard-reader-preview"
    );
}

#[cfg(feature = "tabard-preview")]
#[test]
fn tabard_preview_stylesheet_is_scoped_to_pelt_chrome() {
    let stylesheet = tabard_preview_stylesheet();
    assert!(stylesheet.contains(".pelt-workspace, .pelt-workspace.pelt-theme-light"));
    assert!(stylesheet.contains("--pelt-chrome-surface: var(--tabard-color-surface)"));
}

#[cfg(feature = "tabard-reader-preview")]
#[test]
fn tabard_reader_preview_maps_portable_roles_to_reader_host_colors() {
    let palette = tabard_preview_theme().palette();
    let genet_documents::SmolwebTheme::App(reader) = tabard_reader_preview_theme() else {
        panic!("Tabard Reader preview must use Reader's host palette seam");
    };
    assert_eq!(reader.bg, tinct::color_to_hex(palette.bg));
    assert_eq!(reader.fg, tinct::color_to_hex(palette.text));
    assert_eq!(reader.link, tinct::color_to_hex(palette.primary));
    assert_eq!(reader.quote, tinct::color_to_hex(palette.text_dim));
    assert_eq!(reader.pre_bg, tinct::color_to_hex(palette.surface_2));
    assert_ne!(
        reader.bg, "#fbfaf7",
        "the preview is not Reader's system fallback"
    );
}

#[cfg(feature = "tabard-reader-preview")]
#[test]
fn tabard_reader_preview_only_selects_the_reader_theme_for_its_receipt() {
    let preview = WorkspaceEngineOptions::for_receipt(Some(WorkspaceReceipt::TabardReaderPreview));
    assert_eq!(preview.reader_theme, Some(tabard_reader_preview_theme()));
    assert_eq!(
        WorkspaceEngineOptions::for_receipt(Some(WorkspaceReceipt::Reader)).reader_theme,
        None
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
    assert!(p3.contains("P3, P4, or W4"));
    let p4 = run_livery_workspace_viewer(fallback().with_capability_receipt())
        .expect_err("P4 and P5 receipt drivers are mutually exclusive");
    assert!(p4.contains("P3, P4, or W4"));
}

#[cfg(feature = "reader")]
#[test]
fn reader_workspace_reuses_the_held_livery_response_without_refetching() {
    assert_reader_workspace_receipt(
        WorkspaceReceipt::Reader,
        genet_documents::SmolwebTheme::default(),
        READER_WORKSPACE_ASSERTION,
    );
}

#[cfg(feature = "tabard-reader-preview")]
#[test]
fn tabard_reader_preview_reuses_the_held_response_and_renders_its_palette() {
    let scene = assert_reader_workspace_receipt(
        WorkspaceReceipt::TabardReaderPreview,
        tabard_reader_preview_theme(),
        TABARD_READER_PREVIEW_WORKSPACE_ASSERTION,
    );
    let palette = tabard_preview_theme().palette();
    let color = |color: tinct::Srgb| {
        [
            f32::from(color.r) / f32::from(u8::MAX),
            f32::from(color.g) / f32::from(u8::MAX),
            f32::from(color.b) / f32::from(u8::MAX),
            f32::from(color.a) / f32::from(u8::MAX),
        ]
    };
    let background = color(palette.bg);
    let text = color(palette.text);
    let link = color(palette.primary);
    assert!(matches!(
        scene.ops.first(),
        Some(netrender::SceneOp::Rect(rect)) if rect.color == background
    ));
    assert!(
        scene.ops.iter().any(|operation| {
            matches!(operation, netrender::SceneOp::GlyphRun(run) if run.color == text)
        }),
        "Reader did not render Tabard's body text color"
    );
    assert!(
        scene.ops.iter().any(|operation| {
            matches!(operation, netrender::SceneOp::GlyphRun(run) if run.color == link)
        }),
        "Reader did not render Tabard's link color"
    );
}

#[test]
fn desktop_keeps_source_when_tearout_has_no_native_destination() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p5-fallback/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("tearout fixture contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("tearout fixture opens its Pelt session");
    let before = workspace.tree().clone();
    let focused_before = workspace.focused_tile();
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_tearout_cancellation_receipt()
        .with_workspace_receipt_stage_timeout(Duration::from_millis(1));
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);

    assert!(app.apply_tile_event(
        TileEvent::Dragged {
            tile: TileId(1),
            to: DropTarget::Outside,
        },
        None,
    ));
    assert_eq!(app.workspace.tree(), &before);
    assert!(app.workspace.controller(TileId(1)).is_some());
    assert_eq!(app.workspace.focused_tile(), focused_before);
    assert_eq!(
        app.chrome_status,
        ChromeStatus::Message(
            "Tearout requested for tile 1; a native destination is not available in this dispatch"
                .to_owned()
        )
    );
    app.tearout_receipt_started = Some(Instant::now() - Duration::from_millis(2));
    assert!(app.tearout_receipt_timeout_error().is_some_and(|error| {
        error.contains("W4 cancellation receipt timed out")
            && error.contains("focus_observed=false")
            && error.contains("visible_frame_presented=false")
    }));
    app.redraws = app
        .config
        .frames
        .expect("bounded receipt has a frame count");
    assert!(
        !app.generic_frame_limit_reached(),
        "the named cancellation receipt uses its stage timeout, not the generic frame cap"
    );
    app.tearout_cancellation_receipt_tile = Some(TileId(1));
    app.record_tearout_receipt_close("primary window", None, false);
    assert!(app.receipt_error.as_deref().is_some_and(|error| {
        error.contains("primary window") && error.contains("source custody was still retained")
    }));
}

#[test]
fn tearout_render_error_has_a_retained_alert_document() {
    let chrome = tearout_error_chrome(
        TileId(7),
        "https://example.test/live".to_owned(),
        WorkspaceRect::new(20.0, 40.0, 600.0, 320.0),
        "the imported surface fence failed".to_owned(),
    );
    assert_eq!(chrome.title, "Tearout rendering stopped");
    assert_eq!(chrome.status, "the imported surface fence failed");
    assert!(matches!(
        chrome.diagnostic,
        Some(ChromeDocument {
            kind: ChromeDocumentKind::Error,
            tile: TileId(7),
            message: Some(ref message),
            ..
        }) if message.contains("fence")
    ));
}

#[test]
fn secondary_visible_startup_retries_until_presented() {
    assert!(secondary_redraw_needed(false, true, false));
    assert!(!secondary_redraw_needed(false, true, true));
    assert!(secondary_redraw_needed(true, true, true));
}

#[test]
fn shown_preflight_counts_as_the_first_visible_destination_frame() {
    assert!(visible_preflight_presented(true, Some(true)));
    assert!(!visible_preflight_presented(true, Some(false)));
    assert!(!visible_preflight_presented(false, Some(true)));
}

#[cfg(target_os = "windows")]
#[test]
fn pending_surface_preflight_keeps_the_destination_cache_unclaimed() {
    // `compose_document_workspace_frame` calls `claim_source_cache` only
    // after WindowSurface::acquire returned a swapchain frame. This is the
    // pending/Ok(false) state, so the source cache remains sampleable.
    let receipt = SurfaceTearoutImportReceipt {
        tile: TileId(1),
        cache: None,
    };
    assert!(receipt.cache.is_none());
}

#[test]
fn cancellation_receipt_configures_a_separate_bounded_headed_run() {
    let config = WorkspaceViewerConfig::new(
        vec!["source.html".to_owned(), "sibling.html".to_owned()],
        WindowingMode::Headed,
    )
    .with_tearout_cancellation_receipt();
    assert!(config.tearout_cancellation_receipt);
    assert!(!config.tearout_receipt);
    assert!(!config.chrome);
    assert_eq!(config.frames, Some(180));
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
    assert_eq!(app.chrome_theme(), AppearanceTheme::Light);
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
    assert_eq!(app.chrome_theme(), AppearanceTheme::Light);
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
fn file_appearance_store_restores_theme_after_workspace_recreation() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p6-appearance/index.html")
        .to_string_lossy()
        .into_owned();
    let path = std::env::temp_dir().join(format!(
        "pelt-workspace-appearance-{}-{}.theme",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&path);

    let make_app = |store: FileAppearanceStore| {
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
                    unreachable!("appearance store test contains only documents");
                };
                Ok(PeltTileRequest::new(address, (960, 640)))
            },
            || Box::new(WorkspaceClock(Instant::now())),
        )
        .expect("appearance store test opens its seed document");
        let frisket = FrisketSurface::new(workspace.tree());
        let config = WorkspaceViewerConfig::new(vec![fixture.clone()], WindowingMode::Headed)
            .with_size(960, 640)
            .with_appearance_store(store);
        #[cfg(target_os = "windows")]
        {
            WorkspaceApp::new(config, workspace, frisket, None)
        }
        #[cfg(not(target_os = "windows"))]
        {
            WorkspaceApp::new(config, workspace, frisket)
        }
    };
    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let pane = app
            .frisket
            .frame(960, 640)
            .expect("appearance store Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };

    let mut app = make_app(FileAppearanceStore::load(&path).unwrap());
    compose(&mut app);
    let tile = TileId(1);
    let baseline_content = app
        .workspace
        .content_rect(tile)
        .expect("appearance store test retains its content hole");
    let baseline_address = app
        .workspace
        .controller(tile)
        .expect("appearance store test retains its document")
        .address()
        .to_owned();
    let baseline_route = app
        .workspace
        .route(tile)
        .cloned()
        .expect("appearance store test retains its route");
    let baseline_history = app
        .workspace
        .controller(tile)
        .map(|controller| (controller.can_go_back(), controller.can_go_forward()))
        .expect("appearance store test retains history");

    assert_eq!(app.chrome_theme(), AppearanceTheme::Dark);
    assert!(app.apply_chrome_action(ChromeAction::ToggleAppearance));
    assert!(app.apply_chrome_action(ChromeAction::ChooseTheme(AppearanceTheme::Light)));
    assert_eq!(app.chrome_theme(), AppearanceTheme::Light);
    assert!(app.chrome_appearance().persistent);
    drop(app);

    let mut restored = make_app(FileAppearanceStore::load(&path).unwrap());
    assert_eq!(restored.chrome_theme(), AppearanceTheme::Light);
    compose(&mut restored);
    assert_eq!(restored.chrome_theme(), AppearanceTheme::Light);
    assert_eq!(
        restored.workspace.content_rect(tile),
        Some(baseline_content)
    );
    assert_eq!(
        restored
            .workspace
            .controller(tile)
            .expect("restored workspace retains its document")
            .address(),
        baseline_address
    );
    assert_eq!(restored.workspace.route(tile), Some(&baseline_route));
    assert_eq!(
        restored
            .workspace
            .controller(tile)
            .map(|controller| (controller.can_go_back(), controller.can_go_forward())),
        Some(baseline_history)
    );
    assert!(restored.apply_chrome_action(ChromeAction::ToggleAppearance));
    assert_eq!(
        restored.chrome_model().appearance,
        Some(ChromeAppearance {
            theme: AppearanceTheme::Light,
            persistent: true,
        })
    );
    let _ = std::fs::remove_file(path);
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
    let address = a11y_node(&initial, "Address", Role::TextInput).expect("address input");
    let initial_address_node = initial
        .nodes
        .iter()
        .find(|(id, _)| *id == address)
        .map(|(_, node)| node)
        .expect("initial address node");
    assert!(initial_address_node.supports_action(Action::SetValue));
    assert_eq!(initial_address_node.value(), Some(fixture.as_str()));
    let theme = a11y_node(&initial, "Toggle Pelt appearance settings", Role::Button)
        .expect("appearance toggle");
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Focus,
        target_node: theme,
        data: None,
    }));
    assert_eq!(app.chrome_theme(), AppearanceTheme::Dark);
    assert!(!app.chrome_appearance_open);
    assert!(app.accessibility.focus.is_some());

    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Click,
        target_node: theme,
        data: None,
    }));
    assert!(app.chrome_appearance_open);
    let drawer = app
        .prepare_accessibility_tree()
        .expect("appearance accessibility tree");
    let light = a11y_node(&drawer, "Light", Role::RadioButton).expect("Light radio");
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Focus,
        target_node: light,
        data: None,
    }));
    assert_eq!(app.chrome_theme(), AppearanceTheme::Dark);

    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Click,
        target_node: light,
        data: None,
    }));
    assert_eq!(app.chrome_theme(), AppearanceTheme::Light);
    let selected = app
        .prepare_accessibility_tree()
        .expect("selected appearance accessibility tree");
    let current_address =
        a11y_node(&selected, "Address", Role::TextInput).expect("current address input");
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
        Some(WorkspaceA11yFocus::Frisket(
            FrisketA11yTarget::ChromeAction(ChromeAction::ChooseTheme(AppearanceTheme::Light))
        ))
    );
    assert_eq!(
        app.workspace
            .controller(TileId(1))
            .expect("focused document survives chrome action")
            .address(),
        fixture
    );

    let next_fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p6-load-error/next.html")
        .to_string_lossy()
        .into_owned();
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: current_address,
        data: Some(ActionData::Value(next_fixture.clone().into())),
    }));
    assert_eq!(
        app.workspace
            .controller(TileId(1))
            .expect("address destination")
            .address(),
        next_fixture
    );
    let replaced = app
        .prepare_accessibility_tree()
        .expect("replaced accessibility tree");
    let replaced_address =
        a11y_node(&replaced, "Address", Role::TextInput).expect("replaced address input");
    let current_tab = replaced
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Tab && node.is_selected() == Some(true))
        .map(|(id, _)| *id)
        .expect("current tab");
    let current_address_node = replaced
        .nodes
        .iter()
        .find(|(id, _)| *id == replaced_address)
        .map(|(_, node)| node)
        .expect("replaced address node");
    assert!(current_address_node.supports_action(Action::SetValue));
    assert_eq!(current_address_node.value(), Some(next_fixture.as_str()));
    assert_eq!(
        replaced
            .nodes
            .iter()
            .find(|(id, _)| *id == replaced_address)
            .map(|(_, node)| node.value()),
        Some(Some(next_fixture.as_str()))
    );
    assert!(replaced_address != address);
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: address,
        data: Some(ActionData::Value(fixture.clone().into())),
    }));
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: replaced_address,
        data: Some(ActionData::NumericValue(4.0)),
    }));
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: replaced_address,
        data: None,
    }));
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: current_tab,
        data: Some(ActionData::Value(next_fixture.clone().into())),
    }));
    assert_eq!(
        app.workspace
            .controller(TileId(1))
            .expect("destination remains")
            .address(),
        next_fixture
    );
    let missing_fixture = next_fixture.clone() + ".missing";
    let history_before_missing = app
        .workspace
        .controller(TileId(1))
        .expect("controller before missing navigation")
        .can_go_back();
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: replaced_address,
        data: Some(ActionData::Value(missing_fixture.clone().into())),
    }));
    assert_eq!(
        app.workspace
            .controller(TileId(1))
            .expect("failed navigation preserves controller")
            .address(),
        next_fixture
    );
    assert_eq!(
        app.workspace
            .controller(TileId(1))
            .expect("failed navigation controller")
            .can_go_back(),
        history_before_missing
    );
    assert!(matches!(
        app.workspace
            .controller(TileId(1))
            .expect("failed navigation state")
            .document_state(),
        PeltDocumentState::Error { address, message }
            if address == &missing_fixture && !message.is_empty()
    ));
}

#[test]
fn document_child_accessibility_allocator_never_aliases_local_ids() {
    let mut accessibility = WorkspaceAccessibility::new();
    let shell_ids = HashSet::from([AccessNodeId(1_u64 << 63)]);
    let local = AccessNodeId(41);
    let initial = PeltSessionIdentity {
        instance_id: 1,
        generation: 1,
    };
    let replacement = PeltSessionIdentity {
        instance_id: 1,
        generation: 2,
    };
    let reconstructed = PeltSessionIdentity {
        instance_id: 2,
        generation: 1,
    };
    let first = accessibility.child_global_id(TileId(1), initial, local, &shell_ids);
    let same_session = accessibility.child_global_id(TileId(1), initial, local, &shell_ids);
    let sibling = accessibility.child_global_id(TileId(2), initial, local, &shell_ids);
    let replacement = accessibility.child_global_id(TileId(1), replacement, local, &shell_ids);
    let reconstructed = accessibility.child_global_id(TileId(1), reconstructed, local, &shell_ids);

    assert_eq!(first, same_session);
    assert_ne!(first, sibling);
    assert_ne!(first, replacement);
    assert_ne!(replacement, reconstructed);
    assert!(!shell_ids.contains(&first));
    assert!(accessibility.child_id_is_reserved(first));
}

#[test]
fn secondary_accessibility_projection_keeps_tile_namespace_and_virtual_focus() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p6-accessibility/index.html")
        .to_string_lossy()
        .into_owned();
    let tree = tree_from_urls(std::slice::from_ref(&fixture));
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let mut workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("secondary accessibility fixture contains a document tile");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("secondary accessibility fixture opens its document");
    let mut frisket = FrisketSurface::new(workspace.tree());
    frisket.set_content_accessibility([FrisketContentA11y {
        tile: TileId(1),
        label: "Tile 1 content".to_owned(),
        description: "Secondary content".to_owned(),
    }]);
    let pane = frisket.frame(960, 640).expect("secondary Frisket frame");
    workspace.set_content_rects(pane.content_rects.iter().copied());
    let _ = workspace.pump();
    let _ = workspace.frame();
    workspace.mark_visible_documents_presented();
    let mut accessibility = WorkspaceAccessibility::new();
    let first = secondary_accessibility_projection(&mut accessibility, &frisket, &workspace)
        .expect("secondary composite accessibility projection");
    let aperture = first
        .tree
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Region && node.label() == Some("Tile 1 content"))
        .map(|(id, node)| (*id, node.children().to_vec()))
        .expect("secondary tile content aperture");
    assert_eq!(
        aperture.1.len(),
        1,
        "document root is attached below the tile"
    );
    let child_id = aperture.1[0];
    assert!(
        child_id.0 >= 1_u64 << 63,
        "child IDs stay out of shell range"
    );
    let (action_id, target) = first
        .actions
        .iter()
        .find_map(|(id, target)| {
            let WorkspaceA11yActionTarget::Frisket(target) = target else {
                return None;
            };
            let supports_focus = first
                .tree
                .nodes
                .iter()
                .find(|(candidate, _)| candidate == id)
                .is_some_and(|(_, node)| node.supports_action(Action::Focus));
            supports_focus
                .then(|| frisket.accessibility_target(*target))
                .flatten()
                .map(|target| (*id, target))
        })
        .expect("secondary shell has a typed action target");

    assert!(accessibility.set_focus(WorkspaceA11yFocus::Frisket(target)));
    let second = secondary_accessibility_projection(&mut accessibility, &frisket, &workspace)
        .expect("secondary projection after virtual focus");
    assert_eq!(second.tree.nodes.len(), first.tree.nodes.len());
    let prepared = accessibility.prepare(second, 1.0);
    assert_eq!(prepared.focus, action_id);
    let second_aperture = prepared
        .nodes
        .iter()
        .find(|(id, _)| *id == aperture.0)
        .expect("secondary aperture remains stable");
    assert_eq!(second_aperture.1.children(), &[child_id]);
}

#[cfg(feature = "reader")]
#[test]
fn reader_child_accessibility_is_namespaced_virtual_and_stale_safe() {
    let article_url = "https://reader.test/p7-reader-accessibility/index.html".to_owned();
    let response = genet_host_api::ResourceResponse::new(
        article_url.clone(),
        READER_ACCESSIBILITY_FIXTURE_SOURCE.as_bytes().to_vec(),
    )
    .with_content_type("text/html; charset=utf-8");
    let fetcher = CountingResourceFetcher {
        responses: Arc::new(HashMap::from([(article_url.clone(), response.clone())])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let urls = vec![article_url.clone(), article_url.clone()];
    let tree = tree_from_urls(&urls);
    let workspace = PeltWorkspace::try_routed(
        tree,
        reader_test_registries(fetcher, genet_documents::SmolwebTheme::default()),
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("Reader accessibility tree contains only documents");
            };
            Ok(held_response_request(response.clone(), address, (960, 640))
                .with_engine_override(inker::routing::ENGINE_GENET_READER))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("Reader accessibility fixtures open from their held bodies");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::ReaderAccessibility, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    // The headed receipt uses its 960x640 physical contract on this
    // Windows host at 2x. Keep the GPU-free tree proof inside that same
    // pair of narrow Reader content holes.
    app.scale_factor = 2.0;

    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let (width, height) = app.logical_size();
        let pane = app
            .frisket
            .frame(width, height)
            .expect("Reader accessibility Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };
    fn node(tree: &TreeUpdate, id: AccessNodeId) -> &AccessNode {
        tree.nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, node)| node)
            .expect("Reader link remains in the composite tree")
    }

    compose(&mut app);
    let initial = app
        .prepare_accessibility_tree()
        .expect("initial Reader composite accessibility tree");
    let first_link = reader_a11y_node_for_tile(
        &initial,
        &app.accessibility,
        TileId(1),
        "Continue through the retained article source",
        Role::Link,
    )
    .expect("first Reader link");
    let second_link = reader_a11y_node_for_tile(
        &initial,
        &app.accessibility,
        TileId(2),
        "Continue through the retained article source",
        Role::Link,
    )
    .expect("second Reader link");
    assert_ne!(
        first_link, second_link,
        "Pelt gives duplicated Reader link-local IDs separate workspace IDs"
    );
    let shell = app
        .frisket
        .accessibility_projection(None)
        .expect("Reader accessibility Frisket shell projection");
    let child_root = |tile, link| {
        let aperture = *shell
            .content_nodes
            .get(&tile)
            .expect("Reader tile has one Frisket content aperture");
        let root = initial
            .nodes
            .iter()
            .find(|(candidate, _)| *candidate == aperture)
            .and_then(|(_, node)| node.children().first().copied())
            .expect("Reader aperture owns its partial document root");
        let root_node = initial
            .nodes
            .iter()
            .find(|(candidate, _)| *candidate == root)
            .map(|(_, node)| node)
            .expect("Reader partial document root is retained");
        assert_eq!(root_node.role(), Role::Document);
        assert!(
            root_node.children().contains(&link),
            "Reader link stays below its tile-local document root"
        );
        root
    };
    assert_ne!(
        child_root(TileId(1), first_link),
        child_root(TileId(2), second_link),
        "Pelt grafts each Reader snapshot below its own Frisket aperture"
    );
    for link in [first_link, second_link] {
        let node = node(&initial, link);
        assert!(node.supports_action(Action::Focus));
        assert!(
            !node.supports_action(Action::Click) && !node.supports_action(Action::SetValue),
            "Reader exposes only virtual Focus before host source handoff owns navigation"
        );
    }
    let first_action = match app
        .accessibility
        .action_for(first_link)
        .expect("first Reader link has a Pelt action")
    {
        WorkspaceA11yActionTarget::Document(action) => action,
        WorkspaceA11yActionTarget::Frisket(_) => {
            panic!("first Reader link must retain document ownership")
        },
    };
    let second_action = match app
        .accessibility
        .action_for(second_link)
        .expect("second Reader link has a Pelt action")
    {
        WorkspaceA11yActionTarget::Document(action) => action,
        WorkspaceA11yActionTarget::Frisket(_) => {
            panic!("second Reader link must retain document ownership")
        },
    };
    assert_eq!(first_action.tile, TileId(1));
    assert_eq!(second_action.tile, TileId(2));
    assert_eq!(
        first_action.local_node, second_action.local_node,
        "the fixture deliberately collides Reader-local IDs across tile sessions"
    );
    assert!(
        first_action.supports(DocumentA11yAction::Focus)
            && second_action.supports(DocumentA11yAction::Focus)
    );
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Focus,
        target_node: first_link,
        data: None,
    }));
    assert_eq!(
        app.accessibility.focus,
        Some(WorkspaceA11yFocus::Document(first_link)),
        "Reader Focus remains virtual in Pelt's one-tree bridge"
    );
    for tile in [TileId(1), TileId(2)] {
        let controller = app
            .workspace
            .controller(tile)
            .expect("Reader tile remains live after virtual Focus");
        assert_eq!(controller.address(), article_url);
        assert_eq!(controller.session_generation(), 1);
        assert!(
            controller
                .session_as_any_ref()
                .is::<genet_documents::ReaderDocumentSession>(),
            "virtual Focus does not replace Reader tile {}",
            tile.0
        );
    }
    assert!(
        !app.workspace.has_active_pointer_capture(),
        "virtual Reader Focus does not enter the physical pointer path"
    );

    assert!(
        app.workspace
            .set_route_override(
                TileId(1),
                Some(inker::routing::ENGINE_GENET_LIVERY.to_owned()),
            )
            .expect("held Reader source can reconstruct a Livery session"),
        "the first route changes from Reader to Livery"
    );
    app.frisket.set_tree(app.workspace.tree());
    let after_replacement = app
        .prepare_accessibility_tree()
        .expect("engine-replaced accessibility tree");
    assert!(
        app.accessibility.action_for(first_link).is_none(),
        "an old Reader child ID is removed when the tile's engine source changes"
    );
    assert_eq!(
        app.accessibility.focus, None,
        "a replaced Reader child cannot retain virtual focus"
    );
    assert!(
        !app.apply_accessibility_request(A11yActionRequest {
            action: Action::Focus,
            target_node: first_link,
            data: None,
        }),
        "the stale Reader Focus action is inert after engine replacement"
    );
    assert!(
        app.workspace
            .controller(TileId(1))
            .is_some_and(|controller| controller
                .session_as_any_ref()
                .is::<genet_documents::LiveryDocumentSession>()),
        "the replaced tile now belongs to Livery"
    );
    let sibling = app
        .workspace
        .controller(TileId(2))
        .expect("sibling Reader controller survives replacement");
    assert_eq!(sibling.address(), article_url);
    assert_eq!(sibling.session_generation(), 1);
    assert!(
        sibling
            .session_as_any_ref()
            .is::<genet_documents::ReaderDocumentSession>()
    );
    assert_eq!(
        reader_a11y_node_for_tile(
            &after_replacement,
            &app.accessibility,
            TileId(2),
            "Continue through the retained article source",
            Role::Link,
        )
        .expect("sibling Reader child stays attached"),
        second_link,
        "a sibling Reader namespace survives another tile's source replacement"
    );
}

#[test]
fn livery_child_accessibility_is_namespaced_transformed_and_stale_safe() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p7-accessibility-children/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture.clone(), fixture.clone()];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("child accessibility tree contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("child accessibility fixtures open");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::Accessibility, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);

    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let (width, height) = app.logical_size();
        let pane = app
            .frisket
            .frame(width, height)
            .expect("child accessibility Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };
    let subtree = |tree: &TreeUpdate, root: AccessNodeId| {
        let mut found = HashSet::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !found.insert(id) {
                continue;
            }
            if let Some((_, node)) = tree.nodes.iter().find(|(candidate, _)| *candidate == id) {
                pending.extend(node.children().iter().copied());
            }
        }
        found
    };
    let child_root = |tree: &TreeUpdate, aperture: AccessNodeId| {
        tree.nodes
            .iter()
            .find(|(id, _)| *id == aperture)
            .and_then(|(_, node)| node.children().first().copied())
            .expect("Livery content aperture owns its one child root")
    };

    compose(&mut app);
    let initial = app
        .prepare_accessibility_tree()
        .expect("initial composite accessibility tree");
    let shell = app
        .frisket
        .accessibility_projection(None)
        .expect("Frisket shell projection");
    let aperture_one = *shell
        .content_nodes
        .get(&TileId(1))
        .expect("first content aperture");
    let aperture_two = *shell
        .content_nodes
        .get(&TileId(2))
        .expect("second content aperture");
    let root_one = child_root(&initial, aperture_one);
    let root_two = child_root(&initial, aperture_two);
    assert_ne!(
        root_one, root_two,
        "Pelt remaps colliding local roots into distinct workspace IDs"
    );
    assert!(
        root_one.0 >= 1_u64 << 63 && root_two.0 >= 1_u64 << 63,
        "the host owns a distinct allocation range for child IDs"
    );

    {
        let controller = app
            .workspace
            .controller_mut(TileId(1))
            .expect("first child controller");
        let state = controller
            .set_page_zoom(1.25)
            .expect("Livery accepts the child-tree zoom test");
        assert_eq!(state.applied, 1.25);
    }
    compose(&mut app);
    let content = app
        .workspace
        .content_rect(TileId(1))
        .expect("first child content hole");
    assert!(
        app.workspace.scroll_at(
            content.x + content.width / 2.0,
            content.y + content.height / 2.0,
            0.0,
            600.0,
        ),
        "the long child document accepts root scrolling"
    );
    compose(&mut app);
    let tree = app
        .prepare_accessibility_tree()
        .expect("zoomed and scrolled composite tree");
    let root_one = child_root(&tree, aperture_one);
    let scroll = {
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("first child controller after scroll");
        let session = controller
            .session_as_any_ref()
            .downcast_ref::<genet_documents::LiveryDocumentSession>()
            .expect("first child remains a Livery session");
        session.document().scroll()
    };
    assert!(
        scroll.1 > 0.0,
        "root scroll becomes part of the child transform"
    );
    // The document session publishes final viewport-space bounds after page
    // zoom and root scroll. Pelt contributes only the outer content-hole
    // placement at the subtree root.
    let expected_transform = Affine::translate((f64::from(content.x), f64::from(content.y)));
    let child_root_node = tree
        .nodes
        .iter()
        .find(|(id, _)| *id == root_one)
        .map(|(_, node)| node)
        .expect("first remapped child root is present");
    assert_eq!(child_root_node.transform(), Some(&expected_transform));

    let first_subtree = subtree(&tree, root_one);
    let link = tree
        .nodes
        .iter()
        .find(|(id, node)| {
            first_subtree.contains(id)
                && node.role() == Role::Link
                && node.label() == Some("Open scrolled child destination")
        })
        .map(|(id, _)| *id)
        .expect("visible scrolled Livery link is in tile one's child tree");
    let action = match app
        .accessibility
        .action_for(link)
        .expect("child link has a Pelt action target")
    {
        WorkspaceA11yActionTarget::Document(action) => action,
        WorkspaceA11yActionTarget::Frisket(_) => {
            panic!("child link must not use Frisket action routing")
        },
    };
    assert_eq!(action.tile, TileId(1));
    assert_eq!(action.session_identity.generation, 1);
    let click_point = app
        .workspace
        .controller(TileId(1))
        .and_then(|controller| controller.accessibility_click_target(action.local_node))
        .map(|point| {
            (
                action.content_rect.x + point.point.x,
                action.content_rect.y + point.point.y,
            )
        })
        .expect("visible child link advertises a tile-local click");
    assert!(
        action.content_rect.contains(click_point.0, click_point.1),
        "the transformed child click remains inside its own content hole"
    );
    let (css_point, point_zoom) = {
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("first child controller exposes its live point");
        let session = controller
            .session_as_any_ref()
            .downcast_ref::<genet_documents::LiveryDocumentSession>()
            .expect("first child remains a Livery session");
        (
            session
                .accessible_pointer_target(action.local_node.get())
                .expect("visible child link has a clip-aware CSS point"),
            session.page_zoom(),
        )
    };
    let expected_click_point = (
        content.x + css_point.0 * point_zoom,
        content.y + css_point.1 * point_zoom,
    );
    assert!(
        (click_point.0 - expected_click_point.0).abs() < 0.01
            && (click_point.1 - expected_click_point.1).abs() < 0.01,
        "Livery returns a viewport CSS point, so Pelt applies page zoom and the content origin exactly once"
    );

    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Focus,
        target_node: link,
        data: None,
    }));
    assert_eq!(
        app.accessibility.focus,
        Some(WorkspaceA11yFocus::Document(link)),
        "Focus changes only Pelt's virtual child focus"
    );
    assert!(
        app.workspace.scroll_at(
            content.x + content.width / 2.0,
            content.y + content.height / 2.0,
            0.0,
            64.0,
        ),
        "a later root wheel moves the live child without replacing its session"
    );
    let (fresh_css_point, fresh_scroll, fresh_zoom) = {
        let controller = app
            .workspace
            .controller(TileId(1))
            .expect("first child controller remains live after a wheel");
        let session = controller
            .session_as_any_ref()
            .downcast_ref::<genet_documents::LiveryDocumentSession>()
            .expect("first child remains a Livery session after a wheel");
        (
            session
                .accessible_pointer_target(action.local_node.get())
                .expect("the still-visible child link has a fresh CSS point"),
            session.document().scroll(),
            session.page_zoom(),
        )
    };
    assert_ne!(
        fresh_scroll, scroll,
        "the queued action spans a real root-scroll geometry change"
    );
    let fresh_click_point = (
        content.x + fresh_css_point.0 * fresh_zoom,
        content.y + fresh_css_point.1 * fresh_zoom,
    );
    assert!(
        (fresh_click_point.0 - click_point.0).abs() > 0.01
            || (fresh_click_point.1 - click_point.1).abs() > 0.01,
        "the fresh CSS point differs from the point published in the old tree"
    );
    assert!(
        !app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: link,
            data: None,
        }),
        "the projection revision makes a pre-scroll Click inert"
    );
    let refreshed = app
        .prepare_accessibility_tree()
        .expect("root-scroll geometry is reprojected before Click");
    let refreshed_root = child_root(&refreshed, aperture_one);
    let refreshed_subtree = subtree(&refreshed, refreshed_root);
    let refreshed_link = refreshed
        .nodes
        .iter()
        .find(|(id, node)| {
            refreshed_subtree.contains(id)
                && node.role() == Role::Link
                && node.label() == Some("Open scrolled child destination")
        })
        .map(|(id, _)| *id)
        .expect("current root-scroll projection retains the link");
    assert_eq!(
        refreshed_link, link,
        "a semantic node keeps its global identity across projection revisions"
    );
    assert!(
        app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: refreshed_link,
            data: None,
        }),
        "the refreshed Click uses Livery's current clip-aware point"
    );
    let controller = app
        .workspace
        .controller(TileId(1))
        .expect("child link leaves its tile live");
    assert!(
        controller
            .address()
            .replace('\\', "/")
            .ends_with("/next.html"),
        "Click follows Pelt's normal document navigation path"
    );
    assert_eq!(controller.session_generation(), 2);
    assert!(
        !app.apply_accessibility_request(A11yActionRequest {
            action: Action::Focus,
            target_node: link,
            data: None,
        }) && !app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: link,
            data: None,
        }),
        "the old child ID is inert after its session replacement"
    );

    compose(&mut app);
    let replacement = app
        .prepare_accessibility_tree()
        .expect("replacement child tree");
    let replacement_shell = app
        .frisket
        .accessibility_projection(None)
        .expect("replacement Frisket shell projection");
    let replacement_aperture = *replacement_shell
        .content_nodes
        .get(&TileId(1))
        .expect("replacement first content aperture");
    let replacement_root = child_root(&replacement, replacement_aperture);
    let replacement_subtree = subtree(&replacement, replacement_root);
    let replacement_link = replacement
        .nodes
        .iter()
        .find(|(id, node)| {
            replacement_subtree.contains(id)
                && node.role() == Role::Link
                && node.label() == Some("Return to child source")
        })
        .map(|(id, _)| *id)
        .expect("replacement session has its own child link");
    assert_ne!(link, replacement_link);
    assert_eq!(app.accessibility.focus, None);
}

#[test]
fn livery_accessibility_click_waits_for_a_physical_pointer_capture() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p7-accessibility-children/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture.clone(), fixture.clone()];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("capture fixture contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("capture fixtures open");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::Accessibility, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    app.refresh_chrome();
    let (width, height) = app.logical_size();
    let pane = app
        .frisket
        .frame(width, height)
        .expect("capture Frisket frame");
    app.workspace
        .set_content_rects(pane.content_rects.iter().copied());
    let _ = app.workspace.pump();
    let _ = app.workspace.frame();
    app.workspace.mark_visible_documents_presented();

    let tree = app
        .prepare_accessibility_tree()
        .expect("capture composite accessibility tree");
    let first_link = livery_a11y_node_for_tile(
        &tree,
        &app.accessibility,
        TileId(1),
        "Open child destination",
        Role::Link,
    )
    .expect("first tile link");
    let second_link = livery_a11y_node_for_tile(
        &tree,
        &app.accessibility,
        TileId(2),
        "Open child destination",
        Role::Link,
    )
    .expect("second tile link");
    let second_action = match app
        .accessibility
        .action_for(second_link)
        .expect("second tile link has a Pelt action")
    {
        WorkspaceA11yActionTarget::Document(action) => action,
        WorkspaceA11yActionTarget::Frisket(_) => {
            panic!("second tile link must use Livery routing")
        },
    };
    assert!(second_action.supports(DocumentA11yAction::Click));
    let (x, y) = app
        .workspace
        .controller(TileId(2))
        .and_then(|controller| controller.accessibility_click_target(second_action.local_node))
        .map(|point| {
            (
                second_action.content_rect.x + point.point.x,
                second_action.content_rect.y + point.point.y,
            )
        })
        .expect("second tile link has a concrete pointer point");
    let held = app.workspace.input(SessionInput::PointerButton {
        x,
        y,
        button: SessionPointerButton::Primary,
        state: SessionButtonState::Pressed,
        modifiers: SessionModifiers::default(),
    });
    assert!(held.handled, "ordinary second-tile press starts capture");
    assert!(
        app.workspace.has_active_pointer_capture(),
        "the workspace records the physical pointer capture"
    );
    assert!(
        !app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: first_link,
            data: None,
        }),
        "an accessibility Click cannot borrow another tile's physical capture"
    );
    for tile in [TileId(1), TileId(2)] {
        assert!(
            app.workspace.controller(tile).is_some_and(|controller| {
                controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/index.html")
            }),
            "capture rejection leaves tile {} on its source document",
            tile.0
        );
    }
}

#[test]
fn livery_child_accessibility_text_values_are_namespaced_stale_safe_and_submit_forms() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p7-accessibility-edit/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture.clone(), fixture.clone()];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("accessibility edit tree contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("accessibility edit fixtures open");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::AccessibilityEdit, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    // The headed Windows receipt runs at a 2x physical scale, leaving a
    // 235x206 logical content hole in each side-by-side tile. Keep the
    // GPU-free form route proof inside that same visibility constraint.
    app.scale_factor = 2.0;

    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let (width, height) = app.logical_size();
        let pane = app
            .frisket
            .frame(width, height)
            .expect("accessibility edit Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };
    let node = |app: &WorkspaceApp, tree: &TreeUpdate, tile, label: &str, role| {
        livery_a11y_node_for_tile(tree, &app.accessibility, tile, label, role)
            .unwrap_or_else(|error| panic!("{error}"))
    };
    fn value(tree: &TreeUpdate, id: AccessNodeId) -> Option<&str> {
        tree.nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .and_then(|(_, node)| node.value())
    }
    let supports = |tree: &TreeUpdate, id, action| {
        tree.nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .is_some_and(|(_, node)| node.supports_action(action))
    };

    compose(&mut app);
    let initial = app
        .prepare_accessibility_tree()
        .expect("initial accessibility edit tree");
    let note = node(
        &app,
        &initial,
        TileId(1),
        "Accessible note",
        Role::TextInput,
    );
    let sibling_note = node(
        &app,
        &initial,
        TileId(2),
        "Accessible note",
        Role::TextInput,
    );
    assert_eq!(value(&initial, note), Some("cedar"));
    assert_eq!(value(&initial, sibling_note), Some("cedar"));
    assert!(supports(&initial, note, Action::SetValue));
    for label in ["Read-only note", "Count", "Password"] {
        let protected = node(&app, &initial, TileId(1), label, Role::TextInput);
        assert!(
            !supports(&initial, protected, Action::SetValue),
            "{label} must not advertise SetValue"
        );
        assert!(!app.apply_accessibility_request(A11yActionRequest {
            action: Action::SetValue,
            target_node: protected,
            data: Some(ActionData::Value("not writable".into())),
        }));
    }
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: note,
        data: Some(ActionData::NumericValue(4.0)),
    }));
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: note,
        data: None,
    }));
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: note,
        data: Some(ActionData::Value("birch".into())),
    }));
    assert_eq!(
        app.workspace.document_session_generation(TileId(1)),
        Some(1)
    );
    assert!(
        app.workspace
            .controller(TileId(2))
            .is_some_and(|controller| {
                controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/index.html")
            })
    );

    compose(&mut app);
    let changed = app
        .prepare_accessibility_tree()
        .expect("changed accessibility edit tree");
    let changed_note = node(
        &app,
        &changed,
        TileId(1),
        "Accessible note",
        Role::TextInput,
    );
    let unchanged_sibling = node(
        &app,
        &changed,
        TileId(2),
        "Accessible note",
        Role::TextInput,
    );
    assert_eq!(value(&changed, changed_note), Some("birch"));
    assert_eq!(value(&changed, unchanged_sibling), Some("cedar"));
    assert!(supports(&changed, changed_note, Action::SetValue));
    let action = app
        .accessibility
        .action_for(changed_note)
        .expect("changed note has a Pelt action target");
    let WorkspaceA11yActionTarget::Document(action) = action else {
        panic!("changed note must route through Pelt's document namespace");
    };
    assert_eq!(action.tile, TileId(1));
    assert_eq!(action.session_identity.generation, 1);

    let submit = node(
        &app,
        &changed,
        TileId(1),
        "Save accessible note",
        Role::Button,
    );
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Click,
        target_node: submit,
        data: None,
    }));
    let controller = app
        .workspace
        .controller(TileId(1))
        .expect("submitted Livery tile remains live");
    assert!(
        controller
            .address()
            .replace('\\', "/")
            .contains("/result.html?note=birch"),
        "form submission must carry the changed Livery value"
    );
    assert_eq!(controller.session_generation(), 2);
    assert!(!app.apply_accessibility_request(A11yActionRequest {
        action: Action::SetValue,
        target_node: changed_note,
        data: Some(ActionData::Value("stale".into())),
    }));
    assert!(
        app.workspace
            .controller(TileId(2))
            .is_some_and(|other| { other.address().replace('\\', "/").ends_with("/index.html") })
    );
}

#[test]
fn livery_nested_editor_rejects_a_queued_pre_scroll_set_value() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p7-accessibility-input/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture.clone(), fixture.clone()];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("nested editor tree contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("nested editor fixtures open");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::AccessibilityInput, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    app.scale_factor = 2.0;
    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let (width, height) = app.logical_size();
        let pane = app
            .frisket
            .frame(width, height)
            .expect("nested editor Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };

    compose(&mut app);
    let initial = app
        .prepare_accessibility_tree()
        .expect("initial nested editor accessibility tree");
    let note = livery_a11y_node_for_tile(
        &initial,
        &app.accessibility,
        TileId(1),
        "Nested note",
        Role::TextInput,
    )
    .expect("initial nested textarea");
    assert!(
        initial
            .nodes
            .iter()
            .find(|(id, _)| *id == note)
            .is_some_and(|(_, node)| node.supports_action(Action::SetValue))
    );
    let content = app
        .workspace
        .content_rect(TileId(1))
        .expect("nested editor content hole");
    assert!(app.workspace.scroll_at(
        content.x + content.width.min(190.0) * 0.5,
        content.y + content.height.min(96.0) * 0.5,
        0.0,
        96.0,
    ));

    assert!(
        !app.apply_accessibility_request(A11yActionRequest {
            action: Action::SetValue,
            target_node: note,
            data: Some(ActionData::Value("stale mutation".into())),
        }),
        "a queued pre-scroll SetValue must be rejected against the live projection"
    );
    compose(&mut app);
    let scrolled = app
        .prepare_accessibility_tree()
        .expect("scrolled nested editor accessibility tree");
    let scrolled_note = livery_a11y_node_for_tile(
        &scrolled,
        &app.accessibility,
        TileId(1),
        "Nested note",
        Role::TextInput,
    )
    .expect("scrolled nested textarea");
    let node = scrolled
        .nodes
        .iter()
        .find(|(id, _)| *id == scrolled_note)
        .map(|(_, node)| node)
        .expect("scrolled nested textarea node");
    assert_eq!(node.value(), Some("cedar"));
    assert!(node.supports_action(Action::ScrollIntoView));
    assert!(!node.supports_action(Action::SetValue));
}

#[test]
fn livery_nested_editor_accessibility_input_receipt_runs_at_two_x() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p7-accessibility-input/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture.clone(), fixture.clone()];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("nested editor tree contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("nested editor fixtures open");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::AccessibilityInput, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    app.scale_factor = 2.0;

    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let (width, height) = app.logical_size();
        let pane = app
            .frisket
            .frame(width, height)
            .expect("nested editor Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };

    compose(&mut app);
    let mut assertion = None;
    for _ in 0..4 {
        assertion = app
            .drive_accessibility_input_workspace_receipt_step()
            .expect("accessibility-input receipt state machine");
        compose(&mut app);
        if assertion.is_some() {
            break;
        }
    }
    assert_eq!(
        assertion.as_deref(),
        Some(ACCESSIBILITY_INPUT_WORKSPACE_ASSERTION),
        "the GPU-free twin-tile receipt must complete its physical wheel, drag, Text, and IME path"
    );
}

#[test]
fn livery_child_accessibility_scroll_into_view_is_namespaced_and_tile_local() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pelt desktop has a parent")
        .join("examples/workspace/p7-accessibility-scroll/index.html")
        .to_string_lossy()
        .into_owned();
    let urls = vec![fixture.clone(), fixture.clone()];
    let tree = tree_from_urls(&urls);
    #[cfg(target_os = "windows")]
    let registries = workspace_registries(None);
    #[cfg(not(target_os = "windows"))]
    let registries = workspace_registries();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!("accessibility scroll tree contains only documents");
            };
            Ok(PeltTileRequest::new(address, (960, 640)))
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )
    .expect("accessibility scroll fixtures open");
    let frisket = FrisketSurface::new(workspace.tree());
    let config = WorkspaceViewerConfig::new(urls, WindowingMode::Headed)
        .with_workspace_receipt(WorkspaceReceipt::AccessibilityScroll, "unused.png");
    #[cfg(target_os = "windows")]
    let mut app = WorkspaceApp::new(config, workspace, frisket, None);
    #[cfg(not(target_os = "windows"))]
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    // Match the headed receipt's small 2x logical content holes so the
    // initial wheel route and subsequent a11y reveal share its geometry.
    app.scale_factor = 2.0;

    let compose = |app: &mut WorkspaceApp| {
        app.refresh_chrome();
        let (width, height) = app.logical_size();
        let pane = app
            .frisket
            .frame(width, height)
            .expect("accessibility scroll Frisket frame");
        app.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        let _ = app.workspace.pump();
        let _ = app.workspace.frame();
        app.workspace.mark_visible_documents_presented();
    };
    let node = |app: &WorkspaceApp, tree: &TreeUpdate, tile| {
        livery_a11y_node_for_tile(
            tree,
            &app.accessibility,
            tile,
            "Open nested destination",
            Role::Link,
        )
        .unwrap_or_else(|error| panic!("{error}"))
    };
    let supports = |tree: &TreeUpdate, id, action| {
        tree.nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .is_some_and(|(_, node)| node.supports_action(action))
    };
    let bounds = |tree: &TreeUpdate, id| {
        tree.nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .and_then(|(_, node)| node.bounds())
            .map(|rect| (rect.x0, rect.y0, rect.x1, rect.y1))
            .expect("nested link bounds")
    };
    let scrolls = |app: &WorkspaceApp, tile| {
        app.workspace
            .controller(tile)
            .and_then(|controller| {
                controller
                    .session_as_any_ref()
                    .downcast_ref::<genet_documents::LiveryDocumentSession>()
            })
            .map(|session| session.document().element_scroll().clone())
            .expect("scroll fixture stays a Livery session")
    };

    compose(&mut app);
    let content = app
        .workspace
        .content_rect(TileId(1))
        .expect("first scroll fixture content hole");
    assert!(
        app.workspace.scroll_at(
            content.x + content.width.min(180.0) * 0.5,
            content.y + content.height.min(100.0) * 0.5,
            0.0,
            96.0,
        ),
        "Pelt's tile-local wheel route enters the nested scrollport"
    );
    let pre_reveal_scroll = scrolls(&app, TileId(1));
    let sibling_scroll = scrolls(&app, TileId(2));
    assert!(!pre_reveal_scroll.is_empty());
    assert!(sibling_scroll.is_empty());

    compose(&mut app);
    let scrolled = app
        .prepare_accessibility_tree()
        .expect("scrolled composite accessibility tree");
    let link = node(&app, &scrolled, TileId(1));
    let sibling_link = node(&app, &scrolled, TileId(2));
    assert!(supports(&scrolled, link, Action::ScrollIntoView));
    assert!(!supports(&scrolled, link, Action::Click));
    assert!(
        !supports(&scrolled, sibling_link, Action::ScrollIntoView),
        "the untouched sibling cannot acquire the focused tile's reveal action"
    );
    let before_bounds = bounds(&scrolled, link);
    let target = app
        .accessibility
        .action_for(link)
        .expect("scrolled nested link has a Pelt action target");
    let WorkspaceA11yActionTarget::Document(target) = target else {
        panic!("nested link must route through Pelt's document namespace");
    };
    assert_eq!(target.tile, TileId(1));
    assert_eq!(target.session_identity.generation, 1);
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::ScrollIntoView,
        target_node: link,
        data: None,
    }));
    assert_ne!(scrolls(&app, TileId(1)), pre_reveal_scroll);
    assert_eq!(scrolls(&app, TileId(2)), sibling_scroll);
    assert_eq!(
        app.workspace.document_session_generation(TileId(1)),
        Some(1)
    );
    assert_eq!(
        app.workspace.document_session_generation(TileId(2)),
        Some(1)
    );

    compose(&mut app);
    let revealed = app
        .prepare_accessibility_tree()
        .expect("revealed composite accessibility tree");
    let revealed_link = node(&app, &revealed, TileId(1));
    assert_eq!(
        revealed_link, link,
        "reveal retains the child node identity"
    );
    assert_ne!(bounds(&revealed, revealed_link), before_bounds);
    assert!(supports(&revealed, revealed_link, Action::ScrollIntoView));
    assert!(
        supports(&revealed, revealed_link, Action::Click),
        "a revealed nested target regains Click only after clip-aware Livery promotion"
    );
    assert!(
        matches!(
            app.accessibility.action_for(revealed_link),
            Some(WorkspaceA11yActionTarget::Document(action))
                if action.supports(DocumentA11yAction::Click)
        ),
        "a revealed nested target has a concrete tile-local pointer point"
    );
    assert!(app.apply_accessibility_request(A11yActionRequest {
        action: Action::Click,
        target_node: revealed_link,
        data: None,
    }));
    assert!(
        !app.apply_accessibility_request(A11yActionRequest {
            action: Action::Click,
            target_node: revealed_link,
            data: None,
        }),
        "the retained pre-navigation action is stale after Click replaces the session"
    );
    assert!(
        app.workspace
            .controller(TileId(1))
            .is_some_and(|controller| {
                controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/result.html")
                    && controller.session_generation() == 2
            }),
        "ordinary pointer Click navigates only the focused tile"
    );
    assert!(
        app.workspace
            .controller(TileId(2))
            .is_some_and(|controller| {
                controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/index.html")
                    && controller.session_generation() == 1
            }),
        "nested Click leaves the sibling tile's session untouched"
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
