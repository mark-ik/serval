/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! V2 of the layout verification ladder: the incremental splice must **refine**
//! a full relayout.
//!
//! `IncrementalLayout::apply` may re-lay-out only the mutated subtree and splice
//! the result into the retained `FragmentPlane`. That is a refinement claim —
//! the spliced plane should be indistinguishable from the plane a fresh layout of
//! the same DOM produces — and its failure mode is silent: a stale rect, not a
//! crash. Nothing else in the tree checks it, so today it rests on hand-reasoned
//! bail conditions plus whichever specific shapes happen to have unit tests.
//!
//! This runs each corpus case down both paths and compares the whole plane, both
//! tables. See `docs/2026-08-10_layout_verification_ladder_plan.md`.
//!
//! Three disciplines, each answering a way this harness could be green and
//! worthless:
//!
//! - **Yield is reported and floored.** The splice bails to a full relayout on
//!   five conditions, and a bailed case runs the *same* code down both arms, so
//!   it proves nothing. [`splice_refines_full_relayout`] prints how many cases
//!   actually spliced and fails if too few do, so a change that quietly stops
//!   exercising the splice turns the suite red instead of leaving it green.
//! - **The comparator is shown to fail.** [`the_comparator_detects_a_stale_plane`]
//!   feeds it a knowingly stale plane and requires a complaint. A checker that has
//!   never failed is not known to work — the discipline behind E4's deliberate
//!   bad-trace fixture (`docs/tla/post_message_trace/README.md`).
//! - **Both tables are compared.** The plane grew a second table in 2026-08-09
//!   (`FragmentPlane::inline_boxes`, per-inline-box geometry), and its splice
//!   staleness question was settled by inspection. Comparing only `rects` would
//!   have re-created exactly the blind spot this rung exists to remove.
//!
//! Deliberately a hand-built corpus rather than a property-test generator: the
//! plan's stop rule keeps a new dependency behind an explicit decision, and
//! curated shapes reach the splice path far more reliably than random mutation
//! (most random edits bail).

use genet_layout::{Applied, FragmentPlane, IncrementalLayout};
use genet_scripted_dom::ScriptedDom;
use html5ever::ns;
use layout_dom_api::{DomMutation, LayoutDom, LayoutDomMut, QualName};

type Id = <ScriptedDom as LayoutDom>::NodeId;

const W: f32 = 800.0;
const H: f32 = 600.0;
/// Layout is float arithmetic through two different call paths; equal-to-the-bit
/// is not the claim. A third of a pixel is far below anything visible and far
/// above the noise the two paths legitimately differ by.
const TOLERANCE: f32 = 0.34;

fn html(local: &str) -> QualName {
    QualName::new(None, ns!(html), local.into())
}

fn attr(local: &str) -> QualName {
    QualName::new(None, ns!(), local.into())
}

fn drain(dom: &mut ScriptedDom) -> Vec<DomMutation<Id>> {
    let mut v = Vec::new();
    dom.drain_mutations(&mut v);
    v
}

/// `<html><body>` with the body sized and made a BFC.
///
/// Both are load-bearing for reaching the splice at all: an auto-height body
/// grows when content is appended, which trips the outer-size bail, and a
/// non-BFC body lets the UA `p { margin }` collapse through it, which trips the
/// margin-collapse bail. This is the shape `structural_change_splices_incrementally`
/// established; the corpus reuses it so cases test the splice rather than the
/// fallback.
fn body_scaffold(dom: &mut ScriptedDom) -> Id {
    let root = dom.document();
    let h = dom.create_element(html("html"));
    dom.append_child(root, h);
    let body = dom.create_element(html("body"));
    dom.append_child(h, body);
    body
}

fn text_child(dom: &mut ScriptedDom, parent: Id, text: &str) -> Id {
    let t = dom.create_text(text);
    dom.append_child(parent, t);
    t
}

/// One corpus entry: build a DOM, then mutate it. `build` returns the handles
/// `mutate` needs.
struct Case {
    name: &'static str,
    sheet: &'static [&'static str],
    build: fn(&mut ScriptedDom) -> Vec<Id>,
    mutate: fn(&mut ScriptedDom, &[Id]),
}

/// Every difference between two planes, as human-readable complaints. Empty
/// means the spliced plane is indistinguishable from the reference.
///
/// Both tables, and the key sets as well as the values: a splice that *forgets*
/// to write an entry and one that writes a stale entry are different bugs, and
/// a comparison over only the shared keys would miss the first.
fn differences(spliced: &FragmentPlane<Id>, full: &FragmentPlane<Id>) -> Vec<String> {
    let mut out = Vec::new();

    for (id, want) in full.iter() {
        let Some(got) = spliced.rect_of(*id) else {
            out.push(format!("{id:?}: missing from the spliced plane"));
            continue;
        };
        let fields: [(&str, f32, f32); 4] = [
            ("x", got.location.x, want.location.x),
            ("y", got.location.y, want.location.y),
            ("w", got.size.width, want.size.width),
            ("h", got.size.height, want.size.height),
        ];
        for (field, got_v, want_v) in fields {
            if (got_v - want_v).abs() > TOLERANCE {
                out.push(format!("{id:?}: {field} spliced {got_v} vs full {want_v}"));
            }
        }
    }
    for (id, _) in spliced.iter() {
        if full.rect_of(*id).is_none() {
            out.push(format!("{id:?}: stale entry, absent from a full relayout"));
        }
    }

    for (id, want) in full.iter_inline_boxes() {
        let Some(got) = spliced.inline_box_of(*id) else {
            out.push(format!("{id:?}: missing from the spliced inline-box table"));
            continue;
        };
        if got.leaf != want.leaf {
            out.push(format!(
                "{id:?}: inline leaf spliced {:?} vs full {:?}",
                got.leaf, want.leaf
            ));
        }
        let fields: [(&str, f32, f32); 4] = [
            ("inline x", got.x, want.x),
            ("inline y", got.y, want.y),
            ("inline w", got.width, want.width),
            ("inline h", got.height, want.height),
        ];
        for (field, got_v, want_v) in fields {
            if (got_v - want_v).abs() > TOLERANCE {
                out.push(format!("{id:?}: {field} spliced {got_v} vs full {want_v}"));
            }
        }
    }
    for (id, _) in spliced.iter_inline_boxes() {
        if full.inline_box_of(*id).is_none() {
            out.push(format!(
                "{id:?}: stale inline-box entry, absent from a full relayout"
            ));
        }
    }

    out
}

/// The corpus. Each case is a shape the splice is *meant* to handle; a case that
/// bails is not a failure, it just does not contribute to yield.
fn corpus() -> Vec<Case> {
    vec![
        Case {
            name: "append a sized <p>",
            sheet: &["body { height: 200px; overflow: hidden; } p { height: 20px; }"],
            build: |dom| vec![body_scaffold(dom)],
            mutate: |dom, ids| {
                let p = dom.create_element(html("p"));
                dom.append_child(ids[0], p);
            },
        },
        Case {
            name: "remove one of three sized <p>",
            sheet: &["body { height: 200px; overflow: hidden; } p { height: 20px; }"],
            build: |dom| {
                let body = body_scaffold(dom);
                let mut ids = vec![body];
                for _ in 0..3 {
                    let p = dom.create_element(html("p"));
                    dom.append_child(body, p);
                    ids.push(p);
                }
                ids
            },
            mutate: |dom, ids| dom.remove_child(ids[2]),
        },
        Case {
            name: "text change inside a sized <p>",
            sheet: &["body { height: 200px; overflow: hidden; } p { height: 40px; }"],
            build: |dom| {
                let body = body_scaffold(dom);
                let p = dom.create_element(html("p"));
                dom.append_child(body, p);
                let t = text_child(dom, p, "before");
                vec![body, p, t]
            },
            mutate: |dom, ids| dom.set_text(ids[2], "after the change"),
        },
        // The inline-box table's own splice path. An inline-block establishes no
        // Taffy box, so its geometry lives only in `FragmentPlane::inline_boxes`;
        // relabelling one resizes that entry and nothing else.
        Case {
            name: "inline-block button label change",
            sheet: &["body { height: 200px; overflow: hidden; } \
                 div { height: 60px; overflow: hidden; } \
                 button { padding: 4px 8px; border: 0; }"],
            build: |dom| {
                let body = body_scaffold(dom);
                let row = dom.create_element(html("div"));
                dom.append_child(body, row);
                let b = dom.create_element(html("button"));
                dom.append_child(row, b);
                let t = text_child(dom, b, "Go");
                vec![body, row, b, t]
            },
            mutate: |dom, ids| dom.set_text(ids[3], "Go a good deal further"),
        },
        // A second inline-block appears mid-line: a NEW inline-box entry, which is
        // the case a splice that only rewrites existing entries would miss.
        Case {
            name: "append a second inline-block button",
            sheet: &["body { height: 200px; overflow: hidden; } \
                 div { height: 60px; overflow: hidden; } \
                 button { padding: 4px 8px; border: 0; }"],
            build: |dom| {
                let body = body_scaffold(dom);
                let row = dom.create_element(html("div"));
                dom.append_child(body, row);
                let b = dom.create_element(html("button"));
                dom.append_child(row, b);
                text_child(dom, b, "Alpha");
                vec![body, row]
            },
            mutate: |dom, ids| {
                let b = dom.create_element(html("button"));
                dom.append_child(ids[1], b);
                let t = dom.create_text("Bravo");
                dom.append_child(b, t);
            },
        },
        // The non-atomic inline lane: an `<a>`'s entry is the union of its line
        // boxes, so lengthening its text moves it.
        Case {
            name: "inline anchor text change",
            sheet: &["body { height: 200px; overflow: hidden; } p { height: 40px; }"],
            build: |dom| {
                let body = body_scaffold(dom);
                let p = dom.create_element(html("p"));
                dom.append_child(body, p);
                text_child(dom, p, "see ");
                let a = dom.create_element(html("a"));
                dom.set_attribute(a, attr("href"), "https://example.test/spec");
                dom.append_child(p, a);
                let t = text_child(dom, a, "here");
                vec![body, p, a, t]
            },
            mutate: |dom, ids| dom.set_text(ids[3], "the specification itself"),
        },
        // An attribute-driven restyle that changes geometry, taking the
        // `Restyled` path rather than the structural splice. Included so the
        // corpus covers the other incremental lane into the same plane.
        Case {
            name: "class change resizes a sized child",
            sheet: &["body { height: 200px; overflow: hidden; } \
                 p { height: 20px; } p.wide { width: 300px; }"],
            build: |dom| {
                let body = body_scaffold(dom);
                let p = dom.create_element(html("p"));
                dom.append_child(body, p);
                vec![body, p]
            },
            mutate: |dom, ids| dom.set_attribute(ids[1], attr("class"), "wide"),
        },
    ]
}

/// Run each case down both paths and require the planes to agree.
#[test]
fn splice_refines_full_relayout() {
    let cases = corpus();
    let total = cases.len();
    let mut spliced_count = 0usize;
    let mut report = Vec::new();
    let mut failures = Vec::new();

    for case in cases {
        let mut dom = ScriptedDom::new();
        let ids = (case.build)(&mut dom);
        let mut incremental = IncrementalLayout::new(&dom, case.sheet, W, H);
        let _ = drain(&mut dom);

        (case.mutate)(&mut dom, &ids);
        let muts = drain(&mut dom);
        let applied = incremental.apply(&dom, case.sheet, &muts);
        if applied == Applied::Spliced {
            spliced_count += 1;
        }

        // The reference: what a fresh layout of the mutated DOM produces. A
        // stronger oracle than re-running `full_layout` over the retained
        // cascade, since it also catches a stale incremental restyle.
        let reference = IncrementalLayout::new(&dom, case.sheet, W, H);

        let diffs = differences(incremental.fragments(), reference.fragments());
        report.push(format!("  {:<40} {applied:?}", case.name));
        if !diffs.is_empty() {
            failures.push(format!(
                "{} ({applied:?}):\n    {}",
                case.name,
                diffs.join("\n    ")
            ));
        }
    }

    eprintln!("splice differential: {total} cases");
    for line in &report {
        eprintln!("{line}");
    }
    eprintln!("splice path taken by {spliced_count}/{total}");

    assert!(
        failures.is_empty(),
        "the spliced plane must be indistinguishable from a full relayout:\n{}",
        failures.join("\n")
    );
    // Yield floor. A bailed case runs the same code down both arms, so a corpus
    // that stopped reaching the splice would pass while testing nothing. This is
    // the assertion that makes such a regression visible.
    assert!(
        spliced_count >= 3,
        "only {spliced_count}/{total} cases reached the splice path; the corpus is no longer \
         exercising it (bail reasons are on the `genet_layout::splice` tracing target)"
    );
}

/// The comparator must be able to fail.
///
/// Feed it a plane that is knowingly stale — the pre-mutation plane against a
/// post-mutation reference — and require complaints. Without this, every
/// assertion above could be vacuous and the suite would still be green. Same
/// discipline as the deliberate bad-trace fixture E4 checks TLC against.
#[test]
fn the_comparator_detects_a_stale_plane() {
    const SHEET: &[&str] = &["body { height: 200px; overflow: hidden; } p { height: 20px; }"];
    let mut dom = ScriptedDom::new();
    let body = body_scaffold(&mut dom);
    let p = dom.create_element(html("p"));
    dom.append_child(body, p);

    let stale = IncrementalLayout::new(&dom, SHEET, W, H);
    let _ = drain(&mut dom);

    // A second `<p>`, which a full relayout sees and the stale plane cannot.
    let added = dom.create_element(html("p"));
    dom.append_child(body, added);
    let _ = drain(&mut dom);
    let reference = IncrementalLayout::new(&dom, SHEET, W, H);

    let diffs = differences(stale.fragments(), reference.fragments());
    assert!(
        !diffs.is_empty(),
        "the comparator reported no difference between a stale plane and a fresh one, \
         so a green differential run would prove nothing"
    );
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("missing from the spliced plane")),
        "the missing second <p> should be named specifically, got {diffs:?}"
    );
}
