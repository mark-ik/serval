// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Reader-lane receipts.
//!
//! Reader reuses a focused tile's already-held response; these receipts
//! assert that reuse, its partial accessibility tree, and the restoration
//! back to the original Livery route.

use crate::workspace_viewer::*;

impl WorkspaceApp {
    pub(in crate::workspace_viewer) fn drive_reader_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        #[cfg(not(feature = "reader"))]
        {
            return Err("Reader workspace receipt needs the reader feature".to_owned());
        }
        #[cfg(feature = "reader")]
        {
            let reader = TileId(1);
            let neighbor = TileId(2);
            match self.receipt_step {
                0 => {
                    require_tile(self.workspace.tree(), 2)?;
                    let route = self
                        .workspace
                        .route(reader)
                        .ok_or("Reader receipt has no initial article route")?;
                    let source = self
                        .workspace
                        .controller(reader)
                        .and_then(PeltController::clip)
                        .and_then(|clip| {
                            clip.artifacts.into_iter().find(|artifact| {
                                artifact.role == inker::DocumentClipArtifactRole::SourceResponse
                            })
                        })
                        .ok_or("Reader receipt could not find Livery's retained source response")?;
                    let neighbor_route = self
                        .workspace
                        .route(neighbor)
                        .ok_or("Reader receipt has no neighboring document route")?;
                    if route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                        || route.source != PeltRouteSource::Automatic
                        || !matches!(route.state, PeltRouteState::Document)
                        || source.bytes != READER_FIXTURE_SOURCE.as_bytes()
                        || neighbor_route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
                        || neighbor_route.source != PeltRouteSource::Automatic
                    {
                        return Err(
                            "Reader receipt did not begin with one retained Livery source and one ordinary neighbor"
                                .to_owned(),
                        );
                    }
                    self.click_chrome("engine-menu")?;
                },
                1 => {
                    let reader_choice = self
                        .frisket
                        .chrome_rect("engine-reader")
                        .ok_or("Reader receipt did not expose Reader in the engine menu")?;
                    if !self
                        .chrome_model()
                        .engine_choices
                        .contains(&ChromeEngineChoice::Reader)
                        || !matches!(
                            self.frisket.hit(
                                reader_choice.x + reader_choice.width / 2.0,
                                reader_choice.y + reader_choice.height / 2.0
                            ),
                            Some(FrisketHit::ChromeAction(ChromeAction::ChooseEngine(
                                ChromeEngineChoice::Reader
                            )))
                        )
                    {
                        return Err(
                            "Reader receipt did not retain an interactive Reader route choice"
                                .to_owned(),
                        );
                    }
                    self.click_chrome("engine-reader")?;
                },
                2 => {
                    self.validate_reader_workspace(true)?;
                    self.click_chrome("inspect")?;
                },
                3 => {
                    self.validate_reader_inspector()?;
                    self.click_chrome("engine-menu")?;
                },
                4 => {
                    let automatic = self
                        .frisket
                        .chrome_rect("engine-automatic")
                        .ok_or("Reader receipt did not expose Automatic after Reader selection")?;
                    if !matches!(
                        self.frisket.hit(
                            automatic.x + automatic.width / 2.0,
                            automatic.y + automatic.height / 2.0
                        ),
                        Some(FrisketHit::ChromeAction(ChromeAction::ChooseEngine(
                            ChromeEngineChoice::Automatic
                        )))
                    ) {
                        return Err(
                            "Reader receipt lost Automatic's retained route action".to_owned()
                        );
                    }
                    self.click_chrome("engine-automatic")?;
                },
                5 => {
                    self.validate_reader_workspace(false)?;
                    self.click_chrome("engine-menu")?;
                },
                6 => {
                    if self.frisket.chrome_rect("engine-reader").is_none() {
                        return Err(
                            "Reader receipt could not select Reader again after restoring Livery"
                                .to_owned(),
                        );
                    }
                    self.click_chrome("engine-reader")?;
                },
                7 => {
                    self.validate_reader_workspace(true)?;
                    self.validate_reader_inspector()?;
                    self.receipt_step = self.receipt_step.saturating_add(1);
                    return Ok(Some(READER_WORKSPACE_ASSERTION.to_owned()));
                },
                _ => return Ok(Some(READER_WORKSPACE_ASSERTION.to_owned())),
            }
            self.receipt_step = self.receipt_step.saturating_add(1);
            Ok(None)
        }
    }

    /// Prove the bounded Reader accessibility contract without pretending that
    /// Reader can fetch a link destination from an accessibility action. The
    /// existing navigation route must receive a new held body from the host,
    /// so this receipt keeps Reader link activation virtual until that source
    /// handoff has its own evidence.
    #[cfg(feature = "reader")]
    pub(in crate::workspace_viewer) fn drive_reader_accessibility_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let first_tile = TileId(1);
        let sibling_tile = TileId(2);
        let label = "Continue through the retained article source";
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 2)?;
                if self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "Reader accessibility receipt has no installed platform bridge".to_owned(),
                    );
                }
                let tree = self.prepare_accessibility_tree()?;
                let first_link = match reader_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    first_tile,
                    label,
                    Role::Link,
                ) {
                    Ok(link) => link,
                    // The first receipt render deliberately runs before
                    // `workspace.frame`; wait for Reader's completed frame
                    // rather than deriving semantics early.
                    Err(_) => return Ok(None),
                };
                let sibling_link = reader_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    sibling_tile,
                    label,
                    Role::Link,
                )?;
                if first_link == sibling_link {
                    return Err(
                        "Reader accessibility receipt aliased equal link-local IDs across tiles"
                            .to_owned(),
                    );
                }
                for link in [first_link, sibling_link] {
                    let node = tree
                        .nodes
                        .iter()
                        .find(|(candidate, _)| *candidate == link)
                        .map(|(_, node)| node)
                        .ok_or("Reader accessibility receipt lost its retained link node")?;
                    if !node.supports_action(Action::Focus)
                        || node.supports_action(Action::Click)
                        || node.supports_action(Action::SetValue)
                    {
                        return Err(
                            "Reader accessibility receipt advertised navigation or editing before host source handoff"
                                .to_owned(),
                        );
                    }
                }
                for (tile, link) in [(first_tile, first_link), (sibling_tile, sibling_link)] {
                    let Some(WorkspaceA11yActionTarget::Document(action)) =
                        self.accessibility.action_for(link)
                    else {
                        return Err(format!(
                            "Reader accessibility receipt has no Reader action for tile {}",
                            tile.0
                        ));
                    };
                    if action.tile != tile || !action.supports(DocumentA11yAction::Focus) {
                        return Err(format!(
                            "Reader accessibility receipt misrouted virtual Focus for tile {}",
                            tile.0
                        ));
                    }
                }
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Focus,
                    target_node: first_link,
                    data: None,
                }) {
                    return Err(
                        "Reader accessibility receipt could not apply its virtual Focus action"
                            .to_owned(),
                    );
                }
                self.receipt_step = 1;
                Ok(None)
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let first_link = reader_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    first_tile,
                    label,
                    Role::Link,
                )?;
                let sibling_link = reader_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    sibling_tile,
                    label,
                    Role::Link,
                )?;
                if self.accessibility.focus != Some(WorkspaceA11yFocus::Document(first_link)) {
                    return Err(
                        "Reader accessibility receipt did not retain virtual Focus on its first link"
                            .to_owned(),
                    );
                }
                if first_link == sibling_link || self.workspace.has_active_pointer_capture() {
                    return Err(
                        "Reader accessibility receipt crossed tile namespaces or entered pointer capture"
                            .to_owned(),
                    );
                }
                for tile in [first_tile, sibling_tile] {
                    let controller = self.workspace.controller(tile).ok_or_else(|| {
                        format!("Reader accessibility receipt lost tile {}", tile.0)
                    })?;
                    let route = self.workspace.route(tile).ok_or_else(|| {
                        format!("Reader accessibility receipt lost route {}", tile.0)
                    })?;
                    if route.selected_engine() != inker::routing::ENGINE_GENET_READER
                        || controller.accessibility_projection().is_none()
                    {
                        return Err(format!(
                            "Reader accessibility receipt replaced or rerouted tile {} after virtual Focus",
                            tile.0
                        ));
                    }
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                Ok(Some(READER_ACCESSIBILITY_WORKSPACE_ASSERTION.to_owned()))
            },
            _ => Ok(Some(READER_ACCESSIBILITY_WORKSPACE_ASSERTION.to_owned())),
        }
    }

    pub(in crate::workspace_viewer) fn drive_tabard_reader_preview_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        #[cfg(not(feature = "tabard-reader-preview"))]
        return Err(
            "tabard-reader-preview workspace receipt needs `--features tabard-reader-preview`"
                .to_owned(),
        );

        #[cfg(feature = "tabard-reader-preview")]
        self.drive_reader_workspace_receipt_step().map(|assertion| {
            assertion.map(|_| TABARD_READER_PREVIEW_WORKSPACE_ASSERTION.to_owned())
        })
    }

    #[cfg(feature = "reader")]
    pub(in crate::workspace_viewer) fn validate_reader_workspace(
        &self,
        reader_selected: bool,
    ) -> Result<(), String> {
        let reader = TileId(1);
        let neighbor = TileId(2);
        let route = self
            .workspace
            .route(reader)
            .ok_or("Reader receipt has no article route")?;
        let expected_engine = if reader_selected {
            inker::routing::ENGINE_GENET_READER
        } else {
            inker::routing::ENGINE_GENET_LIVERY
        };
        let expected_title = if reader_selected {
            "Source stays with the tile"
        } else {
            "Pelt reader held source"
        };
        let report = self
            .workspace
            .controller(reader)
            .and_then(PeltController::inspect)
            .ok_or("Reader receipt article has no structural report")?;
        let neighbor_route = self
            .workspace
            .route(neighbor)
            .ok_or("Reader receipt lost its neighboring route")?;
        let neighbor_report = self
            .workspace
            .controller(neighbor)
            .and_then(PeltController::inspect)
            .ok_or("Reader receipt neighbor has no structural report")?;
        let held_source = self
            .workspace
            .controller(reader)
            .map(PeltController::request)
            .and_then(|request| request.body.as_deref());
        let link_is_retained = if reader_selected {
            report
                .links
                .iter()
                .any(|link| link.replace('\\', "/").ends_with("/reader/next.html"))
        } else {
            report.links.iter().any(|link| link == "next.html")
        };
        let lineage_is_reader = report
            .lineage
            .as_ref()
            .is_some_and(|lineage| lineage.tool == "fleece");
        if route.selected_engine() != expected_engine
            || !matches!(route.state, PeltRouteState::Document)
            || report.title.as_deref() != Some(expected_title)
            || report.headings != ["Source stays with the tile"]
            || !link_is_retained
            || held_source != Some(READER_FIXTURE_SOURCE)
            || lineage_is_reader != reader_selected
            || neighbor_route.selected_engine() != inker::routing::ENGINE_GENET_LIVERY
            || neighbor_route.source != PeltRouteSource::Automatic
            || !matches!(neighbor_route.state, PeltRouteState::Document)
            || neighbor_report.title.as_deref() != Some("Reader neighbor stays Livery")
            || neighbor_report.headings != ["Neighbor stays in Livery"]
        {
            let expectation = if reader_selected {
                "preserve its held source, Fleece report, or neighboring Livery tile"
            } else {
                "restore the original Livery document from its held source"
            };
            return Err(format!(
                "Reader receipt did not {expectation}: article engine={}, source={:?}, state={:?}, title={:?}, headings={:?}, links={:?}, retained-link={link_is_retained}, held-source={}, fleece-lineage={lineage_is_reader}; neighbor engine={}, source={:?}, state={:?}, title={:?}, headings={:?}",
                route.selected_engine(),
                route.source,
                route.state,
                report.title,
                report.headings,
                report.links,
                held_source == Some(READER_FIXTURE_SOURCE),
                neighbor_route.selected_engine(),
                neighbor_route.source,
                neighbor_route.state,
                neighbor_report.title,
                neighbor_report.headings,
            ));
        }
        Ok(())
    }

    #[cfg(feature = "reader")]
    pub(in crate::workspace_viewer) fn validate_reader_inspector(&self) -> Result<(), String> {
        let inspector = self
            .chrome_model()
            .inspector
            .ok_or("Reader receipt did not retain its structural inspector")?;
        let lineage = inspector
            .sections
            .iter()
            .find(|section| section.label == "Lineage")
            .and_then(|section| section.entries.first())
            .ok_or("Reader receipt inspector omitted Fleece lineage")?;
        if !lineage.contains("fleece") || !inspector.summary.contains("heading") {
            return Err(
                "Reader receipt inspector did not expose the Fleece derivation posture".to_owned(),
            );
        }
        Ok(())
    }
}
