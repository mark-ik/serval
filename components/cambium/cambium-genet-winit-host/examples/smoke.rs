//! Headed smoke: the host's whole input surface, driven semantically.
//!
//! Run it by hand to poke at it:
//!
//! ```text
//! cargo run -p cambium-genet-winit-host --example smoke
//! ```
//!
//! Or run it bounded, self-driving, and leaving a receipt — the form CI and a
//! headed-verify pass want:
//!
//! ```text
//! HOST_SMOKE_SCENARIO=components/cambium/cambium-genet-winit-host/examples/smoke.scn \
//! HOST_SMOKE_RECEIPT=smoke.receipt \
//!   cargo run -p cambium-genet-winit-host --example smoke
//! ```
//!
//! The scenario drives by **role and label**, not by coordinates, and every
//! pointer event it produces goes back through the host's own routing via
//! [`HostPointer`] — so what the receipt exercised is the shipping code path.
//! Captures are in-process readbacks of the frame that was presented, recorded
//! as size + digest, which is how the receipt can claim a frame was really
//! drawn (and, after a resize, really redrawn at the new size).
//!
//! This file is also the reference for how an application wires a scenario
//! through the host: the `Probe` borrow-struct below is the whole trick.

use std::cell::RefCell;
use std::rc::Rc;

use cambium::{
    AnyView, GenetCtx, GenetElement, PointerEvent, PointerPhase, WheelEvent, clickable, el,
    focusable, on_pointer, on_wheel, text,
};
use cambium_genet_winit_host::{
    AppCtx, Frame, HostHooks, HostOptions, HostPointer, Init, Runner, WindowCommands, read_frame,
    run,
};
use genet_probe::{Automatable, Driveable, ProbeSnapshot, ProbeSurface, Progress, Scenario};

// ------------------------------------------------------------------ state

#[derive(Default)]
struct Smoke {
    /// The window-verb seam. One field, not four bools.
    window: WindowCommands,
    clicks: usize,
    /// 0..=1, driven by dragging the rail.
    level: f32,
    /// Wheel notches the panel absorbed.
    notches: i32,
    /// Semantic transitions the scenario can assert on.
    events: Vec<String>,
}

impl Smoke {
    fn note(&mut self, event: String) {
        self.events.push(event);
    }
}

type Child = Box<dyn AnyView<Smoke, (), GenetCtx, GenetElement>>;
type Logic = fn(&Smoke) -> Child;

/// The client-side title bar.
///
/// The whole bar is `--app-region: drag` (see the sheet), so pressing it moves
/// the window, double-clicking it maximizes, and right-clicking raises the
/// system menu — none of which this file implements. The caption buttons
/// declare `--app-region: no-drag` to carve themselves out, and each carries a
/// role and an accessible name, because a hand-drawn frame is invisible to a
/// screen reader unless the application says otherwise.
fn caption(label: &'static str, name: &'static str, verb: fn(&WindowCommands)) -> Child {
    Box::new(focusable(clickable(
        el("button", text(label))
            .attr("class", "caption")
            .attr("aria-label", name),
        move |s: &mut Smoke, _| {
            verb(&s.window);
            s.note(format!("window {name}"));
        },
    )))
}

fn title_bar() -> Child {
    Box::new(
        el(
            "div",
            (
                el("div", text("host smoke")).attr("class", "caption-title"),
                caption("–", "Minimize", WindowCommands::minimize),
                caption("□", "Maximize", WindowCommands::toggle_maximize),
                caption("×", "Close", WindowCommands::close),
            ),
        )
        .attr("class", "bar"),
    )
}

fn root(state: &Smoke) -> Child {
    let filled = (state.level * 240.0).round() as i32;
    Box::new(
        el(
            "div",
            (
                title_bar(),
                el("div", text("cambium-genet-winit-host smoke")).attr("class", "title"),
                focusable(clickable(
                    el("button", text(format!("clicked {} times", state.clicks)))
                        .attr("class", "button"),
                    |s: &mut Smoke, _| {
                        s.clicks += 1;
                        let n = s.clicks;
                        s.note(format!("clicks {n}"));
                    },
                )),
                focusable(clickable(
                    el("button", text("Reset")).attr("class", "button"),
                    |s: &mut Smoke, _| {
                        s.clicks = 0;
                        s.level = 0.0;
                        s.note("reset".into());
                    },
                )),
                // A drag rail: the receipt that Down/Move/Up carry the
                // captured element's own coordinates. The handler normalizes
                // with nothing but `local` and `size`.
                on_pointer(
                    el(
                        "div",
                        el("div", ())
                            .attr("class", "rail-fill")
                            .attr("style", format!("width:{filled}px;")),
                    )
                    .attr("class", "rail")
                    .attr("role", "slider")
                    .attr("aria-label", "Level"),
                    |s: &mut Smoke, e: PointerEvent| {
                        if e.size.0 > 0.0 && !matches!(e.phase, PointerPhase::Up) {
                            let level = (e.local.0 / e.size.0).clamp(0.0, 1.0);
                            if (level - s.level).abs() > 0.004 {
                                s.level = level;
                                let pct = (level * 100.0).round() as i32;
                                s.note(format!("level {pct}"));
                            }
                        }
                    },
                ),
                // A wheel panel: the receipt that a handler sees the notch
                // before the layout scrolls, and can keep it.
                on_wheel(
                    el("div", text(format!("wheel notches: {}", state.notches)))
                        .attr("class", "panel")
                        .attr("aria-label", "Wheel panel"),
                    |s: &mut Smoke, e: WheelEvent| {
                        s.notches += e.delta.1.signum() as i32;
                        let n = s.notches;
                        s.note(format!("notches {n}"));
                        // Keep the notch: the page behind must not also scroll.
                        e.prevent_default();
                    },
                ),
            ),
        )
        .attr("class", "frame"),
    )
}

// The controls keep genet's UA `display: inline-block`, the standards-correct
// display for a form control. They used to need `display: block`: an inline-level
// box got no fragment of its own, so neither `painted_rect` nor a `genet-probe`
// selector could locate one, and an app had to style its controls to suit the
// driver. The engine now reads a rect back per inline box, so the scenario below
// drives these buttons by role and label with nothing arranged for it.
//
// Their `padding` / `margin` do not show yet: genet measures an inline-block from
// its content and any definite width/height, so the rest of the box model has
// still to reach the atomic-inline path. That is a sizing gap, not a reachability
// one — the rect the driver gets is the rect that paints.
const SHEET: &str = "
.bar { --app-region: drag; background: #1d2733; padding: 6px 8px; margin-bottom: 12px; }
.caption-title { color: #9fb0c4; }
.caption { --app-region: no-drag; width: 32px; background: #29486b; color: #f0ebdd; }
.caption:hover { background: #3a5d85; }
.frame { padding: 24px; font-size: 16px; background: #14181f; color: #f0ebdd; }
.title { margin-bottom: 12px; }
.button { padding: 8px 12px; margin-bottom: 8px; background: #29486b; color: #f0ebdd; width: 240px; }
.button:hover { background: #3a5d85; }
.button:focus { background: #4c76a4; }
.rail { width: 240px; height: 20px; background: #22303f; margin-bottom: 12px; }
.rail-fill { height: 20px; background: #7fb4e8; }
.panel { width: 240px; height: 60px; background: #1d2733; padding: 8px; }
";

// -------------------------------------------------------------- the probe

/// What the scenario lane owns between frames. The application state itself
/// lives in the runner, which only a hook can reach, so this holds the rest.
struct Lane {
    /// Moved out for the duration of a tick, so the driver can hold the rest of
    /// the lane while it runs.
    scenario: Option<Scenario>,
    receipt: Option<std::path::PathBuf>,
    /// Frames captured this run: `(name, width, height, digest, blank)`.
    captures: Vec<(String, u32, u32, u64, bool)>,
    finished: bool,
}

/// The `Automatable` view of the app, borrowed for the duration of one tick.
///
/// This is the pattern: the host owns the runner, so the application cannot
/// hold a long-lived `&mut` to it. It borrows the hook's context instead, for
/// exactly as long as the driver needs, and queues pointer delivery back
/// through the host rather than re-implementing hit testing.
struct Probe<'a, 'c> {
    ctx: &'a mut AppCtx<'c, Smoke, Logic, Child>,
    lane: &'a mut Lane,
}

impl Automatable for Probe<'_, '_> {
    fn with_surfaces<R>(&self, f: impl FnOnce(&[ProbeSurface<'_>]) -> R) -> R {
        let dom = self.ctx.runner.dom();
        let dom_ref = dom.borrow();
        let (w, h) = self.ctx.logical_size;
        f(&[ProbeSurface {
            name: "smoke",
            dom: &dom_ref,
            rect: [0.0, 0.0, w, h],
            sheet: SHEET,
        }])
    }

    fn snapshot(&self) -> ProbeSnapshot {
        let state = self.ctx.runner.state();
        ProbeSnapshot::default()
            .with_field("clicks", state.clicks.to_string())
            .with_field("level", ((state.level * 100.0).round() as i32).to_string())
            .with_field("notches", state.notches.to_string())
            .with_field("captures", self.lane.captures.len().to_string())
    }

    fn drain_events(&mut self) -> Vec<String> {
        let mut drained = Vec::new();
        self.ctx
            .runner
            .update(|s| drained = std::mem::take(&mut s.events));
        drained
    }

    fn act(&mut self, label: &str) -> bool {
        match label {
            "reset" => {
                self.ctx.runner.update(|s| {
                    s.clicks = 0;
                    s.level = 0.0;
                    s.note("reset".into());
                });
                true
            },
            _ => false,
        }
    }

    fn press(&mut self, x: f32, y: f32) {
        self.ctx.pointer.push(HostPointer::Press(x, y));
    }

    fn moved(&mut self, x: f32, y: f32) {
        self.ctx.pointer.push(HostPointer::Moved(x, y));
    }

    fn release(&mut self, x: f32, y: f32) {
        self.ctx.pointer.push(HostPointer::Release(x, y));
    }

    fn busy(&mut self) -> Option<bool> {
        // Nothing asynchronous in this example: a step's effect is in the
        // state by the time the next frame lays out.
        Some(false)
    }
}

impl Driveable for Probe<'_, '_> {
    /// One app verb: `resize <w> <h>`, in logical px. The host owns the window,
    /// so a resize receipt has to be asked for through it rather than faked.
    fn app_step(&mut self, line: &str) -> Result<(), String> {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("resize") => {
                let w: f64 = parts
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("resize wants a width")?;
                let h: f64 = parts
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("resize wants a height")?;
                // Asking the window manager to resize is desktop-only, so it
                // goes through the native handle rather than the neutral seam.
                let native = self.ctx.native_window.ok_or("resize needs a window")?;
                let _ = native.request_inner_size(winit::dpi::LogicalSize::new(w, h));
                native.request_redraw();
                Ok(())
            },
            _ => Err(format!("unknown verb: {line}")),
        }
    }

    fn capture(&mut self, name: &str) -> bool {
        let name = name.to_string();
        let sink = Rc::new(RefCell::new(None::<Frame>));
        let out = sink.clone();
        *self.ctx.capture = Some(Box::new(move |surface, view, w, h| {
            *out.borrow_mut() = read_frame(surface, view, w, h);
        }));
        // The capture runs inside the next frame, while the rasterized view is
        // still alive; record it on the frame after that.
        self.lane.pending_capture(name, sink);
        true
    }
}

impl Lane {
    fn pending_capture(&mut self, name: String, sink: Rc<RefCell<Option<Frame>>>) {
        PENDING.with(|p| *p.borrow_mut() = Some((name, sink)));
    }

    /// Fold in whatever the last armed capture produced.
    fn collect_capture(&mut self) {
        let taken = PENDING.with(|p| p.borrow_mut().take());
        let Some((name, sink)) = taken else { return };
        let frame = sink.borrow_mut().take();
        match frame {
            Some(frame) => {
                let digest = frame.digest();
                let blank = frame.is_blank();
                self.captures
                    .push((name, frame.width, frame.height, digest, blank));
            },
            // Not yet presented: put it back and try again next frame.
            None => PENDING.with(|p| *p.borrow_mut() = Some((name, sink))),
        }
    }

    fn write_receipt(&mut self, outcome: genet_probe::Outcome) {
        if self.finished {
            return;
        }
        self.finished = true;
        // Two claims a scenario's own grammar cannot make, checked here because
        // a receipt that says "ok" while every frame was blank or identical is
        // worse than no receipt: at least one frame must have real pixels, and
        // the frames captured around a state change must actually differ.
        let blanks = self.captures.iter().filter(|c| c.4).count();
        let distinct: std::collections::BTreeSet<u64> = self.captures.iter().map(|c| c.3).collect();
        let sizes: std::collections::BTreeSet<(u32, u32)> =
            self.captures.iter().map(|c| (c.1, c.2)).collect();
        let frames_ok = blanks == 0 && distinct.len() > 1 && sizes.len() > 1;
        let ok = outcome.ok && frames_ok;

        let result = if ok { "RESULT ok" } else { "RESULT fail" };
        let mut body = vec![result.to_string()];
        body.extend(outcome.log.iter().cloned());
        for (name, w, h, digest, blank) in &self.captures {
            body.push(format!(
                "capture {name} {w}x{h} digest={digest:016x}{}",
                if *blank { " BLANK" } else { "" }
            ));
        }
        body.push(format!(
            "frames: {} captured, {} blank, {} distinct digests, {} distinct sizes",
            self.captures.len(),
            blanks,
            distinct.len(),
            sizes.len(),
        ));
        if !frames_ok {
            body.push(
                "FAIL: frames must be non-blank, must differ across a state change, \
                 and must change size across the resize"
                    .to_string(),
            );
        }
        let text = body.join("\n");
        eprintln!("[host-smoke] {text}");
        if let Some(path) = self.receipt.as_ref() {
            let _ = std::fs::write(path, format!("{text}\n"));
        }
    }
}

thread_local! {
    /// The capture armed for the next presented frame. A thread-local because
    /// the capture closure outlives the tick that armed it, and the host runs
    /// the whole application on one thread.
    static PENDING: RefCell<Option<(String, Rc<RefCell<Option<Frame>>>)>> =
        const { RefCell::new(None) };
}

// --------------------------------------------------------------- wiring

fn main() {
    let lane = Rc::new(RefCell::new(std::env::var("HOST_SMOKE_SCENARIO").ok().map(
        |path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("scenario '{path}' unreadable: {e}"));
            let scenario = Scenario::parse(&text)
                .unwrap_or_else(|e| panic!("scenario '{path}' rejected: {e}"));
            eprintln!("[host-smoke] scenario armed: {path}");
            Lane {
                scenario: Some(scenario),
                receipt: std::env::var("HOST_SMOKE_RECEIPT").ok().map(Into::into),
                captures: Vec::new(),
                finished: false,
            }
        },
    )));

    let after_frame_lane = lane.clone();
    let hooks: HostHooks<Smoke, Logic, Child> = HostHooks {
        frame: Box::new(|_ctx| false),
        after_dispatch: Box::new(|_ctx| {}),
        after_frame: Box::new(move |ctx: &mut AppCtx<'_, Smoke, Logic, Child>| {
            let mut borrowed = after_frame_lane.borrow_mut();
            let Some(lane) = borrowed.as_mut() else {
                return;
            };
            lane.collect_capture();
            // The scenario moves out for the tick, so the driver can hold the
            // rest of the lane (the capture log) while it runs.
            let Some(mut scenario) = lane.scenario.take() else {
                return;
            };
            let progress = {
                let mut probe = Probe { ctx, lane };
                scenario.tick(&mut probe)
            };
            // Finish only once every armed capture has actually landed, or the
            // receipt would claim a frame it never read back.
            if progress == Progress::Done && PENDING.with(|p| p.borrow().is_none()) {
                lane.write_receipt(scenario.finish());
                *ctx.close = true;
            }
            lane.scenario = Some(scenario);
            // A scenario run must keep frames coming: every step is pumped by
            // one, and an idle app would stall the run rather than finish it.
            if let Some(window) = ctx.window {
                window.request_redraw();
            }
        }),
        after_wake: Box::new(|_ctx| {}),
        close_request: Box::new(|_ctx, _request| cambium_genet_winit_host::CloseDisposition::Exit),
        focused_text: Box::new(|_runner: &Runner<Smoke, Logic, Child>| None),
        key_intercept: Box::new(|_runner, _press| false),
    };

    let options = HostOptions {
        title: "host smoke".into(),
        initial_logical_size: (420.0, 320.0),
        size_env: Some(("HOST_SMOKE_WIDTH".into(), "HOST_SMOKE_HEIGHT".into())),
        // Client-side decorations: the frame below is ours, so the OS draws
        // none. The host supplies edge resize and its cursors.
        decorations: false,
        ..Default::default()
    };
    run(
        options,
        // The application takes its end of the window-verb seam and keeps it
        // in state; the caption buttons call it from ordinary handlers.
        |_window, commands, _wake| Init {
            state: Smoke {
                window: commands.clone(),
                ..Smoke::default()
            },
            logic: root as Logic,
            sheet: SHEET.to_string(),
        },
        hooks,
    )
    .expect("event loop");
}
