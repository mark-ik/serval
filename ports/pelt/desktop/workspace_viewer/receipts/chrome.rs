//! Host-chrome receipts.
//!
//! `Chrome`, `LoadingError`, `NarrowChrome`, `ChromeDpi`, `Appearance` and
//! the developer-only `TabardPreview`: everything that drives Pelt's own
//! shell rather than a document lane.

use crate::workspace_viewer::*;

impl WorkspaceApp {
    pub(in crate::workspace_viewer) fn drive_chrome_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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

    pub(in crate::workspace_viewer) fn drive_loading_error_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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

    pub(in crate::workspace_viewer) fn drive_narrow_chrome_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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

    pub(in crate::workspace_viewer) fn drive_chrome_dpi_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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
                    || self.chrome_theme() != AppearanceTheme::Dark
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
                            AppearanceTheme::Light
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
                if self.chrome_theme() != AppearanceTheme::Light
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

    pub(in crate::workspace_viewer) fn drive_appearance_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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
                    || self.chrome_theme() != AppearanceTheme::Dark
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
                if chrome.theme != AppearanceTheme::Dark
                    || chrome.appearance != Some(self.chrome_appearance())
                    || light.width <= 0.0
                    || light.height <= 0.0
                    || !matches!(
                        self.frisket
                            .hit(light.x + light.width / 2.0, light.y + light.height / 2.0),
                        Some(FrisketHit::ChromeAction(ChromeAction::ChooseTheme(
                            AppearanceTheme::Light
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
                if self.chrome_theme() != AppearanceTheme::Light
                    || chrome.theme != AppearanceTheme::Light
                    || chrome.appearance != Some(self.chrome_appearance())
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

    pub(in crate::workspace_viewer) fn drive_tabard_preview_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        #[cfg(not(feature = "tabard-preview"))]
        return Err(
            "tabard-preview workspace receipt needs `--features tabard-preview`".to_owned(),
        );

        #[cfg(feature = "tabard-preview")]
        {
            let tile = self
                .workspace
                .focused_tile()
                .ok_or("tabard-preview receipt has no focused tile")?;
            match self.receipt_step {
                0 => {
                    let controller = self
                        .workspace
                        .controller(tile)
                        .ok_or("tabard-preview receipt has no focused controller")?;
                    let content = self
                        .workspace
                        .content_rect(tile)
                        .ok_or("tabard-preview receipt has no Frisket content geometry")?;
                    let tab = self
                        .frisket
                        .tab_rect(tile)
                        .ok_or("tabard-preview receipt has no retained tab geometry")?;
                    let chrome_background = self
                        .frisket
                        .chrome_computed_style("pelt-chrome", "background-color")
                        .ok_or("tabard-preview receipt could not read the baseline Chrome color")?;
                    self.tabard_preview_baseline = Some(TabardPreviewBaseline {
                        focused_tile: tile,
                        tile_count: self.workspace.tree().tiles().len(),
                        content,
                        tab,
                        address: controller.address().to_owned(),
                        can_go_back: controller.can_go_back(),
                        can_go_forward: controller.can_go_forward(),
                        chrome_background,
                    });
                    self.frisket
                        .set_chrome_stylesheet(Some(tabard_preview_stylesheet()));
                },
                1 => {
                    let baseline = self
                        .tabard_preview_baseline
                        .as_ref()
                        .ok_or("tabard-preview receipt lost its baseline")?;
                    let controller = self
                        .workspace
                        .controller(tile)
                        .ok_or("tabard-preview receipt lost its focused controller")?;
                    let chrome_background = self
                        .frisket
                        .chrome_computed_style("pelt-chrome", "background-color")
                        .ok_or("tabard-preview receipt could not read the themed Chrome color")?;
                    if tile != baseline.focused_tile
                        || self.workspace.tree().tiles().len() != baseline.tile_count
                        || self.workspace.content_rect(tile) != Some(baseline.content)
                        || self.frisket.tab_rect(tile) != Some(baseline.tab)
                        || controller.address() != baseline.address
                        || controller.can_go_back() != baseline.can_go_back
                        || controller.can_go_forward() != baseline.can_go_forward
                    {
                        return Err(
                            "tabard-preview receipt changed document tiles, session history, or Frisket geometry"
                                .to_owned(),
                        );
                    }
                    if chrome_background == baseline.chrome_background {
                        return Err(format!(
                            "tabard-preview receipt did not change computed Chrome background color: {chrome_background}"
                        ));
                    }
                    self.receipt_step = self.receipt_step.saturating_add(1);
                    return Ok(Some(TABARD_PREVIEW_WORKSPACE_ASSERTION.to_owned()));
                },
                _ => return Ok(Some(TABARD_PREVIEW_WORKSPACE_ASSERTION.to_owned())),
            }
            self.receipt_step = self.receipt_step.saturating_add(1);
            Ok(None)
        }
    }

    pub(in crate::workspace_viewer) fn validate_chrome_workspace_receipt(
        &self,
    ) -> Result<(), String> {
        self.validate_mixed_workspace(
            "chrome receipt",
            "Scrying native surface",
            "Scrying native surface",
        )
    }
}
