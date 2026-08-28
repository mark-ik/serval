/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::hash::Hash;

use accesskit::{
    Action, HasPopup, Live, Node as AccessNode, NodeId as AccessNodeId, Orientation, Rect, Role,
    Toggled, Tree, TreeId, TreeUpdate,
};
use genet_livery::LiveryLayout;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

use crate::render::ScrollOffsets;

fn access_id<D: LayoutDom>(dom: &D, node: D::NodeId) -> AccessNodeId {
    AccessNodeId(dom.opaque_id(node))
}

fn role_for<D: LayoutDom>(dom: &D, node: D::NodeId) -> Role {
    if let Some(role) = dom.attribute(node, &Namespace::default(), &LocalName::from("role")) {
        match role {
            "button" => return Role::Button,
            "checkbox" => return Role::CheckBox,
            "radio" => return Role::RadioButton,
            "radiogroup" => return Role::RadioGroup,
            "switch" => return Role::Switch,
            "tab" => return Role::Tab,
            "tablist" => return Role::TabList,
            "tabpanel" => return Role::TabPanel,
            "menu" => return Role::Menu,
            "menuitem" => return Role::MenuItem,
            "menuitemcheckbox" => return Role::MenuItemCheckBox,
            "menuitemradio" => return Role::MenuItemRadio,
            "listbox" => return Role::ListBox,
            "option" => return Role::ListBoxOption,
            "combobox" => return Role::ComboBox,
            "separator" => return Role::Splitter,
            "toolbar" => return Role::Toolbar,
            "tree" => return Role::Tree,
            "treeitem" => return Role::TreeItem,
            "progressbar" => return Role::ProgressIndicator,
            "slider" => return Role::Slider,
            "textbox" => return Role::TextInput,
            "link" => return Role::Link,
            "document" => return Role::Document,
            "main" => return Role::Main,
            "navigation" => return Role::Navigation,
            "region" => return Role::Region,
            "heading" => return Role::Heading,
            "alert" => return Role::Alert,
            "status" => return Role::Status,
            _ => {},
        }
    }
    match dom.kind(node) {
        NodeKind::Document => Role::Window,
        NodeKind::Element => match dom.element_name(node).map(|name| name.local.as_ref()) {
            Some("a")
                if dom
                    .attribute(node, &Namespace::default(), &LocalName::from("href"))
                    .is_some() =>
            {
                Role::Link
            },
            Some("button") => Role::Button,
            Some("input" | "textarea") => Role::TextInput,
            Some("p") => Role::Paragraph,
            Some("label") => Role::Label,
            Some("html") => Role::Document,
            _ => Role::GenericContainer,
        },
        _ => Role::GenericContainer,
    }
}

fn direct_text<D: LayoutDom>(dom: &D, node: D::NodeId) -> String {
    dom.dom_children(node)
        .filter_map(|child| {
            (dom.kind(child) == NodeKind::Text)
                .then(|| dom.text(child))
                .flatten()
        })
        .collect()
}

fn text_control_value<D: LayoutDom>(dom: &D, node: D::NodeId) -> Option<String> {
    let tag = dom.element_name(node).map(|name| name.local.as_ref())?;
    match tag {
        "textarea" => Some(descendant_text(dom, node)),
        "input" => {
            let input_type = dom
                .attribute(node, &Namespace::default(), &LocalName::from("type"))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("text");
            matches!(
                input_type.to_ascii_lowercase().as_str(),
                "text" | "search" | "email" | "url" | "tel"
            )
            .then(|| {
                dom.attribute(node, &Namespace::default(), &LocalName::from("value"))
                    .unwrap_or("")
                    .to_owned()
            })
        },
        _ => None,
    }
}

fn descendant_text<D: LayoutDom>(dom: &D, node: D::NodeId) -> String {
    dom.dom_children(node)
        .map(|child| {
            if dom.kind(child) == NodeKind::Text {
                dom.text(child).unwrap_or("").to_owned()
            } else {
                descendant_text(dom, child)
            }
        })
        .collect()
}

/// One ARIA numeric attribute, when it parses. A malformed value is left unset
/// rather than projected as zero: a reader is better told nothing than told a
/// wrong position.
fn aria_number<D: LayoutDom>(dom: &D, node: D::NodeId, name: &str) -> Option<f64> {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .and_then(|value| value.trim().parse::<f64>().ok())
}

/// One ARIA boolean attribute, when it is explicitly true. An absent or
/// malformed value is left unset rather than inferring an interaction state.
fn aria_true<D: LayoutDom>(dom: &D, node: D::NodeId, name: &str) -> bool {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

/// An ARIA boolean, including an explicit false. Invalid and absent values
/// are left unset so the tree does not claim a state the DOM did not express.
fn aria_bool<D: LayoutDom>(dom: &D, node: D::NodeId, name: &str) -> Option<bool> {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
}

fn aria_toggled<D: LayoutDom>(dom: &D, node: D::NodeId, name: &str) -> Option<Toggled> {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "true" => Some(Toggled::True),
            "false" => Some(Toggled::False),
            "mixed" => Some(Toggled::Mixed),
            _ => None,
        })
}

fn aria_orientation<D: LayoutDom>(dom: &D, node: D::NodeId) -> Option<Orientation> {
    dom.attribute(
        node,
        &Namespace::default(),
        &LocalName::from("aria-orientation"),
    )
    .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
        "horizontal" => Some(Orientation::Horizontal),
        "vertical" => Some(Orientation::Vertical),
        _ => None,
    })
}

fn aria_has_popup<D: LayoutDom>(dom: &D, node: D::NodeId) -> Option<HasPopup> {
    dom.attribute(
        node,
        &Namespace::default(),
        &LocalName::from("aria-haspopup"),
    )
    .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
        "true" | "menu" => Some(HasPopup::Menu),
        "listbox" => Some(HasPopup::Listbox),
        "tree" => Some(HasPopup::Tree),
        "grid" => Some(HasPopup::Grid),
        "dialog" => Some(HasPopup::Dialog),
        "false" | "none" | "" => None,
        _ => None,
    })
}

fn is_disabled<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    aria_true(dom, node, "aria-disabled")
        || dom
            .attribute(node, &Namespace::default(), &LocalName::from("disabled"))
            .is_some()
}

fn aria_live<D: LayoutDom>(dom: &D, node: D::NodeId) -> Option<Live> {
    dom.attribute(node, &Namespace::default(), &LocalName::from("aria-live"))
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Live::Off),
            "polite" => Some(Live::Polite),
            "assertive" => Some(Live::Assertive),
            _ => None,
        })
}

fn is_content_editable<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    dom.attribute(
        node,
        &Namespace::default(),
        &LocalName::from("contenteditable"),
    )
    .is_some_and(|value| {
        let value = value.trim();
        value.is_empty()
            || value.eq_ignore_ascii_case("true")
            || value.eq_ignore_ascii_case("plaintext-only")
    })
}

fn has_tabindex<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    dom.attribute(node, &Namespace::default(), &LocalName::from("tabindex"))
        .is_some_and(|value| value.trim().parse::<i32>().is_ok())
}

/// The accumulated scroll owned by element ancestors. A node's own scroll
/// offset moves its descendants, not its own retained border box.
fn ancestor_scroll<D>(
    dom: &D,
    node: D::NodeId,
    scroll_offsets: &ScrollOffsets<D::NodeId>,
) -> (f32, f32)
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut total = (0.0, 0.0);
    let mut current = dom.parent(node);
    while let Some(parent) = current {
        if let Some((x, y)) = scroll_offsets.get(&parent) {
            total.0 += x;
            total.1 += y;
        }
        current = dom.parent(parent);
    }
    total
}

/// An active nested scrollport moves its descendant bounds. Pelt can route a
/// retained `ScrollIntoView` request back to Livery, but it does not yet own
/// the corresponding nested-pointer route. Keep descendants semantic and
/// focusable, advertise only the reveal action, and withhold Click.
fn has_active_scrolled_ancestor<D>(
    dom: &D,
    node: D::NodeId,
    scroll_offsets: &ScrollOffsets<D::NodeId>,
) -> bool
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut current = dom.parent(node);
    while let Some(parent) = current {
        if scroll_offsets
            .get(&parent)
            .is_some_and(|&(x, y)| x != 0.0 || y != 0.0)
        {
            return true;
        }
        current = dom.parent(parent);
    }
    false
}

fn is_native_control<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    matches!(
        dom.element_name(node).map(|name| name.local.as_ref()),
        Some("button" | "input" | "select" | "textarea")
    ) || (dom.element_name(node).map(|name| name.local.as_ref()) == Some("a")
        && dom
            .attribute(node, &Namespace::default(), &LocalName::from("href"))
            .is_some())
}

fn supports_semantic_action(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::Switch
            | Role::Tab
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::Slider
            | Role::TextInput
            | Role::Link
    )
}

fn walk<D>(
    dom: &D,
    fragments: &LiveryLayout<D::NodeId>,
    scroll_offsets: Option<&ScrollOffsets<D::NodeId>>,
    node: D::NodeId,
    out: &mut Vec<(AccessNodeId, AccessNode)>,
) -> Vec<AccessNodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if aria_true(dom, node, "aria-hidden") {
        return Vec::new();
    }
    let children: Vec<_> = dom
        .dom_children(node)
        .filter(|child| dom.kind(*child) == NodeKind::Element)
        .flat_map(|child| walk(dom, fragments, scroll_offsets, child, out))
        .collect();
    // The synthetic document root anchors the platform tree even though it
    // has no layout fragment. Other fragment-less elements are not painted;
    // promote any visible descendants instead of making the shell invent
    // geometry for a hidden control.
    if dom.kind(node) != NodeKind::Document && fragments.get(node).is_none() {
        return children;
    }
    let id = access_id(dom, node);
    let role = role_for(dom, node);
    let mut access = AccessNode::new(role);
    let label = dom
        .attribute(node, &Namespace::default(), &LocalName::from("aria-label"))
        .map(str::to_owned)
        .unwrap_or_else(|| direct_text(dom, node));
    if !label.is_empty() {
        access.set_label(label);
    }
    let read_only = aria_true(dom, node, "aria-readonly")
        || dom
            .attribute(node, &Namespace::default(), &LocalName::from("readonly"))
            .is_some();
    if read_only {
        access.set_read_only();
    }
    if let Some(live) = aria_live(dom, node) {
        access.set_live(live);
    }
    let disabled = is_disabled(dom, node);
    if disabled {
        access.set_disabled();
    }
    if let Some(selected) = aria_bool(dom, node, "aria-selected") {
        access.set_selected(selected);
    }
    if let Some(expanded) = aria_bool(dom, node, "aria-expanded") {
        access.set_expanded(expanded);
    }
    if let Some(toggled) =
        aria_toggled(dom, node, "aria-checked").or_else(|| aria_toggled(dom, node, "aria-pressed"))
    {
        access.set_toggled(toggled);
    }
    if let Some(orientation) = aria_orientation(dom, node) {
        access.set_orientation(orientation);
    }
    if let Some(has_popup) = aria_has_popup(dom, node) {
        access.set_has_popup(has_popup);
    }
    let semantic_control = is_native_control(dom, node) || supports_semantic_action(role);
    let focusable = semantic_control || has_tabindex(dom, node) || is_content_editable(dom, node);
    let action_blocked_by_nested_scroll = scroll_offsets
        .is_some_and(|scroll_offsets| has_active_scrolled_ancestor(dom, node, scroll_offsets));
    if !disabled && action_blocked_by_nested_scroll {
        // The current retained scroll offsets identify the exact document-local
        // scroller Livery must adjust. Until Pelt has refreshed this tree with
        // the revealed bounds, a pointer action would still be stale.
        access.add_action(Action::ScrollIntoView);
    }
    if !disabled
        && !action_blocked_by_nested_scroll
        && (semantic_control || is_content_editable(dom, node))
    {
        access.add_action(Action::Click);
    }
    if !disabled && focusable {
        access.add_action(Action::Focus);
    }
    if let Some(value) = text_control_value(dom, node) {
        access.set_value(value);
        if !disabled && !read_only && !action_blocked_by_nested_scroll {
            access.add_action(Action::SetValue);
        }
    }
    // A progress bar or slider whose value never reaches the tree is
    // decoration: the reader is told it exists but not how far along it is.
    if let Some(value) = aria_number(dom, node, "aria-valuenow") {
        access.set_numeric_value(value);
    }
    if let Some(value) = aria_number(dom, node, "aria-valuemin") {
        access.set_min_numeric_value(value);
    }
    if let Some(value) = aria_number(dom, node, "aria-valuemax") {
        access.set_max_numeric_value(value);
    }
    if let Some(fragment) = fragments.get(node) {
        let (scroll_x, scroll_y) = scroll_offsets
            .map(|scroll_offsets| ancestor_scroll(dom, node, scroll_offsets))
            .unwrap_or_default();
        access.set_bounds(Rect::new(
            (fragment.x - scroll_x) as f64,
            (fragment.y - scroll_y) as f64,
            (fragment.x + fragment.width - scroll_x) as f64,
            (fragment.y + fragment.height - scroll_y) as f64,
        ));
    }
    access.set_children(children);
    out.push((id, access));
    vec![id]
}

/// Project a Livery/Buckram document into an AccessKit tree.
pub fn accesskit_tree<D>(
    dom: &D,
    fragments: &LiveryLayout<D::NodeId>,
    focus: Option<D::NodeId>,
) -> TreeUpdate
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    accesskit_tree_with_optional_scroll(dom, fragments, focus, None)
}

/// Project a retained document after Livery has applied nested element scroll.
///
/// Bounds move with each scrolled ancestor. Enabled descendants of an active
/// nested scrollport advertise `ScrollIntoView`, while Click remains withheld
/// until Pelt owns the matching refreshed pointer-routing semantics.
pub fn accesskit_tree_with_scroll<D>(
    dom: &D,
    fragments: &LiveryLayout<D::NodeId>,
    focus: Option<D::NodeId>,
    scroll_offsets: &ScrollOffsets<D::NodeId>,
) -> TreeUpdate
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    accesskit_tree_with_optional_scroll(dom, fragments, focus, Some(scroll_offsets))
}

fn accesskit_tree_with_optional_scroll<D>(
    dom: &D,
    fragments: &LiveryLayout<D::NodeId>,
    focus: Option<D::NodeId>,
    scroll_offsets: Option<&ScrollOffsets<D::NodeId>>,
) -> TreeUpdate
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let root = dom.document();
    let mut nodes = Vec::new();
    walk(dom, fragments, scroll_offsets, root, &mut nodes);
    let requested_focus = access_id(dom, focus.unwrap_or(root));
    let focus = nodes
        .iter()
        .any(|(candidate, _)| *candidate == requested_focus)
        .then_some(requested_focus)
        .unwrap_or_else(|| access_id(dom, root));
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(access_id(dom, root))),
        tree_id: TreeId::ROOT,
        focus,
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, HasPopup, Live, Node as AccessNode, Orientation, Role, Toggled};
    use genet_scripted_dom::ScriptedDom;
    use layout_dom_api::{LayoutDom, LayoutDomMut, NodeKind};

    use super::{accesskit_tree, accesskit_tree_with_scroll};
    use crate::{ScrollOffsets, fragments_from_scripted_dom};

    const SHEET: &[&str] = &["div { display: block; }"];

    fn nodes_for(html: &str) -> Vec<AccessNode> {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        dom.set_inner_html(root, html);
        let fragments = fragments_from_scripted_dom(&dom, SHEET, 400, 300).expect("layout");
        accesskit_tree(&dom, &fragments, None)
            .nodes
            .into_iter()
            .map(|(_, node)| node)
            .collect()
    }

    fn with_role(html: &str, role: Role) -> AccessNode {
        nodes_for(html)
            .into_iter()
            .find(|node| node.role() == role)
            .unwrap_or_else(|| panic!("no node projected with role {role:?}"))
    }

    #[test]
    fn a_refusal_projects_as_an_alert() {
        let nodes = nodes_for("<div role=\"alert\">Cannot flash this board</div>");
        assert!(
            nodes.iter().any(|node| node.role() == Role::Alert),
            "role=alert must reach the reader as an alert, not a generic container",
        );
    }

    #[test]
    fn a_progress_bar_carries_its_value() {
        let bar = with_role(
            "<div role=\"progressbar\" aria-valuenow=\"50\" aria-valuemin=\"0\" aria-valuemax=\"100\"></div>",
            Role::ProgressIndicator,
        );
        assert_eq!(bar.numeric_value(), Some(50.0));
        assert_eq!(bar.min_numeric_value(), Some(0.0));
        assert_eq!(bar.max_numeric_value(), Some(100.0));
    }

    #[test]
    fn a_malformed_value_is_left_unset() {
        let bar = with_role(
            "<div role=\"progressbar\" aria-valuenow=\"soon\"></div>",
            Role::ProgressIndicator,
        );
        assert_eq!(bar.numeric_value(), None);
    }

    #[test]
    fn a_read_only_document_keeps_its_document_semantics() {
        let document = with_role(
            "<div role=\"document\" aria-readonly=\"true\">Read-only notes</div>",
            Role::Document,
        );
        assert!(document.is_read_only());
    }

    #[test]
    fn landmarks_and_status_reach_the_reader() {
        let nodes = nodes_for(
            "<section role=\"region\" aria-label=\"Related notes\"></section>\
             <div role=\"status\">Synced</div>",
        );
        assert!(nodes.iter().any(|node| node.role() == Role::Region));
        assert!(nodes.iter().any(|node| node.role() == Role::Status));
    }

    #[test]
    fn controls_advertise_click_and_focus_separately() {
        let button = with_role("<button>Open</button>", Role::Button);
        assert!(button.supports_action(Action::Click));
        assert!(button.supports_action(Action::Focus));

        let focusable = nodes_for("<div tabindex=\"0\">Focus only</div>")
            .into_iter()
            .find(|node| node.role() == Role::GenericContainer)
            .expect("focusable div");
        assert!(focusable.supports_action(Action::Focus));
        assert!(!focusable.supports_action(Action::Click));
    }

    #[test]
    fn native_links_project_link_semantics_only_when_navigable() {
        let nodes = nodes_for(
            "<a href=\"next.html\" style=\"display:block\">Navigate</a>\
             <a style=\"display:block\">Named anchor</a>",
        );
        let link = nodes
            .iter()
            .find(|node| node.label() == Some("Navigate"))
            .expect("native link");
        assert_eq!(link.role(), Role::Link);
        assert!(link.supports_action(Action::Click));
        assert!(link.supports_action(Action::Focus));

        let anchor = nodes
            .iter()
            .find(|node| node.label() == Some("Named anchor"))
            .expect("anchor without href");
        assert_eq!(anchor.role(), Role::GenericContainer);
        assert!(!anchor.supports_action(Action::Click));
        assert!(!anchor.supports_action(Action::Focus));
    }

    #[test]
    fn contenteditable_nodes_are_clickable_and_focusable() {
        let editor = with_role(
            "<div role=\"textbox\" contenteditable>Notes</div>",
            Role::TextInput,
        );
        assert!(editor.supports_action(Action::Click));
        assert!(editor.supports_action(Action::Focus));
    }

    #[test]
    fn writable_text_controls_project_values_and_set_value() {
        let nodes = nodes_for(
            "<input value=\"find me\"><input type=\"search\" value=\"search me\">
             <textarea>write me</textarea>",
        );
        let values: Vec<_> = nodes.iter().filter_map(AccessNode::value).collect();
        assert!(values.contains(&"find me"));
        assert!(values.contains(&"search me"));
        assert!(values.contains(&"write me"));
        assert_eq!(values.len(), 3);
        assert_eq!(
            nodes
                .iter()
                .filter(|node| node.supports_action(Action::SetValue))
                .count(),
            3
        );
    }

    #[test]
    fn readonly_disabled_and_non_text_inputs_do_not_advertise_set_value() {
        let nodes = nodes_for(
            "<input readonly value=\"read only\"><input disabled value=\"disabled\">
             <input type=\"number\" value=\"42\"><input type=\"checkbox\" value=\"yes\">
             <textarea aria-readonly=\"true\">locked</textarea>",
        );
        assert!(
            nodes
                .iter()
                .all(|node| !node.supports_action(Action::SetValue))
        );
        assert_eq!(
            nodes.iter().filter(|node| node.value().is_some()).count(),
            3
        );
        assert_eq!(nodes.iter().filter(|node| node.is_read_only()).count(), 2);
    }

    #[test]
    fn nested_scroll_withholds_text_control_set_value() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        dom.set_inner_html(root, "<div><input value=\"nested\"></div>");
        let container = dom
            .dom_children(root)
            .find(|node| dom.kind(*node) == NodeKind::Element)
            .expect("scroll container");
        let input = dom
            .dom_children(container)
            .find(|node| dom.kind(*node) == NodeKind::Element)
            .expect("input");
        let fragments = fragments_from_scripted_dom(&dom, SHEET, 400, 300).expect("layout");
        let mut offsets = ScrollOffsets::new();
        offsets.insert(container, (0.0, 24.0));
        let tree = accesskit_tree_with_scroll(&dom, &fragments, Some(input), &offsets);
        let node = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == super::access_id(&dom, input))
            .map(|(_, node)| node)
            .expect("scrolled input");
        assert_eq!(node.value(), Some("nested"));
        assert!(!node.supports_action(Action::SetValue));
        assert!(node.supports_action(Action::ScrollIntoView));
    }

    #[test]
    fn disabled_nested_controls_do_not_advertise_scroll_into_view() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        dom.set_inner_html(
            root,
            "<div><button disabled>Unavailable</button><div role=\"link\" aria-disabled=\"true\" tabindex=\"0\">Also unavailable</div></div>",
        );
        let container = dom
            .dom_children(root)
            .find(|node| dom.kind(*node) == NodeKind::Element)
            .expect("scroll container");
        let fragments = fragments_from_scripted_dom(&dom, SHEET, 400, 300).expect("layout");
        let mut offsets = ScrollOffsets::new();
        offsets.insert(container, (0.0, 24.0));
        let tree = accesskit_tree_with_scroll(&dom, &fragments, None, &offsets);

        for label in ["Unavailable", "Also unavailable"] {
            let node = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .map(|(_, node)| node)
                .expect("disabled nested control");
            assert!(node.is_disabled());
            assert!(!node.supports_action(Action::Click));
            assert!(!node.supports_action(Action::Focus));
            assert!(!node.supports_action(Action::ScrollIntoView));
            assert!(!node.supports_action(Action::SetValue));
        }
    }

    #[test]
    fn hidden_controls_do_not_enter_the_tree() {
        let nodes = nodes_for(
            "<button style=\"display: none\">Paint hidden</button>\
             <button aria-hidden=\"true\">ARIA hidden</button>\
             <button>Visible</button>",
        );
        let labels: Vec<_> = nodes.iter().filter_map(AccessNode::label).collect();
        assert!(!labels.contains(&"Paint hidden"));
        assert!(!labels.contains(&"ARIA hidden"));
        assert!(labels.contains(&"Visible"));
    }

    #[test]
    fn live_regions_and_disabled_controls_keep_their_state() {
        let nodes = nodes_for(
            "<div role=\"status\" aria-live=\"polite\">Saved</div>\
             <button disabled>Unavailable</button>\
             <div role=\"button\" aria-disabled=\"true\">Also unavailable</div>",
        );
        let status = nodes
            .iter()
            .find(|node| node.role() == Role::Status)
            .expect("status");
        assert_eq!(status.live(), Some(Live::Polite));
        for disabled in nodes.iter().filter(|node| node.is_disabled()) {
            assert!(!disabled.supports_action(Action::Click));
            assert!(!disabled.supports_action(Action::Focus));
        }
        assert_eq!(nodes.iter().filter(|node| node.is_disabled()).count(), 2);
    }

    #[test]
    fn aria_widget_roles_and_states_reach_accesskit() {
        let nodes = nodes_for(
            "<div role=\"menu\" aria-label=\"Actions\">\
                <div role=\"menuitemradio\" aria-label=\"Compact\" aria-checked=\"true\" aria-selected=\"true\">Compact</div>\
                <div role=\"menuitemcheckbox\" aria-label=\"Details\" aria-checked=\"mixed\">Details</div>\
            </div>\
            <div role=\"separator\" aria-orientation=\"vertical\"></div>\
            <button aria-expanded=\"false\" aria-haspopup=\"dialog\">Details</button>",
        );

        let menu = nodes
            .iter()
            .find(|node| node.role() == Role::Menu)
            .expect("menu role");
        assert_eq!(menu.label(), Some("Actions"));

        let radio = nodes
            .iter()
            .find(|node| node.role() == Role::MenuItemRadio)
            .expect("menuitemradio role");
        assert_eq!(radio.toggled(), Some(Toggled::True));
        assert_eq!(radio.is_selected(), Some(true));
        assert!(radio.supports_action(Action::Click));

        let mixed = nodes
            .iter()
            .find(|node| node.label() == Some("Details") && node.role() == Role::MenuItemCheckBox)
            .expect("menuitemcheckbox role");
        assert_eq!(mixed.toggled(), Some(Toggled::Mixed));

        let separator = nodes
            .iter()
            .find(|node| node.role() == Role::Splitter)
            .expect("separator role");
        assert_eq!(separator.orientation(), Some(Orientation::Vertical));

        let trigger = nodes
            .iter()
            .find(|node| node.role() == Role::Button && node.label() == Some("Details"))
            .expect("popup trigger");
        assert_eq!(trigger.is_expanded(), Some(false));
        assert_eq!(trigger.has_popup(), Some(HasPopup::Dialog));
    }

    #[test]
    fn aria_pressed_and_bounds_are_projected_without_inference() {
        let nodes = nodes_for(
            "<button style=\"display:block;width:80px;height:20px\" aria-pressed=\"mixed\">Filter</button>\
             <div role=\"button\" aria-expanded=\"maybe\" aria-haspopup=\"unknown\">Invalid</div>",
        );
        let filter = nodes
            .iter()
            .find(|node| node.label() == Some("Filter"))
            .expect("filter button");
        assert_eq!(filter.toggled(), Some(Toggled::Mixed));
        let bounds = filter.bounds().expect("laid out button bounds");
        assert_eq!(bounds.x1 - bounds.x0, 80.0);
        assert_eq!(bounds.y1 - bounds.y0, 20.0);

        let invalid = nodes
            .iter()
            .find(|node| node.label() == Some("Invalid"))
            .expect("invalid state button");
        assert_eq!(invalid.is_expanded(), None);
        assert_eq!(invalid.has_popup(), None);
    }

    #[test]
    fn nested_scroll_offsets_bounds_and_withholds_descendant_click() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        dom.set_inner_html(
            root,
            "<div><div role=\"link\" tabindex=\"0\" style=\"display:block;width:80px;height:20px\">Scrolled action</div></div>",
        );
        let container = dom
            .dom_children(root)
            .find(|node| dom.kind(*node) == NodeKind::Element)
            .expect("scroll container");
        let fragments = fragments_from_scripted_dom(&dom, SHEET, 400, 300).expect("layout");
        let before = accesskit_tree(&dom, &fragments, None);
        let before_link = before
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Scrolled action"))
            .map(|(_, node)| node)
            .expect("unscrolled link");
        let before_bounds = before_link.bounds().expect("unscrolled bounds");

        let mut offsets = ScrollOffsets::new();
        offsets.insert(container, (0.0, 24.0));
        let after = accesskit_tree_with_scroll(&dom, &fragments, None, &offsets);
        let after_link = after
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Scrolled action"))
            .map(|(_, node)| node)
            .expect("scrolled link");
        let after_bounds = after_link.bounds().expect("scrolled bounds");

        assert_eq!(after_bounds.x0, before_bounds.x0);
        assert_eq!(after_bounds.y0, before_bounds.y0 - 24.0);
        assert!(after_link.supports_action(Action::Focus));
        assert!(
            !after_link.supports_action(Action::Click),
            "an active nested scrollport cannot advertise a stale Click target"
        );
        assert!(after_link.supports_action(Action::ScrollIntoView));
    }
}
