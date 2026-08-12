//! HTML presentational hints projected into the CSS cascade.
//!
//! The provider contract is deliberately DOM-neutral. HTML collection lives
//! here at the Genet boundary; Livery receives only ordinary typed longhands
//! at its dedicated cascade origin.

use std::{collections::HashMap, hash::Hash};

use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use livery::{
    PropertyId, PropertyValue,
    cascade::{Declaration, DeclaredValue},
    values::{
        BorderCollapse, BorderStyle, BorderWidth, CaptionSide, ComputedColor, Float, Length,
        LengthPercentage, Margin, Padding, Size, TableBorderSpacing, TextAlign,
    },
};

use crate::legacy_color::parse_legacy_color;

/// A provider for typed presentational-hint declarations keyed by host node
/// identity. The output contains neither custom properties nor layer data.
pub trait PresentationalHintProvider<Id> {
    fn declarations_for(&self, id: Id) -> Option<&PresentationalDeclarations>;

    fn descendant_alignment_for(&self, _id: Id) -> Option<LegacyDescendantAlignment> {
        None
    }
}

/// HTML's non-CSS request to adjust the used margins of a qualifying
/// descendant. The deepest applicable legacy alignment owner wins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyDescendantAlignment {
    LineLeft,
    Center,
    LineRight,
}

/// A diagnostic from presentational-hint collection or contract validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresentationalHintDiagnostic {
    InvalidNonNegativeInteger {
        attribute: &'static str,
        value: String,
    },
    InvalidDimension {
        attribute: &'static str,
        value: String,
        nonzero: bool,
    },
    InvalidLegacyColor {
        attribute: &'static str,
        value: String,
    },
    ImportantDeclarationRejected {
        property: PropertyId,
    },
}

/// Typed declarations for one source element.
///
/// Construction is intentionally mediated by [`Self::push`], which rejects
/// `!important`. Custom properties and cascade layers have no representation
/// in this type, so a provider cannot smuggle either through the seam.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentationalDeclarations {
    declarations: Vec<Declaration>,
    diagnostics: Vec<PresentationalHintDiagnostic>,
}

impl PresentationalDeclarations {
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    pub fn diagnostics(&self) -> &[PresentationalHintDiagnostic] {
        &self.diagnostics
    }

    pub fn push(&mut self, declaration: Declaration) {
        if declaration.important {
            self.diagnostics
                .push(PresentationalHintDiagnostic::ImportantDeclarationRejected {
                    property: declaration.property,
                });
            return;
        }
        self.declarations.push(declaration);
    }

    fn invalid_non_negative_integer(&mut self, attribute: &'static str, value: &str) {
        self.diagnostics
            .push(PresentationalHintDiagnostic::InvalidNonNegativeInteger {
                attribute,
                value: value.to_owned(),
            });
    }

    fn invalid_dimension(&mut self, attribute: &'static str, value: &str, nonzero: bool) {
        self.diagnostics
            .push(PresentationalHintDiagnostic::InvalidDimension {
                attribute,
                value: value.to_owned(),
                nonzero,
            });
    }

    fn invalid_legacy_color(&mut self, attribute: &'static str, value: &str) {
        self.diagnostics
            .push(PresentationalHintDiagnostic::InvalidLegacyColor {
                attribute,
                value: value.to_owned(),
            });
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty() && self.diagnostics.is_empty()
    }
}

/// A precomputed presentational-hint map. Providers can build their own maps;
/// [`Self::from_html_dom`] is the Genet HTML implementation for PH1.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationalHints<Id: Eq + Hash> {
    by_node: HashMap<Id, PresentationalDeclarations>,
    descendant_alignment: HashMap<Id, LegacyDescendantAlignment>,
}

impl<Id> Default for PresentationalHints<Id>
where
    Id: Eq + Hash,
{
    fn default() -> Self {
        Self {
            by_node: HashMap::new(),
            descendant_alignment: HashMap::new(),
        }
    }
}

impl<Id> PresentationalHints<Id>
where
    Id: Copy + Eq + Hash,
{
    pub fn declarations_for_mut(&mut self, id: Id) -> &mut PresentationalDeclarations {
        self.by_node.entry(id).or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.by_node.is_empty() && self.descendant_alignment.is_empty()
    }

    /// Collect the bounded HTML table attributes admitted through PH2.
    ///
    /// `cellpadding` is expanded onto each cell belonging to its table. The
    /// traversal stops at nested tables so a nested table's cells never inherit
    /// the outer table's hint.
    pub fn from_html_dom<D>(dom: &D) -> Self
    where
        D: LayoutDom<NodeId = Id>,
    {
        fn visit<D>(
            dom: &D,
            id: D::NodeId,
            inherited_alignment: Option<LegacyDescendantAlignment>,
            hints: &mut PresentationalHints<D::NodeId>,
        ) where
            D: LayoutDom,
            D::NodeId: Copy + Eq + Hash,
        {
            let mut child_alignment = inherited_alignment;
            if dom.kind(id) == NodeKind::Element {
                collect_table_part_hints(dom, id, hints);
                collect_text_alignment_hint(dom, id, hints);
                let (has_own_alignment, owned_alignment) = legacy_alignment_behavior(dom, id);
                if !has_own_alignment && let Some(alignment) = inherited_alignment {
                    hints.descendant_alignment.insert(id, alignment);
                }
                child_alignment = if has_own_alignment {
                    owned_alignment
                } else {
                    owned_alignment.or(inherited_alignment)
                };
            }
            if is_html_element(dom, id, "table") {
                collect_table_hints(dom, id, hints);
            }
            for child in dom.dom_children(id) {
                visit(dom, child, child_alignment, hints);
            }
        }

        let mut hints = Self::default();
        visit(dom, dom.document(), None, &mut hints);
        hints
    }
}

fn collect_text_alignment_hint<D>(
    dom: &D,
    id: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(name) = dom.element_name(id) else {
        return;
    };
    if name.ns.as_ref() != "http://www.w3.org/1999/xhtml" {
        return;
    }
    let local = name.local.as_ref().to_ascii_lowercase();
    let value = if local == "center" {
        Some(TextAlign::Center)
    } else if matches!(
        local.as_str(),
        "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    ) {
        html_attribute(dom, id, "align").and_then(legacy_text_alignment)
    } else {
        None
    };
    if let Some(value) = value {
        hints.declarations_for_mut(id).push(Declaration {
            property: PropertyId::TextAlign,
            value: DeclaredValue::Value(PropertyValue::TextAlign(value)),
            important: false,
        });
    }
}

fn legacy_text_alignment(raw: &str) -> Option<TextAlign> {
    match raw.to_ascii_lowercase().as_str() {
        "center" | "middle" => Some(TextAlign::Center),
        "left" => Some(TextAlign::Left),
        "right" => Some(TextAlign::Right),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

/// Return whether this element has an applicable alignment attribute and the
/// alignment it establishes for descendants. Some applicable values, such as
/// `absmiddle` on a table cell, deliberately suppress an ancestor without
/// establishing a new owner.
fn legacy_alignment_behavior<D>(dom: &D, id: D::NodeId) -> (bool, Option<LegacyDescendantAlignment>)
where
    D: LayoutDom,
{
    let Some(name) = dom.element_name(id) else {
        return (false, None);
    };
    if name.ns.as_ref() != "http://www.w3.org/1999/xhtml" {
        return (false, None);
    }
    let local = name.local.as_ref().to_ascii_lowercase();
    if local == "center" {
        return (false, Some(LegacyDescendantAlignment::Center));
    }
    let Some(raw) = html_attribute(dom, id, "align") else {
        return (false, None);
    };
    let raw = raw.to_ascii_lowercase();
    let owner = match local.as_str() {
        "div" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" => match raw.as_str() {
            "left" | "justify" => Some(LegacyDescendantAlignment::LineLeft),
            "center" | "middle" => Some(LegacyDescendantAlignment::Center),
            "right" => Some(LegacyDescendantAlignment::LineRight),
            "absmiddle" if !matches!(local.as_str(), "div") => return (true, None),
            _ => return (false, None),
        },
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            return (legacy_text_alignment(&raw).is_some(), None);
        },
        "table" => return (matches!(raw.as_str(), "left" | "right" | "center"), None),
        "caption" => return (matches!(raw.as_str(), "top" | "bottom"), None),
        _ => return (false, None),
    };
    (true, owner)
}

fn collect_table_part_hints<D>(dom: &D, id: D::NodeId, hints: &mut PresentationalHints<D::NodeId>)
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(name) = dom.element_name(id) else {
        return;
    };
    if name.ns.as_ref() != "http://www.w3.org/1999/xhtml" {
        return;
    }

    match name.local.as_ref().to_ascii_lowercase().as_str() {
        "table" => {
            collect_legacy_color_hint(dom, id, hints, "bgcolor", PropertyId::BackgroundColor);
            collect_table_border_color_hint(dom, id, hints);
            collect_dimension_hint(dom, id, hints, "width", PropertyId::Width, true);
            collect_dimension_hint(dom, id, hints, "height", PropertyId::Height, false);
            collect_table_alignment_hint(dom, id, hints);
        },
        "caption" => collect_caption_alignment_hint(dom, id, hints),
        "col" => collect_dimension_hint(dom, id, hints, "width", PropertyId::Width, false),
        "thead" | "tbody" | "tfoot" | "tr" => {
            collect_legacy_color_hint(dom, id, hints, "bgcolor", PropertyId::BackgroundColor);
            collect_dimension_hint(dom, id, hints, "height", PropertyId::Height, false);
            collect_table_part_alignment_hint(dom, id, hints);
        },
        "td" | "th" => {
            collect_legacy_color_hint(dom, id, hints, "bgcolor", PropertyId::BackgroundColor);
            collect_dimension_hint(dom, id, hints, "width", PropertyId::Width, true);
            collect_dimension_hint(dom, id, hints, "height", PropertyId::Height, true);
            collect_table_part_alignment_hint(dom, id, hints);
        },
        _ => {},
    }
}

fn collect_legacy_color_hint<D>(
    dom: &D,
    id: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
    attribute: &'static str,
    property: PropertyId,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(raw) = html_attribute(dom, id, attribute) else {
        return;
    };
    let Some(color) = parse_legacy_color(raw) else {
        hints
            .declarations_for_mut(id)
            .invalid_legacy_color(attribute, raw);
        return;
    };
    push_value(
        hints.declarations_for_mut(id),
        property,
        PropertyValue::Color(color),
    );
}

fn collect_table_border_color_hint<D>(
    dom: &D,
    table: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(raw) = html_attribute(dom, table, "bordercolor") else {
        return;
    };
    let Some(color) = parse_legacy_color(raw) else {
        hints
            .declarations_for_mut(table)
            .invalid_legacy_color("bordercolor", raw);
        return;
    };
    push_border_colors(hints.declarations_for_mut(table), color);
}

fn collect_caption_alignment_hint<D>(
    dom: &D,
    caption: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if html_attribute(dom, caption, "align").is_some_and(|raw| raw.eq_ignore_ascii_case("bottom")) {
        hints.declarations_for_mut(caption).push(Declaration {
            property: PropertyId::CaptionSide,
            value: DeclaredValue::Value(PropertyValue::CaptionSide(CaptionSide::Bottom)),
            important: false,
        });
    }
}

fn collect_dimension_hint<D>(
    dom: &D,
    id: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
    attribute: &'static str,
    property: PropertyId,
    nonzero: bool,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(raw) = html_attribute(dom, id, attribute) else {
        return;
    };
    let Some(value) = parse_dimension(raw).filter(|value| !nonzero || !value.is_zero()) else {
        hints
            .declarations_for_mut(id)
            .invalid_dimension(attribute, raw, nonzero);
        return;
    };
    hints.declarations_for_mut(id).push(Declaration {
        property,
        value: DeclaredValue::Value(PropertyValue::Size(Size::Value(value.into_css_value()))),
        important: false,
    });
}

fn collect_table_alignment_hint<D>(
    dom: &D,
    table: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(raw) = html_attribute(dom, table, "align") else {
        return;
    };
    let declarations = hints.declarations_for_mut(table);
    match raw.to_ascii_lowercase().as_str() {
        "left" => declarations.push(Declaration {
            property: PropertyId::Float,
            value: DeclaredValue::Value(PropertyValue::Float(Float::Left)),
            important: false,
        }),
        "right" => declarations.push(Declaration {
            property: PropertyId::Float,
            value: DeclaredValue::Value(PropertyValue::Float(Float::Right)),
            important: false,
        }),
        "center" => {
            for property in [PropertyId::MarginInlineStart, PropertyId::MarginInlineEnd] {
                declarations.push(Declaration {
                    property,
                    value: DeclaredValue::Value(PropertyValue::Margin(Margin::Auto)),
                    important: false,
                });
            }
        },
        _ => {},
    }
}

fn collect_table_part_alignment_hint<D>(
    dom: &D,
    id: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(raw) = html_attribute(dom, id, "align") else {
        return;
    };
    let value = match raw.to_ascii_lowercase().as_str() {
        "center" | "middle" | "absmiddle" => TextAlign::Center,
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        "justify" => TextAlign::Justify,
        _ => return,
    };
    hints.declarations_for_mut(id).push(Declaration {
        property: PropertyId::TextAlign,
        value: DeclaredValue::Value(PropertyValue::TextAlign(value)),
        important: false,
    });
}

impl<Id> PresentationalHintProvider<Id> for PresentationalHints<Id>
where
    Id: Copy + Eq + Hash,
{
    fn declarations_for(&self, id: Id) -> Option<&PresentationalDeclarations> {
        self.by_node.get(&id)
    }

    fn descendant_alignment_for(&self, id: Id) -> Option<LegacyDescendantAlignment> {
        self.descendant_alignment.get(&id).copied()
    }
}

fn collect_table_hints<D>(dom: &D, table: D::NodeId, hints: &mut PresentationalHints<D::NodeId>)
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let rules = html_attribute(dom, table, "rules").and_then(LegacyTableRules::parse);
    if rules.is_some() {
        let declarations = hints.declarations_for_mut(table);
        push_border_styles(declarations, [BorderStyle::Hidden; 4]);
        push_value(
            declarations,
            PropertyId::BorderCollapse,
            PropertyValue::BorderCollapse(BorderCollapse::Collapse),
        );
    }

    let border = html_attribute(dom, table, "border").map(|raw| {
        if let Some(px) = parse_non_negative_integer_px(raw) {
            (px, px != 0.0)
        } else {
            hints
                .declarations_for_mut(table)
                .invalid_non_negative_integer("border", raw);
            (1.0, true)
        }
    });
    if let Some((px, equivalent_nonzero)) = border {
        let declarations = hints.declarations_for_mut(table);
        push_border_widths(declarations, BorderWidth::Length(Length::px(px)));
        if equivalent_nonzero {
            push_border_styles(declarations, [BorderStyle::Outset; 4]);
        }
    }

    if let Some(frame) = html_attribute(dom, table, "frame").and_then(LegacyTableFrame::parse) {
        push_border_styles(hints.declarations_for_mut(table), frame.styles());
    }

    if border.is_some_and(|(_, equivalent_nonzero)| equivalent_nonzero) {
        for cell in corresponding_table_cells(dom, table) {
            let declarations = hints.declarations_for_mut(cell);
            push_border_widths(declarations, BorderWidth::Length(Length::px(1.0)));
            push_border_styles(declarations, [BorderStyle::Inset; 4]);
        }
    }

    if let Some(rules) = rules {
        collect_table_rule_hints(dom, table, hints, rules);
    }

    if let Some(raw) = html_attribute(dom, table, "cellspacing") {
        let declarations = hints.declarations_for_mut(table);
        if let Some(px) = parse_non_negative_integer_px(raw) {
            declarations.push(Declaration {
                property: PropertyId::BorderSpacing,
                value: DeclaredValue::Value(PropertyValue::TableBorderSpacing(
                    TableBorderSpacing {
                        horizontal: Length::px(px),
                        vertical: Length::px(px),
                    },
                )),
                important: false,
            });
        } else {
            declarations.invalid_non_negative_integer("cellspacing", raw);
        }
    }

    let Some(raw) = html_attribute(dom, table, "cellpadding") else {
        return;
    };
    let Some(px) = parse_non_negative_integer_px(raw) else {
        hints
            .declarations_for_mut(table)
            .invalid_non_negative_integer("cellpadding", raw);
        return;
    };
    let padding = Padding(LengthPercentage::Length(Length::px(px)));
    collect_table_cells(dom, table, hints, padding);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyTableRules {
    None,
    Groups,
    Rows,
    Cols,
    All,
}

impl LegacyTableRules {
    fn parse(input: &str) -> Option<Self> {
        match input.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "groups" => Some(Self::Groups),
            "rows" => Some(Self::Rows),
            "cols" => Some(Self::Cols),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyTableFrame {
    Void,
    Above,
    Below,
    HorizontalSides,
    LeftHandSide,
    RightHandSide,
    VerticalSides,
    Box,
}

impl LegacyTableFrame {
    fn parse(input: &str) -> Option<Self> {
        match input.to_ascii_lowercase().as_str() {
            "void" => Some(Self::Void),
            "above" => Some(Self::Above),
            "below" => Some(Self::Below),
            "hsides" => Some(Self::HorizontalSides),
            "lhs" => Some(Self::LeftHandSide),
            "rhs" => Some(Self::RightHandSide),
            "vsides" => Some(Self::VerticalSides),
            "box" | "border" => Some(Self::Box),
            _ => None,
        }
    }

    /// Physical order: top, right, bottom, left.
    fn styles(self) -> [BorderStyle; 4] {
        use BorderStyle::{Hidden, Outset};
        match self {
            Self::Void => [Hidden; 4],
            Self::Above => [Outset, Hidden, Hidden, Hidden],
            Self::Below => [Hidden, Hidden, Outset, Hidden],
            Self::HorizontalSides => [Outset, Hidden, Outset, Hidden],
            Self::LeftHandSide => [Hidden, Hidden, Hidden, Outset],
            Self::RightHandSide => [Hidden, Outset, Hidden, Hidden],
            Self::VerticalSides => [Hidden, Outset, Hidden, Outset],
            Self::Box => [Outset; 4],
        }
    }
}

fn collect_table_rule_hints<D>(
    dom: &D,
    table: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
    rules: LegacyTableRules,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    for cell in corresponding_table_cells(dom, table) {
        let declarations = hints.declarations_for_mut(cell);
        push_border_widths(declarations, BorderWidth::Length(Length::px(1.0)));
        match rules {
            LegacyTableRules::None | LegacyTableRules::Groups | LegacyTableRules::Rows => {
                push_border_styles(declarations, [BorderStyle::None; 4]);
            },
            LegacyTableRules::Cols => {
                push_logical_border_styles(declarations, BorderStyle::None, BorderStyle::Solid);
            },
            LegacyTableRules::All => {
                push_border_styles(declarations, [BorderStyle::Solid; 4]);
            },
        }
    }

    match rules {
        LegacyTableRules::Groups => {
            for colgroup in direct_table_children(dom, table, &["colgroup"]) {
                let declarations = hints.declarations_for_mut(colgroup);
                push_logical_inline_border_widths(
                    declarations,
                    BorderWidth::Length(Length::px(1.0)),
                );
                push_logical_inline_border_styles(declarations, BorderStyle::Solid);
            }
            for group in direct_table_children(dom, table, &["thead", "tbody", "tfoot"]) {
                let declarations = hints.declarations_for_mut(group);
                push_logical_block_border_widths(
                    declarations,
                    BorderWidth::Length(Length::px(1.0)),
                );
                push_logical_block_border_styles(declarations, BorderStyle::Solid);
            }
        },
        LegacyTableRules::Rows => {
            for row in corresponding_table_rows(dom, table) {
                let declarations = hints.declarations_for_mut(row);
                push_logical_block_border_widths(
                    declarations,
                    BorderWidth::Length(Length::px(1.0)),
                );
                push_logical_block_border_styles(declarations, BorderStyle::Solid);
            }
        },
        LegacyTableRules::None | LegacyTableRules::Cols | LegacyTableRules::All => {},
    }
}

fn direct_table_children<D>(
    dom: &D,
    table: D::NodeId,
    local_names: &[&str],
) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    dom.dom_children(table)
        .filter(|child| {
            local_names
                .iter()
                .any(|local| is_html_element(dom, *child, local))
        })
        .collect()
}

fn corresponding_table_rows<D>(dom: &D, table: D::NodeId) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    let mut rows = direct_table_children(dom, table, &["tr"]);
    for group in direct_table_children(dom, table, &["thead", "tbody", "tfoot"]) {
        rows.extend(
            dom.dom_children(group)
                .filter(|row| is_html_element(dom, *row, "tr")),
        );
    }
    rows
}

fn corresponding_table_cells<D>(dom: &D, table: D::NodeId) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    corresponding_table_rows(dom, table)
        .into_iter()
        .flat_map(|row| {
            dom.dom_children(row)
                .filter(|cell| {
                    is_html_element(dom, *cell, "td") || is_html_element(dom, *cell, "th")
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn push_value(
    declarations: &mut PresentationalDeclarations,
    property: PropertyId,
    value: PropertyValue,
) {
    declarations.push(Declaration {
        property,
        value: DeclaredValue::Value(value),
        important: false,
    });
}

fn push_border_colors(declarations: &mut PresentationalDeclarations, color: ComputedColor) {
    for property in [
        PropertyId::BorderTopColor,
        PropertyId::BorderRightColor,
        PropertyId::BorderBottomColor,
        PropertyId::BorderLeftColor,
    ] {
        push_value(declarations, property, PropertyValue::Color(color.clone()));
    }
}

fn push_border_widths(declarations: &mut PresentationalDeclarations, width: BorderWidth) {
    for property in [
        PropertyId::BorderTopWidth,
        PropertyId::BorderRightWidth,
        PropertyId::BorderBottomWidth,
        PropertyId::BorderLeftWidth,
    ] {
        push_value(declarations, property, PropertyValue::BorderWidth(width));
    }
}

fn push_border_styles(
    declarations: &mut PresentationalDeclarations,
    [top, right, bottom, left]: [BorderStyle; 4],
) {
    for (property, style) in [
        (PropertyId::BorderTopStyle, top),
        (PropertyId::BorderRightStyle, right),
        (PropertyId::BorderBottomStyle, bottom),
        (PropertyId::BorderLeftStyle, left),
    ] {
        push_value(declarations, property, PropertyValue::BorderStyle(style));
    }
}

fn push_logical_border_styles(
    declarations: &mut PresentationalDeclarations,
    block: BorderStyle,
    inline: BorderStyle,
) {
    push_logical_block_border_styles(declarations, block);
    push_logical_inline_border_styles(declarations, inline);
}

fn push_logical_block_border_styles(
    declarations: &mut PresentationalDeclarations,
    style: BorderStyle,
) {
    for property in [
        PropertyId::BorderBlockStartStyle,
        PropertyId::BorderBlockEndStyle,
    ] {
        push_value(declarations, property, PropertyValue::BorderStyle(style));
    }
}

fn push_logical_inline_border_styles(
    declarations: &mut PresentationalDeclarations,
    style: BorderStyle,
) {
    for property in [
        PropertyId::BorderInlineStartStyle,
        PropertyId::BorderInlineEndStyle,
    ] {
        push_value(declarations, property, PropertyValue::BorderStyle(style));
    }
}

fn push_logical_block_border_widths(
    declarations: &mut PresentationalDeclarations,
    width: BorderWidth,
) {
    for property in [
        PropertyId::BorderBlockStartWidth,
        PropertyId::BorderBlockEndWidth,
    ] {
        push_value(declarations, property, PropertyValue::BorderWidth(width));
    }
}

fn push_logical_inline_border_widths(
    declarations: &mut PresentationalDeclarations,
    width: BorderWidth,
) {
    for property in [
        PropertyId::BorderInlineStartWidth,
        PropertyId::BorderInlineEndWidth,
    ] {
        push_value(declarations, property, PropertyValue::BorderWidth(width));
    }
}

fn collect_table_cells<D>(
    dom: &D,
    ancestor: D::NodeId,
    hints: &mut PresentationalHints<D::NodeId>,
    padding: Padding,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    for child in dom.dom_children(ancestor) {
        if is_html_element(dom, child, "table") {
            continue;
        }
        if is_html_element(dom, child, "td") || is_html_element(dom, child, "th") {
            let declarations = hints.declarations_for_mut(child);
            for property in [
                PropertyId::PaddingTop,
                PropertyId::PaddingRight,
                PropertyId::PaddingBottom,
                PropertyId::PaddingLeft,
            ] {
                declarations.push(Declaration {
                    property,
                    value: DeclaredValue::Value(PropertyValue::Padding(padding)),
                    important: false,
                });
            }
        }
        collect_table_cells(dom, child, hints, padding);
    }
}

fn is_html_element<D>(dom: &D, id: D::NodeId, local: &str) -> bool
where
    D: LayoutDom,
{
    dom.kind(id) == NodeKind::Element
        && dom.element_name(id).is_some_and(|name| {
            name.ns.as_ref() == "http://www.w3.org/1999/xhtml"
                && name.local.as_ref().eq_ignore_ascii_case(local)
        })
}

fn html_attribute<'dom, D>(dom: &'dom D, id: D::NodeId, local: &str) -> Option<&'dom str>
where
    D: LayoutDom,
{
    dom.attribute(id, &Namespace::from(""), &LocalName::from(local))
}

/// HTML's bounded non-negative integer parser for PH1. This intentionally
/// does not call Livery's CSS value parser: HTML accepts an optional plus sign
/// and consumes the leading digit sequence even when legacy trailing text
/// remains.
fn parse_non_negative_integer_px(input: &str) -> Option<f32> {
    let bytes = input.as_bytes();
    let mut position = 0;
    while bytes
        .get(position)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        position += 1;
    }
    let negative = matches!(bytes.get(position), Some(b'-'));
    if negative || matches!(bytes.get(position), Some(b'+')) {
        position += 1;
    }
    let start = position;
    while bytes
        .get(position)
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        position += 1;
    }
    (position > start)
        .then(|| input[start..position].parse::<u32>().ok())
        .flatten()
        .filter(|value| !negative || *value == 0)
        .map(|value| value as f32)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HtmlDimension {
    Length(f32),
    Percentage(f32),
}

impl HtmlDimension {
    fn is_zero(self) -> bool {
        match self {
            Self::Length(value) | Self::Percentage(value) => value == 0.0,
        }
    }

    fn into_css_value(self) -> LengthPercentage {
        match self {
            Self::Length(value) => LengthPercentage::Length(Length::px(value)),
            Self::Percentage(value) => LengthPercentage::Percentage(value / 100.0),
        }
    }
}

/// HTML's legacy dimension parser. Unlike CSS parsing, it requires an ASCII
/// digit after leading whitespace, consumes a decimal prefix, and treats the
/// value as a percentage only when `%` is the next unconsumed character.
fn parse_dimension(input: &str) -> Option<HtmlDimension> {
    let bytes = input.as_bytes();
    let mut position = 0;
    while bytes
        .get(position)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        position += 1;
    }
    let start = position;
    while bytes
        .get(position)
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        position += 1;
    }
    if position == start {
        return None;
    }

    let mut number_end = position;
    if matches!(bytes.get(position), Some(b'.')) {
        position += 1;
        if bytes
            .get(position)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            while bytes
                .get(position)
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                position += 1;
            }
            number_end = position;
        }
    }

    let value = input[start..number_end].parse::<f32>().ok()?;
    if !value.is_finite() {
        return None;
    }
    if matches!(bytes.get(position), Some(b'%')) {
        Some(HtmlDimension::Percentage(value))
    } else {
        Some(HtmlDimension::Length(value))
    }
}

#[cfg(test)]
mod tests {
    use genet_scripted_dom::ScriptedDom;
    use genet_static_dom::StaticDocument;
    use layout_dom_api::LayoutDomMut;

    use super::*;
    use crate::{Device, IncrementalStyle, InteractionStates, StyleSet, resolve_styles};

    fn node_by_id<D>(dom: &D, node: D::NodeId, id: &str) -> Option<D::NodeId>
    where
        D: LayoutDom,
        D::NodeId: Copy,
    {
        if dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(id) {
            return Some(node);
        }
        dom.dom_children(node)
            .find_map(|child| node_by_id(dom, child, id))
    }

    #[test]
    fn table_spacing_and_cell_padding_project_at_the_hint_origin() {
        let dom = StaticDocument::parse(
            r#"
                <table id="outer" cellspacing="+7legacy" cellpadding="5px">
                  <tr>
                    <td id="outer-cell"><div id="not-a-cell">outer</div></td>
                    <td><table id="inner" cellspacing="3" cellpadding="2"><tr><th id="inner-cell">inner</th></tr></table></td>
                  </tr>
                </table>
            "#,
        );
        let outer = node_by_id(&dom, dom.document(), "outer").unwrap();
        let outer_cell = node_by_id(&dom, dom.document(), "outer-cell").unwrap();
        let inner = node_by_id(&dom, dom.document(), "inner").unwrap();
        let inner_cell = node_by_id(&dom, dom.document(), "inner-cell").unwrap();
        let not_a_cell = node_by_id(&dom, dom.document(), "not-a-cell").unwrap();
        let style_set = StyleSet::parse("", &["#outer-cell { padding-left: 13px; }"]);
        let styles = resolve_styles(
            &dom,
            &style_set,
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(
            styles.get(outer).unwrap().border_spacing,
            TableBorderSpacing {
                horizontal: Length::px(7.0),
                vertical: Length::px(7.0),
            }
        );
        assert_eq!(
            styles.get(inner).unwrap().border_spacing,
            TableBorderSpacing {
                horizontal: Length::px(3.0),
                vertical: Length::px(3.0),
            }
        );
        let outer_cell_style = styles.get(outer_cell).unwrap();
        assert_eq!(
            outer_cell_style.padding_top,
            Padding(LengthPercentage::Length(Length::px(5.0)))
        );
        assert_eq!(
            outer_cell_style.padding_left,
            Padding(LengthPercentage::Length(Length::px(13.0))),
            "normal author CSS must override the hint"
        );
        let inner_cell_style = styles.get(inner_cell).unwrap();
        assert_eq!(
            inner_cell_style.padding_top,
            Padding(LengthPercentage::Length(Length::px(2.0))),
            "a nested table owns its own cells"
        );
        assert_eq!(styles.get(not_a_cell).unwrap().padding_top, Padding::ZERO);
    }

    #[test]
    fn invalid_html_values_are_diagnosed_without_css_parsing() {
        let dom = StaticDocument::parse(
            r#"<table id="table" cellspacing="-3" cellpadding="px5"><tr><td>cell</td></tr></table>"#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        assert_eq!(
            styles.presentational_hint_diagnostics(table),
            [
                PresentationalHintDiagnostic::InvalidNonNegativeInteger {
                    attribute: "cellspacing",
                    value: "-3".to_owned(),
                },
                PresentationalHintDiagnostic::InvalidNonNegativeInteger {
                    attribute: "cellpadding",
                    value: "px5".to_owned(),
                },
            ]
        );
        assert_eq!(
            styles.get(table).unwrap().border_spacing,
            TableBorderSpacing::ZERO
        );
    }

    #[test]
    fn table_alignment_uses_float_or_logical_centering_margins() {
        let dom = StaticDocument::parse(
            r#"
                <table id="center" align="center"></table>
                <table id="left" align="left"></table>
                <table id="right" align="RIGHT"></table>
            "#,
        );
        let center = node_by_id(&dom, dom.document(), "center").unwrap();
        let left = node_by_id(&dom, dom.document(), "left").unwrap();
        let right = node_by_id(&dom, dom.document(), "right").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &["#center { margin-left: 7px; }"]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        let centered = styles.get(center).unwrap();
        assert_eq!(
            centered.margin_left,
            "7px".parse().unwrap(),
            "ordinary author CSS must override the mapped start hint"
        );
        assert_eq!(centered.margin_right, Margin::Auto);
        assert_eq!(centered.margin_inline_start, Margin::Auto);
        assert_eq!(centered.margin_inline_end, Margin::Auto);
        assert_eq!(
            styles
                .computed_style(center, "margin-inline-start")
                .as_deref(),
            Some("7px"),
            "CSSOM must project the logical name through the winning physical side"
        );
        assert_eq!(
            styles
                .computed_style(center, "margin-inline-end")
                .as_deref(),
            Some("auto")
        );
        assert_eq!(styles.get(left).unwrap().float, Float::Left);
        assert_eq!(styles.get(right).unwrap().float, Float::Right);
    }

    #[test]
    fn table_dimensions_follow_the_element_specific_zero_rules() {
        let dom = StaticDocument::parse(
            r#"
                <table id="table" width="0" height="0">
                  <colgroup id="colgroup" width="90"><col id="col" width="0"></colgroup>
                  <tbody id="body" height="25.5%">
                    <tr id="row" height="7legacy">
                      <td id="cell" width="0" height="0">cell</td>
                    </tr>
                  </tbody>
                </table>
            "#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let colgroup = node_by_id(&dom, dom.document(), "colgroup").unwrap();
        let col = node_by_id(&dom, dom.document(), "col").unwrap();
        let body = node_by_id(&dom, dom.document(), "body").unwrap();
        let row = node_by_id(&dom, dom.document(), "row").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(styles.get(table).unwrap().width, Size::Auto);
        assert_eq!(
            styles.get(table).unwrap().height,
            Size::Value(LengthPercentage::Length(Length::px(0.0)))
        );
        assert_eq!(
            styles.presentational_hint_diagnostics(table),
            [PresentationalHintDiagnostic::InvalidDimension {
                attribute: "width",
                value: "0".to_owned(),
                nonzero: true,
            }]
        );
        assert_eq!(
            styles.get(col).unwrap().width,
            Size::Value(LengthPercentage::Length(Length::px(0.0)))
        );
        assert_eq!(
            styles.get(colgroup).unwrap().width,
            Size::Auto,
            "HTML maps col[width], not colgroup[width]"
        );
        assert_eq!(
            styles.get(body).unwrap().height,
            Size::Value(LengthPercentage::Percentage(0.255))
        );
        assert_eq!(
            styles.get(row).unwrap().height,
            Size::Value(LengthPercentage::Length(Length::px(7.0)))
        );
        assert_eq!(styles.get(cell).unwrap().width, Size::Auto);
        assert_eq!(styles.get(cell).unwrap().height, Size::Auto);
        assert_eq!(
            styles.presentational_hint_diagnostics(cell),
            [
                PresentationalHintDiagnostic::InvalidDimension {
                    attribute: "width",
                    value: "0".to_owned(),
                    nonzero: true,
                },
                PresentationalHintDiagnostic::InvalidDimension {
                    attribute: "height",
                    value: "0".to_owned(),
                    nonzero: true,
                },
            ]
        );
    }

    #[test]
    fn table_dimensions_preserve_percentages_and_author_css_wins() {
        let dom = StaticDocument::parse(
            r#"
                <table id="outer" width="240">
                  <tr><td>
                    <table id="inner" width="40%"><tr><td id="cell" width="35.5%">cell</td></tr></table>
                  </td></tr>
                </table>
            "#,
        );
        let outer = node_by_id(&dom, dom.document(), "outer").unwrap();
        let inner = node_by_id(&dom, dom.document(), "inner").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &["#outer { width: 260px; }"]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(
            styles.get(outer).unwrap().width,
            Size::Value(LengthPercentage::Length(Length::px(260.0))),
            "ordinary author CSS must override the HTML dimension hint"
        );
        assert_eq!(
            styles.get(inner).unwrap().width,
            Size::Value(LengthPercentage::Percentage(0.4)),
            "the nested table keeps its own percentage hint"
        );
        assert_eq!(
            styles.get(cell).unwrap().width,
            Size::Value(LengthPercentage::Percentage(0.355))
        );
    }

    #[test]
    fn table_part_alignment_projects_text_align_without_changing_table_align() {
        let dom = StaticDocument::parse(
            r#"
                <table id="table" align="left">
                  <caption id="caption" align="BOTTOM">caption</caption>
                  <thead id="head" align="middle"><tr><th id="absolute" align="absmiddle"><span id="child">head</span></th></tr></thead>
                  <tbody><tr id="row" align="justify"><td id="left" align="left">left</td><td id="spaced" align=" right ">spaced</td><td id="right" align="right">right</td></tr></tbody>
                </table>
            "#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let caption = node_by_id(&dom, dom.document(), "caption").unwrap();
        let head = node_by_id(&dom, dom.document(), "head").unwrap();
        let absolute = node_by_id(&dom, dom.document(), "absolute").unwrap();
        let child = node_by_id(&dom, dom.document(), "child").unwrap();
        let row = node_by_id(&dom, dom.document(), "row").unwrap();
        let left = node_by_id(&dom, dom.document(), "left").unwrap();
        let spaced = node_by_id(&dom, dom.document(), "spaced").unwrap();
        let right = node_by_id(&dom, dom.document(), "right").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &["#right { text-align: center; }"]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(styles.get(table).unwrap().float, Float::Left);
        assert_eq!(styles.get(table).unwrap().text_align, TextAlign::Start);
        assert_eq!(
            styles.get(caption).unwrap().caption_side,
            CaptionSide::Bottom
        );
        assert_eq!(styles.get(head).unwrap().text_align, TextAlign::Center);
        assert_eq!(styles.get(absolute).unwrap().text_align, TextAlign::Center);
        assert_eq!(styles.get(child).unwrap().text_align, TextAlign::Center);
        assert_eq!(styles.get(row).unwrap().text_align, TextAlign::Justify);
        assert_eq!(styles.get(left).unwrap().text_align, TextAlign::Left);
        assert_eq!(
            styles.get(spaced).unwrap().text_align,
            TextAlign::Justify,
            "enumerated alignment values are ASCII-insensitive but not whitespace-trimmed"
        );
        assert_eq!(
            styles.get(right).unwrap().text_align,
            TextAlign::Center,
            "ordinary author CSS must override the table-part hint"
        );
    }

    #[test]
    fn deepest_applicable_alignment_owner_selects_descendant_used_margin_policy() {
        let dom = StaticDocument::parse(
            r#"
                <table><tbody><tr align="right"><td>
                  <div id="outer-child">outer</div>
                  <div id="center-owner" align="center"><div id="center-child">center</div></div>
                  <p id="suppressor" align="left"><span id="suppressed-child">suppressed</span></p>
                  <div id="invalid-owner" align=" right "><div id="invalid-child">invalid</div></div>
                </td><th align="absmiddle"><div id="absolute-child">absolute</div></th></tr></tbody></table>
            "#,
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let alignment = |id| {
            let node = node_by_id(&dom, dom.document(), id).unwrap();
            styles.legacy_descendant_alignment(node)
        };

        assert_eq!(
            alignment("outer-child"),
            Some(LegacyDescendantAlignment::LineRight)
        );
        assert_eq!(alignment("center-owner"), None);
        assert_eq!(
            alignment("center-child"),
            Some(LegacyDescendantAlignment::Center)
        );
        assert_eq!(alignment("suppressor"), None);
        assert_eq!(alignment("suppressed-child"), None);
        assert_eq!(
            alignment("invalid-owner"),
            Some(LegacyDescendantAlignment::LineRight),
            "an invalid value neither owns alignment nor suppresses the ancestor"
        );
        assert_eq!(
            alignment("invalid-child"),
            Some(LegacyDescendantAlignment::LineRight)
        );
        assert_eq!(alignment("absolute-child"), None);
    }

    #[test]
    fn center_div_and_paragraph_alignment_project_text_align() {
        let dom = StaticDocument::parse(
            r#"
                <center id="center">center</center>
                <div id="div" align="justify">div</div>
                <p id="paragraph" align="RIGHT">paragraph</p>
            "#,
        );
        let center = node_by_id(&dom, dom.document(), "center").unwrap();
        let div = node_by_id(&dom, dom.document(), "div").unwrap();
        let paragraph = node_by_id(&dom, dom.document(), "paragraph").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(styles.get(center).unwrap().text_align, TextAlign::Center);
        assert_eq!(styles.get(div).unwrap().text_align, TextAlign::Justify);
        assert_eq!(styles.get(paragraph).unwrap().text_align, TextAlign::Right);
    }

    #[test]
    fn caller_provider_is_a_real_cascade_input_not_a_stylesheet() {
        let dom = StaticDocument::parse(r#"<div id="target">target</div>"#);
        let target = node_by_id(&dom, dom.document(), "target").unwrap();
        let mut hints = PresentationalHints::default();
        hints.declarations_for_mut(target).push(Declaration {
            property: PropertyId::PaddingTop,
            value: DeclaredValue::Value(PropertyValue::Padding(Padding(LengthPercentage::Length(
                Length::px(4.0),
            )))),
            important: false,
        });
        let style_set = StyleSet::parse("", &["#target { padding-top: 9px; }"]);
        let styles = crate::resolve_styles_with_presentational_hints(
            &dom,
            &style_set,
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
            &hints,
        );
        assert_eq!(style_set.author_sheets().len(), 1);
        assert_eq!(
            styles.get(target).unwrap().padding_top,
            Padding(LengthPercentage::Length(Length::px(9.0))),
            "ordinary author CSS must override a provider declaration"
        );
    }

    #[test]
    fn cellpadding_attribute_mutation_restyles_the_table_subtree() {
        let mut dom = ScriptedDom::from_serialized_document(
            r#"<table id="table" cellpadding="4"><tr><td id="cell">cell</td></tr></table>"#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let style_set = StyleSet::parse("", &[]);
        let device = Device::screen(800.0, 600.0);
        let states = InteractionStates::default();
        let mut session = IncrementalStyle::new();
        session.update(&dom, &style_set, &device, &states, &[]);
        assert_eq!(
            session.styles().get(cell).unwrap().padding_top,
            Padding(LengthPercentage::Length(Length::px(4.0)))
        );

        dom.set_attribute(
            table,
            layout_dom_api::QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("cellpadding"),
            ),
            "9",
        );
        let mut mutations = Vec::new();
        dom.drain_mutations(&mut mutations);
        let stats = session.update(&dom, &style_set, &device, &states, &mutations);
        assert!(!stats.full_document);
        assert_eq!(
            session.styles().get(cell).unwrap().padding_top,
            Padding(LengthPercentage::Length(Length::px(9.0)))
        );
    }

    #[test]
    fn dimension_and_alignment_attribute_mutations_restyle_computed_css() {
        let mut dom = ScriptedDom::from_serialized_document(
            r#"<table id="table" width="80"><tr><td id="cell" align="right"><div id="descendant">cell</div></td></tr></table>"#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let descendant = node_by_id(&dom, dom.document(), "descendant").unwrap();
        let style_set = StyleSet::parse("", &[]);
        let device = Device::screen(800.0, 600.0);
        let states = InteractionStates::default();
        let mut session = IncrementalStyle::new();
        session.update(&dom, &style_set, &device, &states, &[]);
        assert_eq!(
            session.styles().get(table).unwrap().width,
            Size::Value(LengthPercentage::Length(Length::px(80.0)))
        );
        assert_eq!(
            session.styles().get(cell).unwrap().text_align,
            TextAlign::Right
        );
        assert_eq!(
            session.styles().legacy_descendant_alignment(descendant),
            Some(LegacyDescendantAlignment::LineRight)
        );

        for (node, attribute, value) in [(table, "width", "50%"), (cell, "align", "justify")] {
            dom.set_attribute(
                node,
                layout_dom_api::QualName::new(
                    None,
                    Namespace::from(""),
                    LocalName::from(attribute),
                ),
                value,
            );
        }
        let mut mutations = Vec::new();
        dom.drain_mutations(&mut mutations);
        let stats = session.update(&dom, &style_set, &device, &states, &mutations);
        assert!(!stats.full_document);
        assert_eq!(
            session.styles().get(table).unwrap().width,
            Size::Value(LengthPercentage::Percentage(0.5))
        );
        assert_eq!(
            session.styles().get(cell).unwrap().text_align,
            TextAlign::Justify
        );
        assert_eq!(
            session.styles().legacy_descendant_alignment(descendant),
            Some(LegacyDescendantAlignment::LineLeft)
        );

        dom.set_attribute(
            cell,
            layout_dom_api::QualName::new(None, Namespace::from(""), LocalName::from("align")),
            "invalid",
        );
        mutations.clear();
        dom.drain_mutations(&mut mutations);
        session.update(&dom, &style_set, &device, &states, &mutations);
        assert_eq!(
            session.styles().legacy_descendant_alignment(descendant),
            None,
            "incremental recascade must remove stale used-value metadata"
        );
    }

    #[test]
    fn table_color_border_frame_and_rules_project_typed_css() {
        let dom = StaticDocument::parse(
            r##"
                <table id="table" bgcolor="chucknorris" bordercolor="#0f8"
                       border="3" frame="hsides" rules="cols">
                  <colgroup id="columns"><col></colgroup>
                  <tbody id="group"><tr id="row"><td id="cell" bgcolor="blue">cell</td></tr></tbody>
                </table>
            "##,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let group = node_by_id(&dom, dom.document(), "group").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let table_style = styles.get(table).unwrap();
        let group_style = styles.get(group).unwrap();
        let cell_style = styles.get(cell).unwrap();

        assert_eq!(
            table_style.background_color.to_srgb8(),
            Some((192, 0, 0, 255))
        );
        assert_eq!(
            table_style.border_top_color.to_srgb8(),
            Some((0, 255, 136, 255))
        );
        assert_eq!(
            group_style.border_top_color.to_srgb8(),
            Some((0, 255, 136, 255))
        );
        assert_eq!(
            cell_style.background_color.to_srgb8(),
            Some((0, 0, 255, 255))
        );
        assert_eq!(cell_style.border_top_color.to_srgb8(), Some((0, 0, 0, 255)));
        assert_eq!(table_style.border_collapse, BorderCollapse::Collapse);
        assert_eq!(
            table_style.border_top_width,
            BorderWidth::Length(Length::px(3.0))
        );
        assert_eq!(
            table_style.border_right_width,
            BorderWidth::Length(Length::px(3.0))
        );
        assert_eq!(table_style.border_top_style, BorderStyle::Outset);
        assert_eq!(table_style.border_right_style, BorderStyle::Hidden);
        assert_eq!(table_style.border_bottom_style, BorderStyle::Outset);
        assert_eq!(table_style.border_left_style, BorderStyle::Hidden);
        assert_eq!(
            cell_style.border_top_width,
            BorderWidth::Length(Length::px(1.0))
        );
        assert_eq!(cell_style.border_top_style, BorderStyle::None);
        assert_eq!(cell_style.border_right_style, BorderStyle::Solid);
        assert_eq!(cell_style.border_bottom_style, BorderStyle::None);
        assert_eq!(cell_style.border_left_style, BorderStyle::Solid);
    }

    #[test]
    fn border_attribute_distinguishes_zero_prefixes_from_errors() {
        let dom = StaticDocument::parse(
            r#"
                <table id="empty" border><tr><td id="empty-cell">empty</td></tr></table>
                <table id="prefix" border="1foo"><tr><td id="prefix-cell">prefix</td></tr></table>
                <table id="zero" border="-0foo"><tr><td id="zero-cell">zero</td></tr></table>
            "#,
        );
        let by_id = |id| node_by_id(&dom, dom.document(), id).unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        for id in ["empty", "prefix"] {
            let style = styles.get(by_id(id)).unwrap();
            assert_eq!(style.border_top_width, BorderWidth::Length(Length::px(1.0)));
            assert_eq!(style.border_top_style, BorderStyle::Outset);
        }
        for id in ["empty-cell", "prefix-cell"] {
            let style = styles.get(by_id(id)).unwrap();
            assert_eq!(style.border_top_width, BorderWidth::Length(Length::px(1.0)));
            assert_eq!(style.border_top_style, BorderStyle::Inset);
        }
        let zero = styles.get(by_id("zero")).unwrap();
        assert_eq!(zero.border_top_width, BorderWidth::Length(Length::px(0.0)));
        assert_eq!(zero.border_top_style, BorderStyle::None);
        assert_eq!(
            styles.get(by_id("zero-cell")).unwrap().border_top_width,
            BorderWidth::Medium
        );
        assert_eq!(
            styles.presentational_hint_diagnostics(by_id("empty")),
            [PresentationalHintDiagnostic::InvalidNonNegativeInteger {
                attribute: "border",
                value: String::new(),
            }]
        );
        assert!(
            styles
                .presentational_hint_diagnostics(by_id("prefix"))
                .is_empty()
        );
        assert!(
            styles
                .presentational_hint_diagnostics(by_id("zero"))
                .is_empty()
        );
    }

    #[test]
    fn failed_legacy_colors_are_diagnosed_without_declarations() {
        let dom = StaticDocument::parse(
            r#"<table id="table" bgcolor="transparent" bordercolor=""><tr><td>cell</td></tr></table>"#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::parse("", &[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(
            styles.presentational_hint_diagnostics(table),
            [
                PresentationalHintDiagnostic::InvalidLegacyColor {
                    attribute: "bgcolor",
                    value: "transparent".to_owned(),
                },
                PresentationalHintDiagnostic::InvalidLegacyColor {
                    attribute: "bordercolor",
                    value: String::new(),
                },
            ]
        );
        assert_eq!(
            styles.get(table).unwrap().background_color.to_srgb8(),
            Some((0, 0, 0, 0))
        );
    }

    #[test]
    fn logical_table_rules_follow_the_cells_writing_mode() {
        let dom = StaticDocument::parse(
            r#"<table rules="cols"><tr><td id="cell" style="writing-mode: vertical-rl">cell</td></tr></table>"#,
        );
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let style = styles.get(cell).unwrap();

        assert_eq!(style.border_top_style, BorderStyle::Solid);
        assert_eq!(style.border_right_style, BorderStyle::None);
        assert_eq!(style.border_bottom_style, BorderStyle::Solid);
        assert_eq!(style.border_left_style, BorderStyle::None);
    }

    #[test]
    fn group_rules_target_column_and_row_groups_on_logical_axes() {
        let dom = StaticDocument::parse(
            r#"
                <table rules="groups">
                  <colgroup id="columns"><col></colgroup>
                  <tbody id="rows"><tr><td id="cell">cell</td></tr></tbody>
                </table>
            "#,
        );
        let columns = node_by_id(&dom, dom.document(), "columns").unwrap();
        let rows = node_by_id(&dom, dom.document(), "rows").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let columns = styles.get(columns).unwrap();
        let rows = styles.get(rows).unwrap();
        let cell = styles.get(cell).unwrap();

        assert_eq!(columns.border_left_style, BorderStyle::Solid);
        assert_eq!(columns.border_right_style, BorderStyle::Solid);
        assert_eq!(
            columns.border_left_width,
            BorderWidth::Length(Length::px(1.0))
        );
        assert_eq!(rows.border_top_style, BorderStyle::Solid);
        assert_eq!(rows.border_bottom_style, BorderStyle::Solid);
        assert_eq!(rows.border_top_width, BorderWidth::Length(Length::px(1.0)));
        assert_eq!(cell.border_top_style, BorderStyle::None);
        assert_eq!(cell.border_left_style, BorderStyle::None);
    }

    #[test]
    fn author_css_overrides_ph3_hints_and_ua_rule_colors() {
        let dom = StaticDocument::parse(
            r#"<table id="table" bgcolor="red" border="4" frame="box" rules="all"><tr><td id="cell">cell</td></tr></table>"#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[r#"
                    #table { background-color: white; border-top-style: dashed; }
                    #cell { border-left-style: dotted; border-left-color: red; }
                "#]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        assert_eq!(
            styles.get(table).unwrap().background_color.to_srgb8(),
            Some((255, 255, 255, 255))
        );
        assert_eq!(
            styles.get(table).unwrap().border_top_style,
            BorderStyle::Dashed
        );
        assert_eq!(
            styles.get(cell).unwrap().border_left_style,
            BorderStyle::Dotted
        );
        assert_eq!(
            styles.get(cell).unwrap().border_left_color.to_srgb8(),
            Some((255, 0, 0, 255))
        );
    }

    #[test]
    fn outer_table_rules_do_not_cross_a_nested_table_boundary() {
        let dom = StaticDocument::parse(
            r#"
                <table rules="all" border="2"><tr><td>
                  <table><tr><td id="inner">inner</td></tr></table>
                </td></tr></table>
            "#,
        );
        let inner = node_by_id(&dom, dom.document(), "inner").unwrap();
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let style = styles.get(inner).unwrap();

        assert_eq!(style.border_top_style, BorderStyle::None);
        assert_eq!(style.border_top_width, BorderWidth::Medium);
    }

    #[test]
    fn ph3_attribute_mutation_restyles_table_and_cells_without_stale_rules() {
        let mut dom = ScriptedDom::from_serialized_document(
            r#"<table id="table" bgcolor="red" frame="above" rules="cols"><tr><td id="cell">cell</td></tr></table>"#,
        );
        let table = node_by_id(&dom, dom.document(), "table").unwrap();
        let cell = node_by_id(&dom, dom.document(), "cell").unwrap();
        let style_set = StyleSet::cambium(&[]);
        let device = Device::screen(800.0, 600.0);
        let states = InteractionStates::default();
        let mut session = IncrementalStyle::new();
        session.update(&dom, &style_set, &device, &states, &[]);
        assert_eq!(
            session
                .styles()
                .get(table)
                .unwrap()
                .background_color
                .to_srgb8(),
            Some((255, 0, 0, 255))
        );
        assert_eq!(
            session.styles().get(table).unwrap().border_top_style,
            BorderStyle::Outset
        );
        assert_eq!(
            session.styles().get(cell).unwrap().border_left_style,
            BorderStyle::Solid
        );
        assert_eq!(
            session.styles().get(cell).unwrap().border_top_style,
            BorderStyle::None
        );

        for (attribute, value) in [("bgcolor", "blue"), ("frame", "rhs"), ("rules", "rows")] {
            dom.set_attribute(
                table,
                layout_dom_api::QualName::new(
                    None,
                    Namespace::from(""),
                    LocalName::from(attribute),
                ),
                value,
            );
        }
        let mut mutations = Vec::new();
        dom.drain_mutations(&mut mutations);
        let stats = session.update(&dom, &style_set, &device, &states, &mutations);

        assert!(!stats.full_document);
        assert_eq!(
            session
                .styles()
                .get(table)
                .unwrap()
                .background_color
                .to_srgb8(),
            Some((0, 0, 255, 255))
        );
        assert_eq!(
            session.styles().get(table).unwrap().border_top_style,
            BorderStyle::Hidden
        );
        assert_eq!(
            session.styles().get(table).unwrap().border_right_style,
            BorderStyle::Outset
        );
        assert_eq!(
            session.styles().get(cell).unwrap().border_left_style,
            BorderStyle::None
        );
        assert_eq!(
            session.styles().get(cell).unwrap().border_top_style,
            BorderStyle::None
        );
    }

    #[test]
    fn provider_rejects_important_declarations() {
        let mut hints = PresentationalHints::<u8>::default();
        hints.declarations_for_mut(1).push(Declaration {
            property: PropertyId::PaddingTop,
            value: DeclaredValue::Value(PropertyValue::Padding(Padding::ZERO)),
            important: true,
        });
        let declarations = hints.declarations_for(1).unwrap();
        assert!(declarations.declarations().is_empty());
        assert_eq!(
            declarations.diagnostics(),
            [PresentationalHintDiagnostic::ImportantDeclarationRejected {
                property: PropertyId::PaddingTop,
            }]
        );
    }

    #[test]
    fn html_non_negative_integer_parser_keeps_its_legacy_acceptance() {
        assert_eq!(parse_non_negative_integer_px(" \t+7px"), Some(7.0));
        assert_eq!(parse_non_negative_integer_px("-0legacy"), Some(0.0));
        assert_eq!(parse_non_negative_integer_px("-1"), None);
        assert_eq!(parse_non_negative_integer_px("px7"), None);
    }

    #[test]
    fn html_dimension_parser_keeps_legacy_prefix_and_percentage_rules() {
        assert_eq!(
            parse_dimension(" \t12.5%legacy"),
            Some(HtmlDimension::Percentage(12.5))
        );
        assert_eq!(
            parse_dimension("12.%"),
            Some(HtmlDimension::Percentage(12.0))
        );
        assert_eq!(
            parse_dimension("12garbage%"),
            Some(HtmlDimension::Length(12.0))
        );
        assert_eq!(parse_dimension("+12"), None);
        assert_eq!(parse_dimension("-0"), None);
        assert_eq!(parse_dimension(".5"), None);
    }
}
