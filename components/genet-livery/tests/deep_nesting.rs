// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! A deeply nested document must not overflow the stack.
//!
//! Measured 2026-09-03 (`docs/2026-08-09_cambium_desktop_host_g1_receipt.md`,
//! Findings 2026-09-03): a debug-build layout of plain nested `<div>`s wants
//! ~450 KiB of baseline plus ~127 KiB per DOM nesting level, and a Rust MSVC
//! binary's main thread gets a 1 MiB `SizeOfStackReserve`. That put the ceiling
//! at about eight nested elements for *any* consumer on Windows, with no leaf
//! content and no consumer code involved. The four per-level descents the pass
//! is made of now grow their own stack on demand — see
//! `genet_livery::with_recursion_stack` and `buckram::box_tree`'s
//! `with_box_tree_stack` — so depth is bounded by memory, not by the thread's
//! reserve.
//!
//! This pins that with 64 levels on a 256 KiB thread, which is under the
//! *baseline* appetite, let alone 64 levels of it. Without the guards the
//! process dies with `STATUS_STACK_OVERFLOW` rather than failing an assertion —
//! that is the shape of the defect, and it is what the pre-fix run does at both
//! depths below.

use genet_livery::{Device, InteractionStates, StyleSet, layout, resolve_styles};
use genet_static_dom::StaticDocument;

/// `<div><div>...</div></div>` nested `depth` levels deep, no leaf content.
fn nested_divs(depth: usize) -> String {
    let mut html = String::with_capacity(depth * 19);
    for _ in 0..depth {
        html.push_str("<div class=n>");
    }
    for _ in 0..depth {
        html.push_str("</div>");
    }
    html
}

/// The same two phases `cambium_rootstock::OwnedLayout::rebuild` runs, in the
/// same order: the cascade, then the layout. Both descend per DOM level and
/// both used to overflow.
fn lay_out_nested(depth: usize) -> usize {
    let dom = StaticDocument::parse(&nested_divs(depth));
    let styles = resolve_styles(
        &dom,
        &StyleSet::cambium(&[".n { padding: 1px; border: 1px solid black; }"]),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    layout(&dom, &styles, 800.0, 600.0)
        .expect("layout of nested divs")
        .len()
}

/// Deliberately below the ~450 KiB the pre-fix pass wanted before it descended
/// a single level: a thread this small proves the growth path carries the whole
/// descent rather than just its tail.
const SMALL_STACK: usize = 256 * 1024;

fn on_small_stack(name: &str, depth: usize) -> usize {
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(SMALL_STACK)
        .spawn(move || lay_out_nested(depth))
        .expect("spawn small-stack thread")
        .join()
        .expect("layout must not overflow the stack")
}

#[test]
fn sixty_four_levels_lay_out_on_a_small_stack() {
    let fragments = on_small_stack("deep-nesting-small-stack", 64);
    assert!(
        fragments >= 64,
        "64 nested divs must each produce a fragment, got {fragments}"
    );
}

/// The shallow case on the same thread. This is the baseline appetite on its
/// own, and it overflowed too, so keeping it means a regression that only
/// lowers the ceiling still shows up here rather than only in a consumer.
#[test]
fn eight_levels_lay_out_on_a_small_stack() {
    let fragments = on_small_stack("shallow-nesting-small-stack", 8);
    assert!(
        fragments >= 8,
        "8 nested divs must each produce a fragment, got {fragments}"
    );
}
