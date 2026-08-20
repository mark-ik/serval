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
#[cfg(not(target_os = "macos"))]
use cambium_genet_winit_host::WindowCommands;
use cambium_genet_winit_host::{
    AppCtx, AppFrameInsets, Frame, HostHooks, HostOptions, HostPointer, Init, Runner,
    WindowCommand, WindowFrame, read_frame, run,
};
use genet_probe::{Automatable, Driveable, ProbeSnapshot, ProbeSurface, Progress, Scenario};

// ------------------------------------------------------------------ state

#[derive(Default)]
struct Smoke {
    /// The window-verb seam. One field, not four bools.
    #[cfg(not(target_os = "macos"))]
    window: WindowCommands,
    clicks: usize,
    /// 0..=1, driven by dragging the rail.
    level: f32,
    /// Wheel notches the panel absorbed.
    notches: i32,
    /// Semantic transitions the scenario can assert on.
    events: Vec<String>,
    /// The same frame policy used to create the native window. The view reads
    /// it too, which prevents a host frame and an app title bar appearing
    /// together.
    window_frame: WindowFrame,
    /// Transparent outer margin used by the X11 shadow receipt.
    app_frame_inset: u32,
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
/// system menu — none of which this file implements. Windows/Linux add the
/// product-drawn caption buttons; macOS keeps AppKit's traffic lights and the
/// same stylesheet uses `--titlebar-area-*` to place only the title beside them.
#[cfg(not(target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
    let children: Vec<Child> = vec![Box::new(
        el("div", text("host smoke")).attr("class", "caption-title"),
    )];
    #[cfg(not(target_os = "macos"))]
    let mut children: Vec<Child> = vec![Box::new(
        el("div", text("host smoke")).attr("class", "caption-title"),
    )];
    #[cfg(not(target_os = "macos"))]
    children.extend([
        caption("–", "Minimize", WindowCommands::minimize),
        caption("□", "Maximize", WindowCommands::toggle_maximize),
        caption("×", "Close", WindowCommands::close),
    ]);
    Box::new(el("div", children).attr("class", "bar"))
}

fn root(state: &Smoke) -> Child {
    let filled = (state.level * 240.0).round() as i32;
    let content: Child = Box::new(
        el(
            "div",
            (
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
        .attr("class", "content"),
    );
    let mut children = Vec::with_capacity(2);
    if state.window_frame == WindowFrame::App {
        children.push(title_bar());
    }
    children.push(content);
    let frame = el("div", children).attr("class", "frame");
    let frame = if state.app_frame_inset > 0 {
        let inset = state.app_frame_inset;
        frame.attr(
            "style",
            format!("margin:{inset}px; box-shadow:0 4px 12px rgba(0,0,0,0.55); border-radius:7px;"),
        )
    } else {
        frame
    };
    Box::new(frame)
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
.bar { --app-region: drag; display: flex; margin-left: var(--titlebar-area-x, 0px); margin-top: var(--titlebar-area-y, 0px); width: var(--titlebar-area-width, 100%); height: 32px; background: #1d2733; }
.caption-title { flex-grow: 1; color: #9fb0c4; padding: 6px 8px; }
.caption { --app-region: no-drag; width: 32px; background: #29486b; color: #f0ebdd; }
.caption:hover { background: #3a5d85; }
.frame { font-size: 16px; background: #14181f; color: #f0ebdd; }
.content { padding: 24px; }
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
    /// Frames captured this run: name, geometry, digest, blankness and alpha.
    captures: Vec<(String, u32, u32, u64, bool, u8, u8, usize, usize)>,
    /// Optional directory for exact frame artifacts. The textual receipt keeps
    /// compact digests; a headed geometry receipt also needs inspectable pixels.
    capture_dir: Option<std::path::PathBuf>,
    capture_errors: Vec<String>,
    /// A shadow receipt requires both clear margin and blurred shadow pixels.
    require_alpha: bool,
    /// Optional file an external platform probe creates when it has finished
    /// observing this exact window. This keeps a headed receipt bounded without
    /// guessing how many frames native inspection will take.
    release_file: Option<std::path::PathBuf>,
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
            .with_field(
                "window_frame",
                match state.window_frame {
                    WindowFrame::Host => "host",
                    WindowFrame::App => "app",
                },
            )
            .with_field("app_frame_inset", state.app_frame_inset.to_string())
            .with_field("captures", self.lane.captures.len().to_string())
            .with_field(
                "maximized",
                self.ctx
                    .geometry
                    .map(|geometry| geometry.maximized.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            )
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
        Some(
            self.lane
                .release_file
                .as_ref()
                .is_some_and(|path| !path.exists()),
        )
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
                // Resizing goes through the window-verb queue, like every
                // other window verb, rather than through a native handle.
                self.ctx.window_commands.push(WindowCommand::Resize(w, h));
                Ok(())
            },
            Some("await-release") => {
                let path = std::env::var_os("HOST_SMOKE_RELEASE_FILE")
                    .map(std::path::PathBuf::from)
                    .ok_or("await-release wants HOST_SMOKE_RELEASE_FILE")?;
                self.lane.release_file = Some(path);
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
                if let Some(dir) = self.capture_dir.as_ref() {
                    let safe_name: String = name
                        .chars()
                        .map(|ch| {
                            if ch.is_ascii_alphanumeric() || ch == '-' {
                                ch
                            } else {
                                '_'
                            }
                        })
                        .collect();
                    let path = dir.join(format!("{safe_name}.bmp"));
                    if let Err(error) = write_bmp(&path, &frame) {
                        self.capture_errors
                            .push(format!("could not write {}: {error}", path.display()));
                    }
                }
                let alpha_min = frame
                    .rgba
                    .chunks_exact(4)
                    .map(|pixel| pixel[3])
                    .min()
                    .unwrap_or(0);
                let alpha_max = frame
                    .rgba
                    .chunks_exact(4)
                    .map(|pixel| pixel[3])
                    .max()
                    .unwrap_or(0);
                let transparent = frame
                    .rgba
                    .chunks_exact(4)
                    .filter(|pixel| pixel[3] == 0)
                    .count();
                let translucent = frame
                    .rgba
                    .chunks_exact(4)
                    .filter(|pixel| (1..=254).contains(&pixel[3]))
                    .count();
                self.captures.push((
                    name,
                    frame.width,
                    frame.height,
                    digest,
                    blank,
                    alpha_min,
                    alpha_max,
                    transparent,
                    translucent,
                ));
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
        let alpha_ok = !self.require_alpha
            || self
                .captures
                .iter()
                .all(|capture| capture.7 > 0 && capture.8 > 0);
        let frames_ok = blanks == 0
            && distinct.len() > 1
            && sizes.len() > 1
            && alpha_ok
            && self.capture_errors.is_empty();
        let ok = outcome.ok && frames_ok;

        let result = if ok { "RESULT ok" } else { "RESULT fail" };
        let mut body = vec![result.to_string()];
        body.extend(outcome.log.iter().cloned());
        for (name, w, h, digest, blank, alpha_min, alpha_max, transparent, translucent) in
            &self.captures
        {
            body.push(format!(
                "capture {name} {w}x{h} digest={digest:016x} alpha={alpha_min}..{alpha_max} transparent={transparent} translucent={translucent}{}",
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
                 must change size across the resize, and requested artifacts must be written"
                    .to_string(),
            );
        }
        if !alpha_ok {
            body.push(
                "FAIL: a frame-shadow capture needs transparent margin pixels and translucent shadow pixels"
                    .to_string(),
            );
        }
        for error in &self.capture_errors {
            body.push(format!("FAIL: {error}"));
        }
        let text = body.join("\n");
        eprintln!("[host-smoke] {text}");
        if let Some(path) = self.receipt.as_ref() {
            let _ = std::fs::write(path, format!("{text}\n"));
        }
    }
}

/// Write an inspectable 32-bit BMP without pulling an image codec into the
/// host example. Rows are bottom-up BGRA, the ordinary uncompressed BMP form.
fn write_bmp(path: &std::path::Path, frame: &Frame) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "capture path has no parent",
        )
    })?)?;
    let pixel_bytes = frame
        .width
        .checked_mul(frame.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| std::io::Error::other("capture dimensions overflow BMP size"))?;
    let file_size = 54_u32
        .checked_add(pixel_bytes)
        .ok_or_else(|| std::io::Error::other("capture is too large for BMP"))?;
    let width = i32::try_from(frame.width)
        .map_err(|_| std::io::Error::other("capture width does not fit BMP"))?;
    let height = i32::try_from(frame.height)
        .map_err(|_| std::io::Error::other("capture height does not fit BMP"))?;

    let mut bytes = Vec::with_capacity(file_size as usize);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&file_size.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54_u32.to_le_bytes());
    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&pixel_bytes.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    for row in (0..frame.height as usize).rev() {
        let start = row * frame.width as usize * 4;
        let end = start + frame.width as usize * 4;
        for rgba in frame.rgba[start..end].chunks_exact(4) {
            bytes.extend_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
        }
    }
    std::fs::write(path, bytes)
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
    let window_frame = match std::env::var("HOST_SMOKE_WINDOW_FRAME") {
        Ok(value) if value.eq_ignore_ascii_case("host") => WindowFrame::Host,
        Ok(value) if value.eq_ignore_ascii_case("app") => WindowFrame::App,
        Ok(value) => panic!("HOST_SMOKE_WINDOW_FRAME must be 'host' or 'app', got {value:?}"),
        Err(std::env::VarError::NotPresent) => WindowFrame::App,
        Err(error) => panic!("HOST_SMOKE_WINDOW_FRAME is not valid Unicode: {error}"),
    };
    let app_frame_inset = match std::env::var("HOST_SMOKE_APP_FRAME_INSET") {
        Ok(value) => value.parse::<u32>().unwrap_or_else(|_| {
            panic!("HOST_SMOKE_APP_FRAME_INSET must be an unsigned integer, got {value:?}")
        }),
        Err(std::env::VarError::NotPresent) => 0,
        Err(error) => panic!("HOST_SMOKE_APP_FRAME_INSET is not valid Unicode: {error}"),
    };
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
                capture_dir: std::env::var("HOST_SMOKE_CAPTURE_DIR").ok().map(Into::into),
                capture_errors: Vec::new(),
                require_alpha: app_frame_inset > 0,
                release_file: None,
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
        // The view reads this same value before deciding whether to include its
        // title row, so the two frame providers cannot overlap.
        window_frame,
        app_frame_insets: AppFrameInsets::uniform(app_frame_inset),
        ..Default::default()
    };
    run(
        options,
        // The application takes its end of the window-verb seam and keeps it
        // in state; the caption buttons call it from ordinary handlers.
        move |_window, _commands, _wake| Init {
            state: Smoke {
                #[cfg(not(target_os = "macos"))]
                window: _commands.clone(),
                window_frame,
                app_frame_inset,
                ..Smoke::default()
            },
            logic: root as Logic,
            sheet: SHEET.to_string(),
        },
        hooks,
    )
    .expect("event loop");
}
