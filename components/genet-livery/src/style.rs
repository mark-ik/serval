use std::{
    collections::HashMap,
    hash::Hash,
    ops::{Deref, DerefMut},
};

use genet_document_resources::{
    ResolvedImportRule, ResolvedStylesheet, StylesheetImportParent, StylesheetOwner,
};
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use livery::{
    ComputedValues, PropertyId, PropertyValue,
    cascade::{
        CascadeLayer, ColorComputeContext, DeclarationError, MatchedCustomDeclaration,
        MatchedDeclaration, Origin, Specificity, cascade_with_custom_context,
        parse_declaration_block,
    },
    custom::CustomProperties,
    media::Device,
    stylesheet::{
        ContainerSnapshot, Keyframes, RuleMutationError, StyleRule, Stylesheet,
        StylesheetDiagnostic,
    },
    values::{
        BackgroundImage, BorderStyle, BoxShadow, ComputedColor, FontSize, Length, LengthPercentage,
        LengthUnit, LineHeight, Margin, Padding, Size, SystemColor, TreeCounts, UsedColorContext,
    },
};

use crate::{CAMBIUM_UA_DEFAULTS, InteractionStates, SelectorTree};

/// Layout facts needed to serialize properties whose CSSOM result is a used
/// value rather than only a computed value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsedValueContext {
    pub border_box: (f32, f32),
    pub containing_inline_size: Option<f32>,
}

/// Parsed UA and author rules for one document class. The sheets are
/// retained as CSSOM-shaped objects (harvest H3); the flattened rule and
/// keyframes views are rebuilt after every mutation.
/// One retained author sheet plus the HTML ownership that introduced it.
/// CSSOM consumers can keep a stable sheet identity while the cascade only
/// needs the enclosed parsed stylesheet.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthorStylesheet {
    sheet_id: u64,
    owner: StylesheetOwner,
    owner_node: Option<u64>,
    source_url: Option<String>,
    requested_url: Option<String>,
    content_type: Option<String>,
    media: Option<String>,
    imports: Vec<ResolvedImportRule>,
    import_parent: Option<StylesheetImportParent>,
    cssom_key: String,
    authored_text: String,
    stylesheet: Stylesheet,
}

impl AuthorStylesheet {
    fn from_resolved(sheet: &ResolvedStylesheet, cssom_key: String) -> Self {
        Self {
            sheet_id: sheet.sheet_id,
            owner: sheet.owner,
            owner_node: sheet.owner_node,
            source_url: sheet.source_url.clone(),
            requested_url: sheet.requested_url.clone(),
            content_type: sheet.content_type.clone(),
            media: sheet.media.clone(),
            imports: sheet.imports.clone(),
            import_parent: sheet.import_parent,
            cssom_key,
            authored_text: sheet.text.clone(),
            stylesheet: Stylesheet::parse(&sheet.text, Origin::Author)
                .with_document_media(sheet.media.as_deref()),
        }
    }

    pub fn owner(&self) -> StylesheetOwner {
        self.owner
    }

    /// The stable document-element identity for a direct sheet. Imported
    /// sheets retain their root's value for source attribution, but are not
    /// members of `document.styleSheets`.
    pub fn owner_node(&self) -> Option<u64> {
        self.owner_node
    }

    pub fn source_url(&self) -> Option<&str> {
        self.source_url.as_deref()
    }

    pub fn media(&self) -> Option<&str> {
        self.media.as_deref()
    }

    /// Opaque CSSOM identity. Direct sheets derive it from their owner element;
    /// imported sheets derive it from their parent import path.
    pub fn cssom_key(&self) -> &str {
        &self.cssom_key
    }

    fn refresh_resource_graph(&mut self, source: &ResolvedStylesheet, cssom_key: String) {
        self.sheet_id = source.sheet_id;
        self.imports = source.imports.clone();
        self.import_parent = source.import_parent;
        self.cssom_key = cssom_key;
    }

    fn can_retain_live_cssom(&self, source: &ResolvedStylesheet) -> bool {
        self.owner != StylesheetOwner::Imported
            && source.owner != StylesheetOwner::Imported
            && self.owner == source.owner
            && self.owner_node == source.owner_node
            && self.source_url == source.source_url
            && self.requested_url == source.requested_url
            && self.content_type == source.content_type
            && self.media == source.media
            && self.authored_text == source.text
    }
}

/// CSSOM metadata for a retained `@import` rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CssomImportRule {
    pub href: String,
    pub media: Option<String>,
    pub child_sheet_key: Option<String>,
}

/// The parent import rule for an imported CSSOM stylesheet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CssomImportOwner {
    pub parent_sheet_key: String,
    pub import_index: usize,
}

impl Deref for AuthorStylesheet {
    type Target = Stylesheet;

    fn deref(&self) -> &Self::Target {
        &self.stylesheet
    }
}

impl DerefMut for AuthorStylesheet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stylesheet
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSet {
    ua: Stylesheet,
    authors: Vec<AuthorStylesheet>,
    rules: Vec<StyleRule>,
    keyframes: Vec<Keyframes>,
    diagnostics: Vec<StylesheetDiagnostic>,
    generation: u64,
}

fn cssom_keys(sheets: &[ResolvedStylesheet]) -> Vec<String> {
    fn resolve(
        index: usize,
        sheets: &[ResolvedStylesheet],
        by_id: &HashMap<u64, usize>,
        keys: &mut [Option<String>],
        active: &mut Vec<usize>,
    ) -> String {
        if let Some(key) = &keys[index] {
            return key.clone();
        }
        let sheet = &sheets[index];
        if active.contains(&index) {
            return format!("orphan-import:{}", sheet.sheet_id);
        }
        active.push(index);
        let key = match sheet.import_parent {
            None => sheet.owner_node.map_or_else(
                || format!("document:{}", sheet.document_order),
                |owner_node| format!("node:{owner_node}"),
            ),
            Some(parent) => by_id
                .get(&parent.sheet_id)
                .copied()
                .map(|parent_index| {
                    let parent_key = resolve(parent_index, sheets, by_id, keys, active);
                    format!("{parent_key}/import:{}", parent.import_index)
                })
                .unwrap_or_else(|| format!("orphan-import:{}", sheet.sheet_id)),
        };
        active.pop();
        keys[index] = Some(key.clone());
        key
    }

    let by_id = sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| (sheet.sheet_id, index))
        .collect::<HashMap<_, _>>();
    let mut keys = vec![None; sheets.len()];
    for index in 0..sheets.len() {
        let _ = resolve(index, sheets, &by_id, &mut keys, &mut Vec::new());
    }
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| {
            key.unwrap_or_else(|| format!("orphan-import:{}", sheets[index].sheet_id))
        })
        .collect()
}

impl StyleSet {
    pub fn cambium(author_sheets: &[&str]) -> Self {
        Self::parse(CAMBIUM_UA_DEFAULTS, author_sheets)
    }

    pub fn parse(ua_sheet: &str, author_sheets: &[&str]) -> Self {
        let author_sheets = author_sheets
            .iter()
            .enumerate()
            .map(|(document_order, text)| ResolvedStylesheet {
                sheet_id: document_order as u64,
                owner: StylesheetOwner::Inline,
                owner_node: None,
                source_url: None,
                requested_url: None,
                content_type: None,
                media: None,
                imports: Vec::new(),
                import_parent: None,
                text: (*text).to_owned(),
                document_order: document_order as u64,
            })
            .collect::<Vec<_>>();
        Self::parse_resolved(ua_sheet, &author_sheets)
    }

    /// Build an author cascade from the host's ordered document resource set.
    /// Link `media` remains separate metadata until Livery evaluates it.
    pub fn cambium_resources(author_sheets: &[ResolvedStylesheet]) -> Self {
        Self::parse_resolved(CAMBIUM_UA_DEFAULTS, author_sheets)
    }

    pub fn parse_resolved(ua_sheet: &str, author_sheets: &[ResolvedStylesheet]) -> Self {
        let mut result = Self {
            ua: Stylesheet::parse(ua_sheet, Origin::UserAgent),
            ..Self::default()
        };
        let cssom_keys = cssom_keys(author_sheets);
        for (source, cssom_key) in author_sheets.iter().zip(cssom_keys) {
            result
                .authors
                .push(AuthorStylesheet::from_resolved(source, cssom_key));
        }
        result.rebuild();
        result
    }

    /// Reconcile a live document's freshly resolved resource set. Direct
    /// `<style>` and `<link>` sheets with unchanged owner, identity, media,
    /// response metadata, and source text retain their parsed CSSOM object, so
    /// prior `insertRule` / `deleteRule` mutations survive an unrelated sheet
    /// insertion, removal, or reorder. Imported sheets always derive afresh
    /// from their current parent response.
    pub fn replace_author_sheets(&mut self, author_sheets: &[ResolvedStylesheet]) {
        let cssom_keys = cssom_keys(author_sheets);
        let mut previous = self
            .authors
            .drain(..)
            .map(Some)
            .collect::<Vec<Option<AuthorStylesheet>>>();
        let mut authors = Vec::with_capacity(author_sheets.len());
        for (source, cssom_key) in author_sheets.iter().zip(cssom_keys) {
            let retained = previous
                .iter()
                .position(|candidate| {
                    candidate
                        .as_ref()
                        .is_some_and(|candidate| candidate.can_retain_live_cssom(source))
                })
                .and_then(|index| previous[index].take());
            let author = match retained {
                Some(mut retained) => {
                    retained.refresh_resource_graph(source, cssom_key);
                    retained
                },
                None => AuthorStylesheet::from_resolved(source, cssom_key),
            };
            authors.push(author);
        }
        self.authors = authors;
        self.rebuild();
    }

    /// Rebuild the flattened cascade views from the retained sheets, with
    /// author source order running across sheets in document order.
    fn rebuild(&mut self) {
        self.rules.clear();
        self.keyframes.clear();
        self.diagnostics.clear();
        self.diagnostics.extend_from_slice(self.ua.diagnostics());
        self.rules.extend(self.ua.reindexed_rules(0));
        self.keyframes.extend(self.ua.keyframes().iter().cloned());
        let mut source_order = 0_u64;
        for author in &self.authors {
            self.diagnostics.extend_from_slice(author.diagnostics());
            self.rules.extend(author.reindexed_rules(source_order));
            source_order = source_order.saturating_add(author.rules().len() as u64);
            self.keyframes.extend(author.keyframes().iter().cloned());
        }
        self.generation = self.generation.saturating_add(1);
    }

    /// The retained author sheets, in document order.
    pub fn author_sheets(&self) -> &[AuthorStylesheet] {
        &self.authors
    }

    /// Author-sheet slots which appear in `document.styleSheets`. Flattened
    /// `@import` sheets contribute to the cascade but remain children of their
    /// parent sheet rather than document-list entries.
    pub fn document_sheet_indexes(&self) -> Vec<usize> {
        self.authors
            .iter()
            .enumerate()
            .filter_map(|(index, sheet)| {
                (sheet.owner != StylesheetOwner::Imported).then_some(index)
            })
            .collect()
    }

    /// Look up either a direct document sheet or an imported child through its
    /// stable opaque CSSOM key.
    pub fn author_index_by_cssom_key(&self, key: &str) -> Option<usize> {
        self.authors
            .iter()
            .position(|sheet| sheet.cssom_key() == key)
    }

    pub fn cssom_key(&self, sheet: usize) -> Option<&str> {
        self.authors.get(sheet).map(AuthorStylesheet::cssom_key)
    }

    /// Top-level CSSOM rules. The resolver has already removed leading imports
    /// from the parser input, so restore their visible positions before the
    /// selected engine's parsed rule count.
    pub fn cssom_rule_count(&self, sheet: usize) -> Option<usize> {
        self.authors
            .get(sheet)
            .map(|sheet| sheet.imports.len().saturating_add(sheet.items().len()))
    }

    pub fn cssom_import_rule(&self, sheet: usize, index: usize) -> Option<CssomImportRule> {
        let parent = self.authors.get(sheet)?;
        let import = parent.imports.get(index)?;
        let child_sheet_key = import.child_sheet_id.and_then(|child_sheet_id| {
            self.authors
                .iter()
                .find(|sheet| sheet.sheet_id == child_sheet_id)
                .map(|sheet| sheet.cssom_key.clone())
        });
        Some(CssomImportRule {
            href: import.authored_url.clone(),
            media: import.media.clone(),
            child_sheet_key,
        })
    }

    pub fn cssom_import_owner(&self, sheet: usize) -> Option<CssomImportOwner> {
        let parent = self.authors.get(sheet)?.import_parent?;
        let parent_sheet = self
            .authors
            .iter()
            .find(|sheet| sheet.sheet_id == parent.sheet_id)?;
        Some(CssomImportOwner {
            parent_sheet_key: parent_sheet.cssom_key.clone(),
            import_index: parent.import_index,
        })
    }

    /// Monotonic author-cascade stamp for consumers retaining a style plane.
    /// This changes for sheet replacement/reordering as well as CSSOM rule
    /// mutations, even when individual parsed sheet generations happen to sum
    /// to the same value.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn has_sibling_dependencies(&self) -> bool {
        self.rules.iter().any(StyleRule::has_sibling_dependency)
    }

    pub(crate) fn has_structural_dependencies(&self) -> bool {
        self.rules.iter().any(StyleRule::has_structural_dependency)
    }

    pub(crate) fn has_container_queries(&self) -> bool {
        self.rules.iter().any(StyleRule::has_container_query)
    }

    /// CSSOM `insertRule` on one author sheet; the cascade views rebuild.
    pub fn insert_author_rule(
        &mut self,
        sheet: usize,
        rule: &str,
        index: usize,
    ) -> Result<usize, RuleMutationError> {
        let target = self
            .authors
            .get_mut(sheet)
            .ok_or(RuleMutationError::IndexSize)?;
        let inserted = target.insert_rule(rule, index)?;
        if target.media.is_some() {
            target.stylesheet =
                std::mem::take(&mut target.stylesheet).with_document_media(target.media.as_deref());
        }
        self.rebuild();
        Ok(inserted)
    }

    /// CSSOM mutation counts leading `@import` rules even though the selected
    /// parser receives their flattened child sheets separately. Import-rule
    /// mutation itself stays outside this projection because the immutable
    /// resource graph, not the parser, owns fetch and child replacement.
    pub fn insert_cssom_rule(
        &mut self,
        sheet: usize,
        rule: &str,
        index: usize,
    ) -> Result<usize, RuleMutationError> {
        let import_count = self
            .authors
            .get(sheet)
            .ok_or(RuleMutationError::IndexSize)?
            .imports
            .len();
        if index < import_count {
            return Err(RuleMutationError::Syntax(
                "inserting before a retained @import rule is not supported".to_owned(),
            ));
        }
        self.insert_author_rule(sheet, rule, index.saturating_sub(import_count))
            .map(|inserted| inserted.saturating_add(import_count))
    }

    /// CSSOM `deleteRule` on one author sheet; the cascade views rebuild.
    pub fn delete_author_rule(
        &mut self,
        sheet: usize,
        index: usize,
    ) -> Result<(), RuleMutationError> {
        let target = self
            .authors
            .get_mut(sheet)
            .ok_or(RuleMutationError::IndexSize)?;
        target.delete_rule(index)?;
        self.rebuild();
        Ok(())
    }

    pub fn delete_cssom_rule(
        &mut self,
        sheet: usize,
        index: usize,
    ) -> Result<(), RuleMutationError> {
        let import_count = self
            .authors
            .get(sheet)
            .ok_or(RuleMutationError::IndexSize)?
            .imports
            .len();
        if index < import_count {
            return Err(RuleMutationError::Syntax(
                "deleting a retained @import rule is not supported".to_owned(),
            ));
        }
        self.delete_author_rule(sheet, index.saturating_sub(import_count))
    }

    pub fn rules(&self) -> &[StyleRule] {
        &self.rules
    }

    pub fn diagnostics(&self) -> &[StylesheetDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn keyframes(&self, name: &str) -> Option<&Keyframes> {
        self.keyframes
            .iter()
            .rev()
            .find(|keyframes| keyframes.name().eq_ignore_ascii_case(name))
    }
}

/// Concrete Livery computed styles keyed by the source DOM node.
#[derive(Clone, Debug)]
pub struct StylePlane<Id> {
    values: HashMap<Id, ComputedValues>,
    custom: HashMap<Id, CustomProperties>,
    inline_diagnostics: HashMap<Id, Vec<DeclarationError>>,
    color_context: ColorComputeContext,
}

impl<Id> PartialEq for StylePlane<Id>
where
    Id: Eq + Hash,
{
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
            && self.custom == other.custom
            && self.inline_diagnostics == other.inline_diagnostics
            && self.color_context == other.color_context
    }
}

impl<Id> Default for StylePlane<Id> {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            custom: HashMap::new(),
            inline_diagnostics: HashMap::new(),
            color_context: ColorComputeContext::default(),
        }
    }
}

impl<Id> StylePlane<Id>
where
    Id: Eq + Hash,
{
    pub fn get(&self, id: Id) -> Option<&ComputedValues> {
        self.values.get(&id)
    }

    /// Resolve contextual colors for one element using the exact palette and
    /// used `color-scheme` that created this style plane.
    pub(crate) fn used_color_context(&self, id: Id) -> Option<UsedColorContext> {
        self.get(id)
            .map(|values| self.used_color_context_for(values))
    }

    /// Turn all color-bearing fields into numeric leaves for a paint or
    /// animation consumer. The retained plane itself stays contextual so
    /// inheritance and CSSOM can still use the authored computed model.
    pub(crate) fn with_used_colors(&self) -> Self
    where
        Id: Clone,
    {
        let mut used = self.clone();
        for values in used.values.values_mut() {
            let context = self.used_color_context_for(values);
            for &property in PropertyId::ALL {
                let value = resolve_property_used_colors(values.get(property), context);
                values
                    .set(property, value)
                    .expect("generated property read and write types agree");
            }
        }
        used
    }

    /// Resolve one tagged property for an animation endpoint. This is the
    /// shared endpoint conversion used by transitions and keyframes before
    /// their normal numeric interpolation dispatch.
    pub(crate) fn resolve_used_color_value(&self, id: Id, value: PropertyValue) -> PropertyValue {
        if let Some(context) = self.used_color_context(id) {
            resolve_property_used_colors(value, context)
        } else {
            value
        }
    }

    /// The element's computed custom-property map (harvest H1).
    pub fn custom_properties(&self, id: Id) -> Option<&CustomProperties> {
        self.custom.get(&id)
    }

    /// Serialize one computed longhand or custom property from this plane.
    /// This is shared by retained documents and scripted on-demand reads, so
    /// both JS and native CSSOM surfaces use the generated H2 value dispatch.
    pub fn computed_style(&self, id: Id, property: &str) -> Option<String> {
        self.computed_style_with_used_size(id, property, None)
    }

    /// Serialize a computed value with an optional retained layout size.
    ///
    /// CSSOM exposes used pixel values for width and height. A caller that has
    /// laid out the current style plane can supply that border-box size; the
    /// bounded lane uses it only when no padding or border changes the
    /// relationship between the fragment and the property value.
    pub fn computed_style_with_used_size(
        &self,
        id: Id,
        property: &str,
        used_size: Option<(f32, f32)>,
    ) -> Option<String> {
        self.computed_style_with_used_values(
            id,
            property,
            used_size.map(|border_box| UsedValueContext {
                border_box,
                containing_inline_size: None,
            }),
        )
    }

    /// Serialize a computed value with the layout bases needed by CSSOM used
    /// values. The current bounded surface covers box size and physical
    /// margins; other adorned-box properties remain explicit follow-ons.
    pub fn computed_style_with_used_values(
        &self,
        id: Id,
        property: &str,
        used: Option<UsedValueContext>,
    ) -> Option<String> {
        if property.starts_with("--") {
            return self.custom_properties(id)?.get(property).cloned();
        }
        let property = PropertyId::from_css_name(&property.to_ascii_lowercase())?;
        let values = self.get(id)?;
        let property = logical_inline_property(property, values);
        if let Some(used) = used
            && box_is_unadorned(values)
        {
            let value = match property {
                PropertyId::Width => Some(used.border_box.0),
                PropertyId::Height => Some(used.border_box.1),
                PropertyId::MarginTop => used_margin(values.margin_top, values, used),
                PropertyId::MarginRight => used_margin(values.margin_right, values, used),
                PropertyId::MarginBottom => used_margin(values.margin_bottom, values, used),
                PropertyId::MarginLeft => used_margin(values.margin_left, values, used),
                _ => None,
            };
            if let Some(value) = value {
                return Some(used_px(value));
            }
        }
        if property == PropertyId::Transform {
            let em = match values.font_size {
                FontSize::Value(LengthPercentage::Length(Length {
                    value,
                    unit: LengthUnit::Px,
                })) => value,
                _ => 16.0,
            };
            let reference_box = definite_transform_reference_box(values, em);
            return Some(values.transform.to_computed_css(em, reference_box));
        }
        Some(
            resolve_property_used_colors(values.get(property), self.used_color_context_for(values))
                .to_css_string(),
        )
    }

    pub(crate) fn get_mut(&mut self, id: Id) -> Option<&mut ComputedValues> {
        self.values.get_mut(&id)
    }

    pub(crate) fn resolve_relative_lengths(
        &mut self,
        id: Id,
        environment: livery::values::RelativeLengthEnvironment,
    ) {
        if let Some(computed) = self.values.get_mut(&id) {
            resolve_relative_lengths(computed, environment);
        }
    }

    pub fn inline_diagnostics(&self, id: Id) -> &[DeclarationError] {
        self.inline_diagnostics.get(&id).map_or(&[], Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn remove(&mut self, id: Id) {
        self.values.remove(&id);
        self.custom.remove(&id);
        self.inline_diagnostics.remove(&id);
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(Id) -> bool)
    where
        Id: Copy,
    {
        self.values.retain(|id, _| keep(*id));
        self.custom.retain(|id, _| keep(*id));
        self.inline_diagnostics.retain(|id, _| keep(*id));
    }

    fn with_color_context(color_context: ColorComputeContext) -> Self {
        Self {
            color_context,
            ..Self::default()
        }
    }

    fn used_color_context_for(&self, values: &ComputedValues) -> UsedColorContext {
        let scheme = values
            .color_scheme
            .used_scheme(self.color_context.preferred_scheme());
        let palette = self.color_context.palette();
        let inherited_foreground = palette.get(scheme, SystemColor::CanvasText);
        let foreground = values.color.resolve_used(UsedColorContext::with_palette(
            inherited_foreground,
            palette,
            scheme,
        ));
        UsedColorContext::with_palette(foreground, palette, scheme)
    }
}

fn resolve_property_used_colors(value: PropertyValue, context: UsedColorContext) -> PropertyValue {
    match value {
        PropertyValue::Color(color) => {
            PropertyValue::Color(ComputedColor::Absolute(color.resolve_used(context)))
        },
        PropertyValue::BackgroundImage(BackgroundImage::LinearGradient { from, to }) => {
            PropertyValue::BackgroundImage(BackgroundImage::LinearGradient {
                from: ComputedColor::Absolute(from.resolve_used(context)),
                to: ComputedColor::Absolute(to.resolve_used(context)),
            })
        },
        PropertyValue::BoxShadow(BoxShadow::Value(mut shadow)) => {
            shadow.color = ComputedColor::Absolute(shadow.color.resolve_used(context));
            PropertyValue::BoxShadow(BoxShadow::Value(shadow))
        },
        value => value,
    }
}

/// Resolve every element in a neutral Genet DOM through Livery.
pub fn resolve_styles<D>(
    dom: &D,
    style_set: &StyleSet,
    device: &Device,
    states: &InteractionStates<D::NodeId>,
) -> StylePlane<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let selector_tree = SelectorTree::new(dom, states);
    let mut plane = StylePlane::with_color_context(ColorComputeContext::from_device(device));
    resolve_subtree(
        &selector_tree,
        style_set,
        device,
        dom.document(),
        None,
        None,
        TreeCounts::Deferred,
        &mut plane,
    );
    plane
}

pub(crate) fn resolve_styles_with_containers<D>(
    dom: &D,
    style_set: &StyleSet,
    device: &Device,
    states: &InteractionStates<D::NodeId>,
    containers: &HashMap<D::NodeId, Vec<ContainerSnapshot>>,
) -> StylePlane<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let selector_tree = SelectorTree::new(dom, states);
    let mut plane = StylePlane::with_color_context(ColorComputeContext::from_device(device));
    resolve_subtree_with_containers(
        &selector_tree,
        style_set,
        device,
        dom.document(),
        None,
        None,
        TreeCounts::Deferred,
        &mut plane,
        Some(containers),
    );
    plane
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_subtree<D>(
    selector_tree: &SelectorTree<'_, D>,
    style_set: &StyleSet,
    device: &Device,
    id: D::NodeId,
    parent: Option<&ComputedValues>,
    parent_custom: Option<&CustomProperties>,
    tree_counts: TreeCounts,
    plane: &mut StylePlane<D::NodeId>,
) -> usize
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    resolve_subtree_with_containers(
        selector_tree,
        style_set,
        device,
        id,
        parent,
        parent_custom,
        tree_counts,
        plane,
        None,
    )
}

/// The one-based position of every element child of `id`, in tree order, and
/// the size of that sibling group. Non-element children keep the deferred
/// counts: they never carry a style of their own, and they must not shift the
/// ordinals of the elements around them.
fn child_tree_counts<D>(dom: &D, id: D::NodeId) -> Vec<(D::NodeId, TreeCounts)>
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    let children: Vec<D::NodeId> = dom.dom_children(id).collect();
    let count = children
        .iter()
        .filter(|child| dom.kind(**child) == NodeKind::Element)
        .count() as u32;
    let mut index = 0;
    children
        .into_iter()
        .map(|child| {
            if dom.kind(child) == NodeKind::Element {
                index += 1;
                (child, TreeCounts::new(index, count))
            } else {
                (child, TreeCounts::Deferred)
            }
        })
        .collect()
}

/// The counts for one element, recovered from its parent. An incremental
/// restyle enters mid-tree, so a restyle root has to look its own ordinal up
/// rather than inherit it from the walk.
pub(crate) fn tree_counts_of<D>(dom: &D, id: D::NodeId) -> TreeCounts
where
    D: LayoutDom,
    D::NodeId: Copy + Eq,
{
    dom.parent(id)
        .and_then(|parent| {
            child_tree_counts(dom, parent)
                .into_iter()
                .find_map(|(child, counts)| (child == id).then_some(counts))
        })
        .unwrap_or(TreeCounts::Deferred)
}

#[allow(clippy::too_many_arguments)]
fn resolve_subtree_with_containers<D>(
    selector_tree: &SelectorTree<'_, D>,
    style_set: &StyleSet,
    device: &Device,
    id: D::NodeId,
    parent: Option<&ComputedValues>,
    parent_custom: Option<&CustomProperties>,
    tree_counts: TreeCounts,
    plane: &mut StylePlane<D::NodeId>,
    containers: Option<&HashMap<D::NodeId, Vec<ContainerSnapshot>>>,
) -> usize
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if selector_tree.dom().kind(id) == NodeKind::Element {
        let element = selector_tree.element(id).expect("element kind has adapter");
        let candidates = containers
            .and_then(|containers| containers.get(&id))
            .map_or(&[][..], Vec::as_slice);
        let mut matched = Vec::new();
        let mut matched_custom = Vec::new();
        for rule in &style_set.rules {
            matched.extend(rule.matched_declarations_with_containers(&element, device, candidates));
            matched_custom.extend(
                rule.matched_custom_declarations_with_containers(&element, device, candidates),
            );
        }

        if let Some(inline) =
            selector_tree
                .dom()
                .attribute(id, &Namespace::from(""), &LocalName::from("style"))
        {
            let block = parse_declaration_block(inline);
            if !block.errors.is_empty() {
                plane.inline_diagnostics.insert(id, block.errors);
            }
            let inline_order = u64::MAX.saturating_sub(65_535);
            matched.extend(block.declarations.into_iter().enumerate().map(
                |(index, declaration)| MatchedDeclaration {
                    declaration,
                    origin: Origin::Author,
                    layer: CascadeLayer::Unlayered,
                    specificity: Specificity::INLINE,
                    source_order: inline_order.saturating_add(index as u64),
                },
            ));
            matched_custom.extend(block.custom.into_iter().enumerate().map(
                |(index, declaration)| MatchedCustomDeclaration {
                    declaration,
                    origin: Origin::Author,
                    layer: CascadeLayer::Unlayered,
                    specificity: Specificity::INLINE,
                    source_order: inline_order.saturating_add(index as u64),
                },
            ));
        }

        let (mut computed, custom) = cascade_with_logical_inline_properties(
            parent,
            parent_custom,
            matched,
            matched_custom,
            ColorComputeContext::from_device(device),
        );
        resolve_viewport_units(&mut computed, device, tree_counts);
        resolve_font_metrics(&mut computed, parent);
        let mut resolved = 1;
        for (child, child_counts) in child_tree_counts(selector_tree.dom(), id) {
            resolved += resolve_subtree_with_containers(
                selector_tree,
                style_set,
                device,
                child,
                Some(&computed),
                Some(&custom),
                child_counts,
                plane,
                containers,
            );
        }
        plane.values.insert(id, computed);
        plane.custom.insert(id, custom);
        resolved
    } else {
        let mut resolved = 0;
        for (child, child_counts) in child_tree_counts(selector_tree.dom(), id) {
            resolved += resolve_subtree_with_containers(
                selector_tree,
                style_set,
                device,
                child,
                parent,
                parent_custom,
                child_counts,
                plane,
                containers,
            );
        }
        resolved
    }
}

/// Resolve logical inline properties after the winning writing mode is known,
/// while leaving declarations in their ordinary cascade priority order beside
/// their physical counterparts. Generated logical fields retain the CSSOM
/// values; layout consumes the mapped physical longhands.
fn cascade_with_logical_inline_properties(
    parent: Option<&ComputedValues>,
    parent_custom: Option<&CustomProperties>,
    matched: Vec<MatchedDeclaration>,
    matched_custom: Vec<MatchedCustomDeclaration>,
    color_context: ColorComputeContext,
) -> (ComputedValues, CustomProperties) {
    let (logical, _) = cascade_with_custom_context(
        parent,
        parent_custom,
        matched.clone(),
        matched_custom.clone(),
        color_context,
    );
    let inline_size = logical.inline_size;
    let mapped = matched.into_iter().map(|mut declaration| {
        let property = logical_inline_property(declaration.declaration.property, &logical);
        if property != declaration.declaration.property {
            declaration.declaration.property = property;
        }
        declaration
    });
    let (mut computed, custom) =
        cascade_with_custom_context(parent, parent_custom, mapped, matched_custom, color_context);
    computed.inline_size = inline_size;
    (computed, custom)
}

fn logical_inline_property(property: PropertyId, values: &ComputedValues) -> PropertyId {
    if property != PropertyId::InlineSize {
        return property;
    }
    if values.writing_mode.is_vertical() {
        PropertyId::Height
    } else {
        PropertyId::Width
    }
}

fn resolve_viewport_units(computed: &mut ComputedValues, device: &Device, tree_counts: TreeCounts) {
    let environment = livery::values::RelativeLengthEnvironment::viewport(device.viewport_sizes)
        .with_vertical_writing(computed.writing_mode.is_vertical())
        .with_tree_counts(tree_counts);
    resolve_relative_lengths(computed, environment);
}

fn resolve_relative_lengths(
    computed: &mut ComputedValues,
    environment: livery::values::RelativeLengthEnvironment,
) {
    for &property in PropertyId::ALL {
        let value = computed.get(property).resolve_relative_lengths(environment);
        computed
            .set(property, value)
            .expect("generated property read and write types agree");
    }
}

fn resolve_font_metrics(computed: &mut ComputedValues, parent: Option<&ComputedValues>) {
    let parent_size = parent.map_or(16.0, |style| match style.font_size {
        FontSize::Value(LengthPercentage::Length(Length {
            value,
            unit: LengthUnit::Px,
        })) => value,
        _ => 16.0,
    });
    let font_size = match computed.font_size {
        FontSize::Medium => 16.0,
        FontSize::Value(value) => resolve_length_percentage(value, parent_size, parent_size),
    }
    .max(0.0);
    computed.font_size = FontSize::Value(LengthPercentage::Length(Length::px(font_size)));
    computed.transform.resolve_lengths(font_size, 16.0);

    if let LineHeight::Value(value) = computed.line_height {
        computed.line_height = LineHeight::Value(LengthPercentage::Length(Length::px(
            resolve_length_percentage(value, font_size, font_size).max(0.0),
        )));
    }
    for spacing in [&mut computed.letter_spacing, &mut computed.word_spacing] {
        if let livery::values::Spacing::Length(value) = *spacing {
            *spacing =
                livery::values::Spacing::Length(value.resolve_font_relative(font_size, 16.0));
        }
    }
}

fn resolve_length_percentage(value: LengthPercentage, em: f32, percentage_basis: f32) -> f32 {
    match value {
        LengthPercentage::Zero => 0.0,
        LengthPercentage::Length(length) => length.unit.to_px(length.value, em, 16.0),
        LengthPercentage::Percentage(value) => percentage_basis * value,
        LengthPercentage::Calc(calc) => {
            percentage_basis * calc.percentage + calc.px + calc.em * em + calc.rem * 16.0
        },
        LengthPercentage::Math(math) => {
            LengthPercentage::Math(math).to_px(em, 16.0, percentage_basis)
        },
    }
}

fn definite_transform_reference_box(values: &ComputedValues, em: f32) -> Option<(f32, f32)> {
    // Without retained layout, this CSSOM path can derive a border box only
    // for a definite, unadorned box. Paint always receives the actual fragment.
    if !box_is_unadorned(values) {
        return None;
    }
    Some((
        definite_size(values.width, em)?,
        definite_size(values.height, em)?,
    ))
}

fn box_is_unadorned(values: &ComputedValues) -> bool {
    ![
        values.padding_top,
        values.padding_right,
        values.padding_bottom,
        values.padding_left,
    ]
    .into_iter()
    .any(|padding| padding != Padding::ZERO)
        && ![
            values.border_top_style,
            values.border_right_style,
            values.border_bottom_style,
            values.border_left_style,
        ]
        .into_iter()
        .any(|style| style != BorderStyle::None)
}

fn used_px(value: f32) -> String {
    let value = (value * 10_000.0).round() / 10_000.0;
    if value == 0.0 {
        "0px".to_string()
    } else {
        Length::px(value).to_string()
    }
}

fn used_margin(margin: Margin, values: &ComputedValues, context: UsedValueContext) -> Option<f32> {
    let Margin::Value(value) = margin else {
        return None;
    };
    let basis = if value.has_percentage() {
        context.containing_inline_size?
    } else {
        0.0
    };
    let em = match values.font_size {
        FontSize::Value(LengthPercentage::Length(Length {
            value,
            unit: LengthUnit::Px,
        })) => value,
        _ => 16.0,
    };
    Some(value.to_px(em, 16.0, basis))
}

fn definite_size(size: Size, em: f32) -> Option<f32> {
    let Size::Value(value) = size else {
        return None;
    };
    (!value.has_percentage()).then(|| value.to_px(em, 16.0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(css: &str, source_order: u64) -> MatchedDeclaration {
        let mut block = parse_declaration_block(css);
        assert!(block.errors.is_empty(), "{css}: {:?}", block.errors);
        assert_eq!(block.declarations.len(), 1, "{css}");
        MatchedDeclaration {
            declaration: block.declarations.remove(0),
            origin: Origin::Author,
            layer: CascadeLayer::Unlayered,
            specificity: Specificity::INLINE,
            source_order,
        }
    }

    #[test]
    fn inline_size_maps_to_the_winning_physical_axis() {
        let (horizontal, _) = cascade_with_logical_inline_properties(
            None,
            None,
            vec![matched("inline-size: 25px", 0), matched("width: 50px", 1)],
            vec![],
            ColorComputeContext::default(),
        );
        assert_eq!(horizontal.inline_size, "25px".parse().unwrap());
        assert_eq!(horizontal.width, "50px".parse().unwrap());

        let (vertical, _) = cascade_with_logical_inline_properties(
            None,
            None,
            vec![
                matched("writing-mode: vertical-rl", 0),
                matched("height: 50px", 1),
                matched("inline-size: 25px", 2),
            ],
            vec![],
            ColorComputeContext::default(),
        );
        assert_eq!(vertical.inline_size, "25px".parse().unwrap());
        assert_eq!(vertical.height, "25px".parse().unwrap());
    }
}
