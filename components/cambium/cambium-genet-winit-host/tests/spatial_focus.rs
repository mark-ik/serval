// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Hold Tab, steer with the arrows.
//!
//! The grid below is the case document order handles badly: 5×5 buttons, so
//! crossing it diagonally costs 24 Tab presses and lands you somewhere you have
//! to count to predict. Spatial navigation costs two, in the direction you were
//! already looking.

use cambium::{AnyView, GenetCtx, GenetElement, button, el};
use cambium_genet_winit_host::{Harness, HostOptions, Init, inert_hooks};
use winit::keyboard::NamedKey;

const COLS: usize = 5;
const ROWS: usize = 5;
const CELL: f32 = 80.0;
const GAP: f32 = 10.0;

#[derive(Default)]
struct Grid {
    pressed: Vec<&'static str>,
}

type Child = Box<dyn AnyView<Grid, (), GenetCtx, GenetElement>>;
type Logic = fn(&Grid) -> Child;

/// `r{row}c{col}`, laid out absolutely so every assertion is arithmetic.
fn name(row: usize, col: usize) -> String {
    format!("r{row}c{col}")
}

fn root(_state: &Grid) -> Child {
    let cells: Vec<Child> = (0..ROWS)
        .flat_map(|row| (0..COLS).map(move |col| (row, col)))
        .map(|(row, col)| -> Child {
            let x = col as f32 * (CELL + GAP);
            let y = row as f32 * (CELL + GAP);
            Box::new(
                button(name(row, col), |s: &mut Grid, _| s.pressed.push("hit"))
                    .attr("class", "cell")
                    .attr(
                        "style",
                        format!(
                            "position:absolute;left:{x}px;top:{y}px;\
                             width:{CELL}px;height:{CELL}px;"
                        ),
                    ),
            )
        })
        .collect();
    Box::new(el("div", cells).attr("style", "position:relative;width:500px;height:500px;"))
}

// `display: block` is load-bearing: genet's UA default makes `button`
// inline-block, and inline-level boxes share their line's fragment, so an
// inline control has no rect of its own for spatial navigation to reason about.
const SHEET: &str = ".cell { display: block; }";

fn harness() -> Harness<Grid, Logic, Child> {
    let mut h = Harness::new(SHEET, Grid::default(), root as Logic);
    h.layout_at(500.0, 500.0);
    h
}

/// Which cell has focus, by its label.
fn focused_label(h: &Harness<Grid, Logic, Child>) -> Option<String> {
    let node = h.focus()?;
    Some(h.with_dom(|dom| {
        use layout_dom_api::LayoutDom as _;
        dom.dom_children(node)
            .filter_map(|c| dom.text(c).map(str::to_string))
            .collect::<String>()
    }))
}

/// The whole point: Right and Down move one cell, in the direction meant.
#[test]
fn arrows_move_one_cell_while_tab_is_held() {
    let mut h = harness();
    // Press Tab and keep it down: the press traverses to the first cell, and
    // from there the arrows steer. No waiting for a repeat.
    h.hold_tab(true);
    assert!(h.tab_held(), "Tab held arms spatial navigation at once");
    assert_eq!(focused_label(&h).as_deref(), Some("r0c0"));

    h.key_named(NamedKey::ArrowRight);
    assert_eq!(focused_label(&h).as_deref(), Some("r0c1"));

    h.key_named(NamedKey::ArrowDown);
    assert_eq!(focused_label(&h).as_deref(), Some("r1c1"));

    h.key_named(NamedKey::ArrowLeft);
    assert_eq!(focused_label(&h).as_deref(), Some("r1c0"));

    h.key_named(NamedKey::ArrowUp);
    assert_eq!(focused_label(&h).as_deref(), Some("r0c0"));
}

/// Two presses instead of twenty-four. This is the case that motivated it.
#[test]
fn crossing_the_grid_costs_two_presses_not_twenty_four() {
    let mut h = harness();
    h.hold_tab(true);
    for _ in 0..(COLS - 1) {
        h.key_named(NamedKey::ArrowRight);
    }
    for _ in 0..(ROWS - 1) {
        h.key_named(NamedKey::ArrowDown);
    }
    assert_eq!(
        focused_label(&h).as_deref(),
        Some("r4c4"),
        "eight arrows reach the far corner; document order would take 24 tabs",
    );
}

/// Nothing beyond the edge, so focus stays put rather than wrapping into
/// whatever happens to be first in document order.
#[test]
fn an_edge_holds_rather_than_wrapping() {
    let mut h = harness();
    h.hold_tab(true);
    h.key_named(NamedKey::ArrowUp);
    assert_eq!(
        focused_label(&h).as_deref(),
        Some("r0c0"),
        "up from the top row stays: spatial movement is not a ring",
    );
    h.key_named(NamedKey::ArrowLeft);
    assert_eq!(focused_label(&h).as_deref(), Some("r0c0"));
}

/// Tapping Tab is untouched — it still walks document order, and the arrows go
/// back to meaning whatever the focused control says they mean.
#[test]
fn a_tap_is_unchanged_and_releasing_leaves_the_mode() {
    let mut h = harness();
    h.tab(true);
    h.tab(true);
    assert_eq!(
        focused_label(&h).as_deref(),
        Some("r0c1"),
        "two taps walk document order, exactly as before",
    );
    assert!(!h.tab_held(), "a tap leaves nothing armed");

    // Holding Tab traverses once too — the steering starts from wherever that
    // press left you (r0c1 → r0c2), which is what makes a tap and a hold the
    // same gesture up to the moment an arrow arrives.
    h.hold_tab(true);
    assert_eq!(focused_label(&h).as_deref(), Some("r0c2"));
    h.key_named(NamedKey::ArrowDown);
    assert_eq!(focused_label(&h).as_deref(), Some("r1c2"));

    h.release_tab();
    assert!(!h.tab_held());
    // With Tab released the arrow is an ordinary key again: it reaches the
    // focused control rather than steering, so focus does not move.
    h.key_named(NamedKey::ArrowDown);
    assert_eq!(
        focused_label(&h).as_deref(),
        Some("r1c2"),
        "released, arrows belong to the focused control again",
    );
}

/// With the option off, holding Tab does nothing special — an application whose
/// arrow keys are already spoken for keeps them.
#[test]
fn the_option_turns_it_off() {
    let mut h = Harness::with_hooks_and_options(
        Init {
            state: Grid::default(),
            logic: root as Logic,
            sheet: SHEET.to_string(),
        },
        inert_hooks(),
        HostOptions {
            spatial_focus: false,
            ..Default::default()
        },
    );
    h.layout_at(500.0, 500.0);
    h.tab(true);
    assert_eq!(focused_label(&h).as_deref(), Some("r0c0"));

    // With the mode off, holding Tab arms nothing.
    h.hold_tab(true);
    assert!(!h.tab_held(), "the mode is off");
    assert_eq!(
        focused_label(&h).as_deref(),
        Some("r0c1"),
        "and the press traverses, as Tab always does",
    );

    let before = focused_label(&h);
    h.key_named(NamedKey::ArrowRight);
    assert_eq!(
        focused_label(&h),
        before,
        "and the arrow belongs to the control, not to focus",
    );
}
