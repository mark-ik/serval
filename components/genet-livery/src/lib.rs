//! Genet's concrete integration path for the clean-room Livery engine.
//!
//! The shared boundary is [`layout_dom_api::LayoutDom`]. Livery owns cascade
//! values and Buckram owns CSS geometry on this side of the document router.
//! The retired Stylo / `genet-layout` compatibility cone is not in the build.

#![forbid(unsafe_code)]

mod box_tree;
mod document;
mod dom;
mod invalidation;
mod layout;
mod legacy_color;
mod paint;
mod presentational_hints;
mod style;
// K4d6b: Buckram lays out live tables' block axis through the phase order it
// owns; a table it cannot lay out defers under a named gap.
pub mod table_block;
mod table_sizing;
// K4h: Buckram owns live table inline sizing and dispatch on every route;
// unresolved sizing input remains a named counter, never a backend route.
pub mod table_shadow;
// K4e1: the table wrapper box carries the properties CSS 2.1 section 17.4
// and CSS Tables 3 section 3.6.1 take off the grid.
mod table_wrapper;
mod text;
mod text_fragment;

pub use buckram::{
    AnonymousBoxKind, Baselines, BoxGeneration, BoxId, BoxOrigin, BreakToken, ContainingBlock,
    ContainingBlockEstablishment, CssBox, CssBoxTree, DisplayInside, DisplayOutside, DisplayRole,
    FormattingContextKind, Fragment, FragmentId, FragmentTree, FragmentationContextId,
    InternalTableRole, LayoutResult, LogicalRect, PhysicalRect, PositioningScheme, PseudoElement,
};
pub use document::{ClickOutcome, LayoutDamage, LayoutDamageKind, LinkTarget, LiveryDocument};
pub use dom::{ElementRef, InteractionStates, SelectorTree};
pub use invalidation::{AttributeSnapshot, ElementSnapshot, IncrementalStyle, RestyleStats};
pub use layout::{
    BlockAlgorithmCounts, LayoutError, LiveryLayout, content_box_size, hit_test,
    hit_test_with_scroll, layout, layout_with_text_system, resolve_container_query_styles,
    resolve_container_query_styles_with_images, resolve_container_relative_styles,
    used_value_context,
};
pub use livery::media::{Device, ViewportSize, ViewportSizes};
pub use livery::selector::StatePseudoClass;
pub use livery::stylesheet::{CssomRule, CssomRuleKind, FontFaceRule, RuleMutationError};
pub use livery::{
    PropertyId, canonicalize_specified_longhand, canonicalize_specified_value,
    is_implemented_shorthand,
};
pub use paint::{
    LiveryPaintList, emit_paint_list, emit_paint_list_with_text_system,
    emit_paint_list_with_text_system_scrolled_with_images,
};
pub use presentational_hints::{
    LegacyDescendantAlignment, PresentationalDeclarations, PresentationalHintDiagnostic,
    PresentationalHintProvider, PresentationalHints,
};
pub use style::{
    AuthorStylesheet, CssomImportOwner, CssomImportRule, StylePlane, StyleSet, UsedValueContext,
    resolve_styles, resolve_styles_with_presentational_hints,
};
pub use text::{TextRange, TextRect, TextSelection, TextSystem};
pub use text_fragment::{
    NavigationFragment, TextDirective, parse_fragment_directive, parse_text_directive,
};

/// Clean-room UA defaults for the bounded Cambium structural lane.
///
/// This deliberately follows the lane contract rather than importing the
/// retired Stylo route's larger UA sheet.
pub const CAMBIUM_UA_DEFAULTS: &str = r#"
html, body, main, section, article, header, footer, nav, aside,
div, blockquote, h1, h2, h3, h4, h5, h6, p, ul, ol, pre {
    display: block;
}
li { display: list-item; }
hr {
    display: block;
    color: gray;
    border-style: inset;
    border-width: 1px;
    margin: 0.5em auto;
    overflow: hidden;
}

table { display: table; border-collapse: separate; border-spacing: 2px; }
thead { display: table-header-group; vertical-align: middle; }
tbody { display: table-row-group; vertical-align: middle; }
tfoot { display: table-footer-group; vertical-align: middle; }
colgroup { display: table-column-group; }
col { display: table-column; }
tr { display: table-row; }
td, th { display: table-cell; padding: 1px; vertical-align: inherit; }
caption { display: table-caption; }

thead, tbody, tfoot, tr {
    border-top-color: inherit;
    border-right-color: inherit;
    border-bottom-color: inherit;
    border-left-color: inherit;
}

table[rules=none i], table[rules=groups i], table[rules=rows i],
table[rules=cols i], table[rules=all i], table[frame=void i],
table[frame=above i], table[frame=below i], table[frame=hsides i],
table[frame=lhs i], table[frame=rhs i], table[frame=vsides i],
table[frame=box i], table[frame=border i],
table[rules=none i] > tr > td, table[rules=none i] > tr > th,
table[rules=groups i] > tr > td, table[rules=groups i] > tr > th,
table[rules=rows i] > tr > td, table[rules=rows i] > tr > th,
table[rules=cols i] > tr > td, table[rules=cols i] > tr > th,
table[rules=all i] > tr > td, table[rules=all i] > tr > th,
table[rules=none i] > thead > tr > td, table[rules=none i] > thead > tr > th,
table[rules=groups i] > thead > tr > td, table[rules=groups i] > thead > tr > th,
table[rules=rows i] > thead > tr > td, table[rules=rows i] > thead > tr > th,
table[rules=cols i] > thead > tr > td, table[rules=cols i] > thead > tr > th,
table[rules=all i] > thead > tr > td, table[rules=all i] > thead > tr > th,
table[rules=none i] > tbody > tr > td, table[rules=none i] > tbody > tr > th,
table[rules=groups i] > tbody > tr > td, table[rules=groups i] > tbody > tr > th,
table[rules=rows i] > tbody > tr > td, table[rules=rows i] > tbody > tr > th,
table[rules=cols i] > tbody > tr > td, table[rules=cols i] > tbody > tr > th,
table[rules=all i] > tbody > tr > td, table[rules=all i] > tbody > tr > th,
table[rules=none i] > tfoot > tr > td, table[rules=none i] > tfoot > tr > th,
table[rules=groups i] > tfoot > tr > td, table[rules=groups i] > tfoot > tr > th,
table[rules=rows i] > tfoot > tr > td, table[rules=rows i] > tfoot > tr > th,
table[rules=cols i] > tfoot > tr > td, table[rules=cols i] > tfoot > tr > th,
table[rules=all i] > tfoot > tr > td, table[rules=all i] > tfoot > tr > th {
    border-top-color: black;
    border-right-color: black;
    border-bottom-color: black;
    border-left-color: black;
}

button, input, select, textarea {
    display: inline-block;
}

img {
    display: inline-block;
}

iframe {
    border-top-width: 2px;
    border-right-width: 2px;
    border-bottom-width: 2px;
    border-left-width: 2px;
    border-top-style: inset;
    border-right-style: inset;
    border-bottom-style: inset;
    border-left-style: inset;
}

head, title, meta, link, style, script, template {
    display: none;
}

html { width: 100%; }
body { inline-size: 100%; margin: 8px; }
h1 { font-size: 2em; margin: 0.67em 0; font-weight: bold; }
h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: bold; }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold; }
p, ul, ol, pre { margin: 1em 0; }
blockquote { margin: 1em 40px; }
ul, ol { padding-left: 40px; }
ul { list-style-type: disc; }
ol { list-style-type: decimal; }
pre { white-space: pre; }
"#;
