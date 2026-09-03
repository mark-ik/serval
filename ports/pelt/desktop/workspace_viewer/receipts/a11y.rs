// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Accessibility receipts.
//!
//! The typed AccessKit action routes: focus and click, address `SetValue`,
//! namespaced Livery children, nested scroll, clip-aware click, and the
//! nested editor input and edit routes.

use crate::workspace_viewer::*;

impl WorkspaceApp {
    pub(in crate::workspace_viewer) fn drive_accessibility_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
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
                let theme = a11y_node(&tree, "Toggle Pelt appearance settings", Role::Button)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Focus,
                    target_node: theme,
                    data: None,
                }) || self.chrome_appearance_open
                    || self.chrome_theme() != AppearanceTheme::Dark
                {
                    return Err(
                        "accessibility Focus activated Pelt's appearance control instead of only moving virtual focus"
                            .to_owned(),
                    );
                }
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let theme = a11y_node(&tree, "Toggle Pelt appearance settings", Role::Button)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: theme,
                    data: None,
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
                    data: None,
                }) || self.chrome_theme() != AppearanceTheme::Dark
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
                    data: None,
                }) || self.chrome_theme() != AppearanceTheme::Light
                    || !self.chrome_appearance_open
                {
                    return Err("accessibility Click did not select Pelt's Light theme".to_owned());
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

    pub(in crate::workspace_viewer) fn drive_accessibility_address_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let tile = TileId(1);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                if self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "accessibility address receipt began without an installed platform bridge"
                            .to_owned(),
                    );
                }
                let tree = self.prepare_accessibility_tree()?;
                let address = a11y_node(&tree, "Address", Role::TextInput)?;
                let node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == address)
                    .map(|(_, node)| node)
                    .ok_or("accessibility address receipt lost the address node")?;
                if !node.supports_action(Action::SetValue)
                    || node.value().is_none()
                    || !self.apply_accessibility_request(A11yActionRequest {
                        action: Action::SetValue,
                        target_node: address,
                        data: Some(ActionData::Value(
                            Path::new(node.value().unwrap())
                                .parent()
                                .ok_or("accessibility address fixture has no parent")?
                                .join("next.html")
                                .to_string_lossy()
                                .into_owned()
                                .into(),
                        )),
                    })
                {
                    return Err(
                        "accessibility address receipt could not submit SetValue through the bridge"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility address receipt lost its focused controller")?;
                if !matches!(
                    controller.document_state(),
                    PeltDocumentState::Loading { .. }
                ) || !controller.can_go_back()
                    || !self.workspace.controller(TileId(2)).is_some_and(|other| {
                        other.address().replace('\\', "/").ends_with("/index.html")
                    })
                {
                    return Err(
                        "accessibility address SetValue did not retain the focused tile's loading transition"
                            .to_owned(),
                    );
                }
            },
            1 => {
                let diagnostic = self
                    .last_chrome_document
                    .clone()
                    .ok_or("accessibility address receipt did not compose its loading document")?;
                if diagnostic.kind != ChromeDocumentKind::Loading
                    || diagnostic.tile != tile
                    || self.workspace.content_rect(tile) != Some(diagnostic.rect)
                {
                    return Err(
                        "accessibility address receipt did not keep loading content in the focused tile hole"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(None);
            },
            2 => {
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility address receipt lost its successful controller")?;
                if !matches!(controller.document_state(), PeltDocumentState::Ready)
                    || !controller.can_go_back()
                {
                    return Err(
                        "accessibility address receipt did not settle the successful destination to Ready"
                            .to_owned(),
                    );
                }
                let next = controller.address().to_owned();
                if !self.workspace.controller(TileId(2)).is_some_and(|other| {
                    other.address().replace('\\', "/").ends_with("/index.html")
                }) {
                    return Err(
                        "accessibility address navigation changed the sibling tile".to_owned()
                    );
                }
                let tree = self.prepare_accessibility_tree()?;
                let address = a11y_node(&tree, "Address", Role::TextInput)?;
                if tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == address)
                    .and_then(|(_, node)| node.value())
                    != Some(next.as_str())
                {
                    return Err(
                        "accessibility address receipt did not project the settled successful address value"
                            .to_owned(),
                    );
                }
                let missing = Path::new(&next)
                    .parent()
                    .ok_or("accessibility address destination has no parent")?
                    .join("missing.html")
                    .to_string_lossy()
                    .into_owned();
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::SetValue,
                    target_node: address,
                    data: Some(ActionData::Value(missing.into())),
                }) {
                    return Err(
                        "accessibility address receipt could not submit its deterministic missing path"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility address receipt lost its failed controller")?;
                if !matches!(controller.document_state(), PeltDocumentState::Error { .. })
                    || !controller.can_go_back()
                    || controller.address() != next
                    || !self.workspace.controller(TileId(2)).is_some_and(|other| {
                        other.address().replace('\\', "/").ends_with("/index.html")
                    })
                {
                    return Err(
                        "accessibility address failure replaced the retained successful controller"
                            .to_owned(),
                    );
                }
            },
            3 => {
                let diagnostic = self
                    .last_chrome_document
                    .clone()
                    .ok_or("accessibility address receipt did not compose its error document")?;
                let retained_address = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility address receipt lost its retained controller")?
                    .address()
                    .to_owned();
                let retained_can_go_back = self
                    .workspace
                    .controller(tile)
                    .is_some_and(PeltController::can_go_back);
                if diagnostic.kind != ChromeDocumentKind::Error
                    || diagnostic.tile != tile
                    || !diagnostic
                        .address
                        .replace('\\', "/")
                        .ends_with("/missing.html")
                    || self.workspace.content_rect(tile) != Some(diagnostic.rect)
                    || !retained_address.replace('\\', "/").ends_with("/next.html")
                    || !retained_can_go_back
                    || !self.workspace.controller(TileId(2)).is_some_and(|other| {
                        other.address().replace('\\', "/").ends_with("/index.html")
                    })
                {
                    return Err(
                        "accessibility address receipt did not retain the error document in the focused content hole"
                            .to_owned(),
                    );
                }
                let tree = self.prepare_accessibility_tree()?;
                let address = a11y_node(&tree, "Address", Role::TextInput)?;
                let projected = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == address)
                    .and_then(|(_, node)| node.value())
                    .ok_or("accessibility address receipt lost the projected address value")?;
                if projected != retained_address {
                    return Err(
                        "accessibility address receipt did not project the retained successful address after failure"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(ACCESSIBILITY_ADDRESS_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_ADDRESS_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    pub(in crate::workspace_viewer) fn drive_accessibility_children_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let tile = TileId(1);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 1)?;
                if self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "child accessibility receipt began without an installed platform bridge"
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
                    .ok_or("child accessibility receipt did not expose a content aperture")?;
                if !content.description().is_some_and(|description| {
                    description.contains("partial accessibility")
                        && description.contains("composes")
                }) || content.children().len() != 1
                {
                    return Err(
                        "child accessibility receipt did not attach Livery semantics beneath its declared aperture"
                            .to_owned(),
                    );
                }
                let link = a11y_node(&tree, "Open child destination", Role::Link)?;
                let address = self
                    .workspace
                    .controller(tile)
                    .ok_or("child accessibility receipt lost its document")?
                    .address()
                    .to_owned();
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Focus,
                    target_node: link,
                    data: None,
                }) || self.accessibility.focus != Some(WorkspaceA11yFocus::Document(link))
                    || address != self.config.urls[0]
                {
                    return Err(
                        "child accessibility Focus activated or replaced the Livery document"
                            .to_owned(),
                    );
                }
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let link = a11y_node(&tree, "Open child destination", Role::Link)?;
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: link,
                    data: None,
                }) {
                    return Err(
                        "child accessibility Click did not enter Pelt's content input path"
                            .to_owned(),
                    );
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("child accessibility receipt lost its document after Click")?;
                if !controller
                    .address()
                    .replace('\\', "/")
                    .ends_with("/next.html")
                    || controller.session_generation() != 2
                {
                    return Err(
                        "child accessibility Click did not replace only the focused Livery session"
                            .to_owned(),
                    );
                }
            },
            2 => {
                let tree = self.prepare_accessibility_tree()?;
                let return_link = a11y_node(&tree, "Return to child source", Role::Link)?;
                let action = self
                    .accessibility
                    .action_for(return_link)
                    .ok_or("replacement Livery link has no Pelt action target")?;
                let WorkspaceA11yActionTarget::Document(action) = action else {
                    return Err(
                        "replacement Livery link was incorrectly routed through Frisket".to_owned(),
                    );
                };
                if action.tile != tile
                    || action.session_identity.generation != 2
                    || !action.supports(DocumentA11yAction::Click)
                    || self.accessibility.focus.is_some()
                {
                    return Err(
                        "child accessibility receipt did not publish a fresh replacement subtree"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(ACCESSIBILITY_CHILDREN_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_CHILDREN_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    pub(in crate::workspace_viewer) fn drive_accessibility_scroll_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let tree = self.prepare_accessibility_tree()?;
        let link = livery_a11y_node_for_tile(
            &tree,
            &self.accessibility,
            TileId(1),
            "Open nested destination",
            Role::Link,
        )?;
        let supports = |action| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == link)
                .is_some_and(|(_, node)| node.supports_action(action))
        };
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 2)?;
                let content = self.workspace.content_rect(TileId(1)).ok_or_else(|| {
                    "nested-scroll receipt has no tile-one content hole".to_owned()
                })?;
                if !self.workspace.scroll_at(
                    content.x + content.width.min(180.0) * 0.5,
                    content.y + content.height.min(100.0) * 0.5,
                    0.0,
                    96.0,
                ) {
                    return Err(
                        "nested-scroll fixture did not accept the inducing scroll".to_owned()
                    );
                }
                self.receipt_step = 1;
            },
            1 => {
                if !supports(Action::ScrollIntoView) || supports(Action::Click) {
                    return Err("nested link advertised the wrong post-scroll actions".to_owned());
                }
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::ScrollIntoView,
                    target_node: link,
                    data: None,
                }) {
                    return Err("nested ScrollIntoView was not routed".to_owned());
                }
                self.receipt_step = 2;
            },
            2 => {
                if self.workspace.document_session_generation(TileId(1)) != Some(1)
                    || self.workspace.document_session_generation(TileId(2)) != Some(1)
                {
                    return Err("ScrollIntoView changed a session generation".to_owned());
                }
                self.receipt_step = 3;
                return Ok(Some(ACCESSIBILITY_SCROLL_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_SCROLL_WORKSPACE_ASSERTION.to_owned())),
        }
        Ok(None)
    }

    pub(in crate::workspace_viewer) fn drive_accessibility_click_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let tile = TileId(1);
        let sibling = TileId(2);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 2)?;
                if self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "nested-click receipt began without an installed platform bridge"
                            .to_owned(),
                    );
                }
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("nested-click receipt has no tile-one content hole")?;
                if !self.workspace.scroll_at(
                    content.x + content.width.min(180.0) * 0.5,
                    content.y + content.height.min(100.0) * 0.5,
                    0.0,
                    96.0,
                ) {
                    return Err(
                        "nested-click fixture did not accept the inducing scroll".to_owned()
                    );
                }
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let link = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Open nested destination",
                    Role::Link,
                )?;
                let supports = |action| {
                    tree.nodes
                        .iter()
                        .find(|(id, _)| *id == link)
                        .is_some_and(|(_, node)| node.supports_action(action))
                };
                if !supports(Action::ScrollIntoView) || supports(Action::Click) {
                    return Err(
                        "nested-click target advertised the wrong pre-reveal actions".to_owned(),
                    );
                }
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::ScrollIntoView,
                    target_node: link,
                    data: None,
                }) {
                    return Err("nested-click receipt could not route ScrollIntoView".to_owned());
                }
            },
            2 => {
                let tree = self.prepare_accessibility_tree()?;
                let link = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Open nested destination",
                    Role::Link,
                )?;
                let node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == link)
                    .map(|(_, node)| node)
                    .ok_or("nested-click receipt lost its revealed link")?;
                let action = self
                    .accessibility
                    .action_for(link)
                    .ok_or("nested-click target has no Pelt action target")?;
                let WorkspaceA11yActionTarget::Document(action) = action else {
                    return Err("nested-click target was routed through Frisket".to_owned());
                };
                if !node.supports_action(Action::Click)
                    || !action.supports(DocumentA11yAction::Click)
                    || action.tile != tile
                    || action.session_identity.generation != 1
                {
                    return Err(
                        "nested-click target did not publish a clip-aware tile-local Click"
                            .to_owned(),
                    );
                }
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: link,
                    data: None,
                }) {
                    return Err(
                        "nested-click receipt could not route ordinary pointer Click".to_owned(),
                    );
                }
                if self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: link,
                    data: None,
                }) {
                    return Err(
                        "nested-click receipt accepted a stale pre-navigation action".to_owned(),
                    );
                }
                let focused = self
                    .workspace
                    .controller(tile)
                    .ok_or("nested-click receipt lost its focused controller")?;
                if !focused
                    .address()
                    .replace('\\', "/")
                    .ends_with("/result.html")
                    || focused.session_generation() != 2
                    || !self.workspace.controller(sibling).is_some_and(|other| {
                        other.address().replace('\\', "/").ends_with("/index.html")
                    })
                {
                    return Err(
                        "nested-click receipt changed the wrong tile or retained the old session"
                            .to_owned(),
                    );
                }
                self.receipt_step = 3;
            },
            3 => {
                let focused = self
                    .workspace
                    .controller(tile)
                    .ok_or("nested-click receipt lost its focused controller while settling")?;
                if !matches!(focused.document_state(), PeltDocumentState::Ready) {
                    return Ok(None);
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(ACCESSIBILITY_CLICK_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_CLICK_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }

    pub(in crate::workspace_viewer) fn drive_accessibility_input_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let tile = TileId(1);
        let sibling = TileId(2);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 2)?;
                if self.window.is_some() && self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "accessibility-input receipt began without an installed platform bridge"
                            .to_owned(),
                    );
                }
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("accessibility-input receipt has no tile-one content hole")?;
                if !self.workspace.scroll_at(
                    content.x + content.width.min(190.0) * 0.5,
                    content.y + content.height.min(96.0) * 0.5,
                    0.0,
                    96.0,
                ) {
                    return Err(
                        "accessibility-input fixture did not accept the inducing wheel scroll"
                            .to_owned(),
                    );
                }
                self.receipt_step = 1;
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Nested note",
                    Role::TextInput,
                )?;
                let node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == note)
                    .map(|(_, node)| node)
                    .ok_or("accessibility-input receipt lost its nested textarea")?;
                if node.value() != Some("cedar")
                    || !node.supports_action(Action::ScrollIntoView)
                    || node.supports_action(Action::SetValue)
                {
                    return Err(
                        "scrolled nested textarea did not retain its value while withholding SetValue"
                        .to_owned(),
                    );
                }
                if self.apply_accessibility_request(A11yActionRequest {
                    action: Action::SetValue,
                    target_node: note,
                    data: Some(ActionData::Value("wrong path".into())),
                }) {
                    return Err(
                        "scrolled nested textarea accepted the withheld SetValue action".to_owned(),
                    );
                }
                let sibling_note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    sibling,
                    "Nested note",
                    Role::TextInput,
                )?;
                let sibling_node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == sibling_note)
                    .map(|(_, node)| node)
                    .ok_or("accessibility-input receipt lost its sibling textarea")?;
                if sibling_node.value() != Some("cedar")
                    || sibling_node.supports_action(Action::ScrollIntoView)
                {
                    return Err(
                        "the untouched sibling acquired the focused tile's nested-scroll action"
                            .to_owned(),
                    );
                }

                let text_target = self
                    .workspace
                    .controller(tile)
                    .and_then(|controller| controller.text_target("cedar"))
                    .ok_or(
                        "accessibility-input receipt could not resolve retained textarea text",
                    )?;
                if text_target.anchor == text_target.focus {
                    return Err(
                        "accessibility-input receipt could not resolve a non-empty drag range"
                            .to_owned(),
                    );
                }
                let content = self
                    .workspace
                    .content_rect(tile)
                    .ok_or("accessibility-input receipt lost tile-one content geometry")?;
                let anchor = (
                    content.x + text_target.anchor[0],
                    content.y + text_target.anchor[1],
                );
                let focus = (
                    content.x + text_target.focus[0],
                    content.y + text_target.focus[1],
                );
                if !content.contains(anchor.0, anchor.1) || !content.contains(focus.0, focus.1) {
                    return Err(
                        "accessibility-input receipt resolved textarea drag points outside its tile"
                            .to_owned(),
                    );
                }
                self.pointer_move(anchor.0, anchor.1);
                if !self.pointer_down() {
                    return Err(
                        "ordinary Pelt pointer press did not focus the retained textarea"
                            .to_owned(),
                    );
                }
                self.pointer_move(focus.0, focus.1);
                if !self.pointer_up() {
                    return Err(
                        "ordinary Pelt pointer drag did not finish the retained text selection"
                            .to_owned(),
                    );
                }
                if self
                    .workspace
                    .controller(tile)
                    .and_then(PeltController::clip)
                    .is_some_and(|clip| clip.selector.is_some())
                {
                    return Err(
                        "ordinary editor selection produced a DOM-range document clip".to_owned(),
                    );
                }
                if self.workspace.has_active_pointer_capture() {
                    return Err(
                        "ordinary Pelt pointer drag left a stale physical pointer capture"
                            .to_owned(),
                    );
                }
                let text_effect = self.workspace.input(SessionInput::Text("oak".to_owned()));
                if !text_effect.handled {
                    return Err("SessionInput::Text did not reach the focused textarea".to_owned());
                }
                self.apply_effect(text_effect);
                if self
                    .workspace
                    .controller(tile)
                    .and_then(PeltController::clip)
                    .is_some_and(|clip| clip.selector.is_some())
                {
                    return Err(
                        "text input produced a DOM-range document clip from the editor selection"
                            .to_owned(),
                    );
                }
                for ime in [
                    SessionIme::Enabled,
                    SessionIme::Preedit {
                        text: "+ ime".to_owned(),
                        selection: None,
                    },
                    SessionIme::Commit(" + ime".to_owned()),
                ] {
                    let effect = self.workspace.input(SessionInput::Ime(ime));
                    if !effect.handled {
                        return Err(
                            "SessionInput::Ime did not reach the focused textarea".to_owned()
                        );
                    }
                    self.apply_effect(effect);
                }
                if self
                    .workspace
                    .controller(tile)
                    .and_then(PeltController::clip)
                    .is_some_and(|clip| clip.selector.is_some())
                {
                    return Err(
                        "IME input produced a DOM-range document clip from the editor selection"
                            .to_owned(),
                    );
                }
                self.receipt_step = 2;
            },
            2 => {
                let tree = self.prepare_accessibility_tree()?;
                let note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Nested note",
                    Role::TextInput,
                )?;
                let node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == note)
                    .map(|(_, node)| node)
                    .ok_or("accessibility-input receipt lost its edited textarea")?;
                if node.value() != Some("oak + ime")
                    || !node.supports_action(Action::ScrollIntoView)
                    || node.supports_action(Action::SetValue)
                {
                    return Err(
                        "textarea did not reproject exact `oak + ime` replacement while retaining its nested-scroll action boundary".to_owned(),
                    );
                }
                let sibling_note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    sibling,
                    "Nested note",
                    Role::TextInput,
                )?;
                let sibling_node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == sibling_note)
                    .map(|(_, node)| node)
                    .ok_or("accessibility-input receipt lost its sibling textarea")?;
                if sibling_node.value() != Some("cedar")
                    || self.workspace.document_session_generation(tile) != Some(1)
                    || self.workspace.document_session_generation(sibling) != Some(1)
                {
                    return Err(
                        "physical nested editor input changed the sibling or replaced a session"
                            .to_owned(),
                    );
                }
                self.receipt_step = 3;
                return Ok(Some(ACCESSIBILITY_INPUT_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_INPUT_WORKSPACE_ASSERTION.to_owned())),
        }
        Ok(None)
    }

    pub(in crate::workspace_viewer) fn drive_accessibility_edit_workspace_receipt_step(
        &mut self,
    ) -> Result<Option<String>, String> {
        let tile = TileId(1);
        let sibling = TileId(2);
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 2)?;
                if self.accessibility.status() != BridgeStatus::Installed {
                    return Err(
                        "accessibility edit receipt began without an installed platform bridge"
                            .to_owned(),
                    );
                }
                let tree = self.prepare_accessibility_tree()?;
                let note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Accessible note",
                    Role::TextInput,
                )?;
                let note_node = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == note)
                    .map(|(_, node)| node)
                    .ok_or("accessibility edit receipt lost its note input")?;
                if note_node.value() != Some("cedar")
                    || !note_node.supports_action(Action::SetValue)
                {
                    return Err(
                        "accessibility edit receipt did not expose a writable Livery textarea"
                            .to_owned(),
                    );
                }
                let sibling_note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    sibling,
                    "Accessible note",
                    Role::TextInput,
                )?;
                if tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == sibling_note)
                    .and_then(|(_, node)| node.value())
                    != Some("cedar")
                {
                    return Err(
                        "accessibility edit receipt did not retain the sibling textarea value"
                            .to_owned(),
                    );
                }
                for label in ["Read-only note", "Count", "Password"] {
                    let protected = livery_a11y_node_for_tile(
                        &tree,
                        &self.accessibility,
                        tile,
                        label,
                        Role::TextInput,
                    )?;
                    if tree
                        .nodes
                        .iter()
                        .find(|(id, _)| *id == protected)
                        .is_some_and(|(_, node)| node.supports_action(Action::SetValue))
                        || self.apply_accessibility_request(A11yActionRequest {
                            action: Action::SetValue,
                            target_node: protected,
                            data: Some(ActionData::Value("not writable".into())),
                        })
                    {
                        return Err(format!(
                            "accessibility edit receipt exposed or changed protected Livery control {label:?}"
                        ));
                    }
                }
                if self.apply_accessibility_request(A11yActionRequest {
                    action: Action::SetValue,
                    target_node: note,
                    data: Some(ActionData::NumericValue(4.0)),
                }) || self.apply_accessibility_request(A11yActionRequest {
                    action: Action::SetValue,
                    target_node: note,
                    data: None,
                }) || !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::SetValue,
                    target_node: note,
                    data: Some(ActionData::Value("birch".into())),
                }) {
                    return Err(
                        "accessibility edit receipt did not reject malformed data or apply its value"
                            .to_owned(),
                    );
                }
                if self.workspace.document_session_generation(tile) != Some(1)
                    || !self
                        .workspace
                        .controller(sibling)
                        .is_some_and(|controller| {
                            controller
                                .address()
                                .replace('\\', "/")
                                .ends_with("/index.html")
                        })
                {
                    return Err(
                        "accessibility edit SetValue replaced a session or changed the sibling tile"
                            .to_owned(),
                    );
                }
            },
            1 => {
                let tree = self.prepare_accessibility_tree()?;
                let note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Accessible note",
                    Role::TextInput,
                )?;
                let reprojected = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == note)
                    .map(|(_, node)| node);
                let Some(note_node) = reprojected else {
                    return Err("accessibility edit receipt lost its changed textarea".to_owned());
                };
                if note_node.value() != Some("birch") {
                    return Ok(None);
                }
                if !note_node.supports_action(Action::SetValue) {
                    return Err(
                        "accessibility edit receipt stopped advertising SetValue after its mutation"
                            .to_owned(),
                    );
                }
                let sibling_note = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    sibling,
                    "Accessible note",
                    Role::TextInput,
                )?;
                if tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == sibling_note)
                    .and_then(|(_, node)| node.value())
                    != Some("cedar")
                {
                    return Err(
                        "accessibility edit receipt reprojected its value into the sibling tile"
                            .to_owned(),
                    );
                }
                let submit = livery_a11y_node_for_tile(
                    &tree,
                    &self.accessibility,
                    tile,
                    "Save accessible note",
                    Role::Button,
                )?;
                let submit_action = self.accessibility.action_for(submit);
                if !self.apply_accessibility_request(A11yActionRequest {
                    action: Action::Click,
                    target_node: submit,
                    data: None,
                }) {
                    return Err(format!(
                        "accessibility edit receipt could not submit the mutated Livery form (action target {submit_action:?})"
                    ));
                }
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility edit receipt lost its focused controller")?;
                if !controller
                    .address()
                    .replace('\\', "/")
                    .contains("/result.html?note=birch")
                    || controller.session_generation() != 2
                    || !self.workspace.controller(sibling).is_some_and(|other| {
                        other.address().replace('\\', "/").ends_with("/index.html")
                    })
                {
                    return Err(
                        "accessibility edit receipt did not submit the focused Livery value through its ordinary route"
                            .to_owned(),
                    );
                }
            },
            2 => {
                let controller = self
                    .workspace
                    .controller(tile)
                    .ok_or("accessibility edit receipt lost its submitted controller")?;
                if !matches!(controller.document_state(), PeltDocumentState::Ready) {
                    return Ok(None);
                }
                if !controller
                    .address()
                    .replace('\\', "/")
                    .contains("/result.html?note=birch")
                    || !self.workspace.controller(sibling).is_some_and(|other| {
                        other.address().replace('\\', "/").ends_with("/index.html")
                    })
                {
                    return Err(
                        "accessibility edit receipt did not preserve its submitted route and sibling"
                            .to_owned(),
                    );
                }
                self.receipt_step = self.receipt_step.saturating_add(1);
                return Ok(Some(ACCESSIBILITY_EDIT_WORKSPACE_ASSERTION.to_owned()));
            },
            _ => return Ok(Some(ACCESSIBILITY_EDIT_WORKSPACE_ASSERTION.to_owned())),
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(None)
    }
}
