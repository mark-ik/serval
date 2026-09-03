// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Capability-routing receipts and the shared receipt step machine.
//!
//! Holds `Mixed` and `Fallback` (capability routing proper), the dispatch
//! and timeout scaffolding every other receipt advances through, and the
//! synthetic click helpers they drive input with.

use crate::workspace_viewer::*;

impl WorkspaceApp {
    pub(in crate::workspace_viewer) fn drive_workspace_receipt(
        &mut self,
    ) -> Result<Option<String>, String> {
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
            WorkspaceReceipt::AccessibilityAddress => {
                self.drive_accessibility_address_workspace_receipt_step()
            },
            WorkspaceReceipt::AccessibilityChildren => {
                self.drive_accessibility_children_workspace_receipt_step()
            },
            WorkspaceReceipt::AccessibilityEdit => {
                self.drive_accessibility_edit_workspace_receipt_step()
            },
            WorkspaceReceipt::AccessibilityScroll => {
                self.drive_accessibility_scroll_workspace_receipt_step()
            },
            WorkspaceReceipt::AccessibilityClick => {
                self.drive_accessibility_click_workspace_receipt_step()
            },
            WorkspaceReceipt::AccessibilityInput => {
                self.drive_accessibility_input_workspace_receipt_step()
            },
            WorkspaceReceipt::NarrowChrome => self.drive_narrow_chrome_workspace_receipt_step(),
            WorkspaceReceipt::ChromeDpi => self.drive_chrome_dpi_workspace_receipt_step(),
            WorkspaceReceipt::Reader => self.drive_reader_workspace_receipt_step(),
            WorkspaceReceipt::ReaderAccessibility => {
                #[cfg(feature = "reader")]
                {
                    self.drive_reader_accessibility_workspace_receipt_step()
                }
                #[cfg(not(feature = "reader"))]
                {
                    Err("reader-accessibility receipt needs the Reader feature".to_owned())
                }
            },
            WorkspaceReceipt::TabardPreview => self.drive_tabard_preview_workspace_receipt_step(),
            WorkspaceReceipt::TabardReaderPreview => {
                self.drive_tabard_reader_preview_workspace_receipt_step()
            },
        }
    }

    pub(in crate::workspace_viewer) fn workspace_receipt_timeout_error(&self) -> Option<String> {
        if matches!(
            self.config.workspace_receipt,
            Some(
                WorkspaceReceipt::LoadingError
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

    pub(in crate::workspace_viewer) fn drive_mixed_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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
    pub(in crate::workspace_viewer) fn mixed_size_matrix_ready(&mut self) -> Result<bool, String> {
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
    pub(in crate::workspace_viewer) fn mixed_size_is_composed(
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
    pub(in crate::workspace_viewer) fn request_mixed_resize(&mut self, target: (u32, u32)) {
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

    pub(in crate::workspace_viewer) fn drive_mixed_workspace_receipt(
        &mut self,
    ) -> Result<String, String> {
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

    #[cfg(target_os = "windows")]
    pub(in crate::workspace_viewer) fn mixed_native_receipt_ready(&mut self) -> bool {
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

    pub(in crate::workspace_viewer) fn drive_fallback_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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

    pub(in crate::workspace_viewer) fn validate_fallback_route(
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

    pub(in crate::workspace_viewer) fn validate_capability_receipt(&self) -> Result<(), String> {
        self.validate_mixed_workspace("P4", "Static Livery", "Static Livery")
    }

    pub(in crate::workspace_viewer) fn validate_mixed_workspace(
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

    pub(in crate::workspace_viewer) fn capability_receipt_ready(&self) -> bool {
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

    pub(in crate::workspace_viewer) fn drive_receipt_step(&mut self) -> Result<(), String> {
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
                    path: workbench::TilePath(Vec::new()),
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

    pub(in crate::workspace_viewer) fn click_tab(&mut self, tile: TileId) -> Result<(), String> {
        let rect = self
            .frisket
            .tab_rect(tile)
            .ok_or_else(|| format!("tile {} tab has no Frisket geometry", tile.0))?;
        self.pointer_move(rect.x + 8.0, rect.y + rect.height / 2.0);
        self.pointer_down();
        self.pointer_up();
        Ok(())
    }

    pub(in crate::workspace_viewer) fn click_chrome(&mut self, action: &str) -> Result<(), String> {
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

    pub(in crate::workspace_viewer) fn click_chrome_physical(
        &mut self,
        action: &str,
    ) -> Result<(), String> {
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
