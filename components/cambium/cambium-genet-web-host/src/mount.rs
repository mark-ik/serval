// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Putting an application on a canvas, and keeping it fed.
//!
//! The browser's answer to the winit source's `run`. It differs in shape for
//! one reason worth stating: `run` owns the thread until the window closes,
//! and nothing may own a browser's thread. So this returns as soon as the
//! application is mounted, and the listeners and frame callback it installed
//! keep it alive.

use std::cell::RefCell;
use std::rc::Rc;

use cambium_rootstock::{
    Host, HostHooks, HostOptions, HostState, HostWake, HostWindow, Init, Runner, ScriptedDom,
    meristem_bounds::RootView,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::HtmlCanvasElement;

use crate::a11y::DomAccessibility;
use crate::input::{
    CompositionKind, composition_from_dom, key_press_from_dom, wheel_delta_from_dom,
};
use crate::surface::{WebSurface, WebWindow};
use crate::{WHEEL_LINE_PX, WHEEL_PAGE_PX};

/// A mounted application, kept alive by the closures it installed.
///
/// Dropping this does not stop it: the listeners hold their own references,
/// which is how a browser application stays running with nothing on the stack.
pub struct Mounted<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    host: Rc<RefCell<Host<State, Logic, V>>>,
    window: WebWindow,
}

impl<State, Logic, V> Mounted<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// The host, for a caller that wants to drive it directly.
    pub fn host(&self) -> &Rc<RefCell<Host<State, Logic, V>>> {
        &self.host
    }

    /// The canvas frame.
    pub fn window(&self) -> &WebWindow {
        &self.window
    }
}

/// Mount an application onto a canvas.
///
/// Mirrors what the winit source does on `resumed`: boot a surface, run the
/// application's init against the frame it will present on, build the runner,
/// attach accessibility, then draw once so the first frame exists before any
/// event arrives.
pub async fn mount<State, Logic, V>(
    canvas: HtmlCanvasElement,
    options: HostOptions,
    init: impl FnOnce(
        &dyn HostWindow,
        &cambium_rootstock::WindowCommands,
        &HostWake,
    ) -> Init<State, Logic>,
    hooks: HostHooks<State, Logic, V>,
) -> Result<Mounted<State, Logic, V>, String>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    let window = WebWindow::new(canvas.clone());
    let (width, height) = window.physical_size();
    // The backing store is sized in physical pixels so a fragment lands on a
    // device pixel, the same contract a desktop surface is configured under.
    canvas.set_width(width);
    canvas.set_height(height);

    let surface = WebSurface::boot(canvas.clone(), width, height, (options.netrender)()).await?;
    let label = options.title.clone();
    // The wake flag the host reads, and the signal that gets a frame drawn.
    // Both are atomics: the host's wake handle is `Send + Sync` because a
    // desktop worker thread may wake the UI, and that bound holds regardless
    // of how many threads a browser has.
    let wake_pending = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wake = HostWake::new(wake_pending.clone(), {
        let pending = window.pending_frame_flag();
        std::sync::Arc::new(move || pending.store(true, std::sync::atomic::Ordering::Relaxed))
    });

    let mut s = HostState::new();
    s.wake_pending = wake_pending;
    let Init {
        state,
        logic,
        sheet,
    } = init(&window, &s.commands.clone(), &wake);
    let dom = Rc::new(RefCell::new(ScriptedDom::new()));

    s.sheet = sheet;
    s.runner = Some(Runner::new(dom, logic, state));
    s.a11y = Some(Box::new(DomAccessibility::new(canvas.clone(), label)));
    s.window = Some(Box::new(window.clone()));
    s.surface = Some(Box::new(surface));

    let host = Rc::new(RefCell::new(Host::new(options, None, hooks, s, wake)));

    // The first frame, before any event: a canvas that has never painted shows
    // whatever was behind it.
    host.borrow_mut().redraw();

    install_listeners(&canvas, &host, &window)?;
    schedule_frames(&host, &window)?;

    Ok(Mounted { host, window })
}

/// The frame loop.
///
/// `requestAnimationFrame` unconditionally, but a frame is only *drawn* when
/// one was asked for. That keeps a static application at zero GPU work while
/// still noticing a resize, which the desktop gets from the window manager and
/// the browser must observe for itself.
fn schedule_frames<State, Logic, V>(
    host: &Rc<RefCell<Host<State, Logic, V>>>,
    window: &WebWindow,
) -> Result<(), String>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    let browser = web_sys::window().ok_or("no window")?;
    let frame: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let next = frame.clone();
    let host = host.clone();
    let window = window.clone();
    let mut last_size = window.physical_size();

    *next.borrow_mut() = Some(Closure::new(move |_: f64| {
        // A CSS-driven resize has no event of its own that also covers zoom
        // and device-pixel-ratio changes, so the frame loop watches the one
        // number that captures all three.
        let size = window.physical_size();
        if size != last_size {
            last_size = size;
            window.canvas().set_width(size.0);
            window.canvas().set_height(size.1);
            let mut h = host.borrow_mut();
            if let Some(surface) = h.s.surface.as_mut() {
                surface.resize(size.0, size.1);
            }
            let (lw, lh) = h.logical_size();
            h.relayout(lw, lh);
            window.request_redraw();
        }
        if window.take_pending_frame() {
            let mut h = host.borrow_mut();
            h.redraw();
            h.sync_a11y();
            h.with_ctx(cambium_rootstock::Hook::AfterFrame);
        }
        if let Some(browser) = web_sys::window()
            && let Some(callback) = frame.borrow().as_ref()
        {
            let _ = browser.request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));

    let callback = next.borrow();
    let callback = callback.as_ref().ok_or("frame closure")?;
    browser
        .request_animation_frame(callback.as_ref().unchecked_ref())
        .map_err(|_| "could not schedule frames".to_string())?;
    Ok(())
}

/// Attach the DOM listeners that drive the host.
///
/// Every one of these is the same shape: read the event, say what happened in
/// the host's vocabulary, let the host decide. The ordering rules that used to
/// live in each event source are inside `pointer_moved` now, so this cannot
/// get them subtly wrong.
fn install_listeners<State, Logic, V>(
    canvas: &HtmlCanvasElement,
    host: &Rc<RefCell<Host<State, Logic, V>>>,
    window: &WebWindow,
) -> Result<(), String>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// Where a pointer event landed, in the layout's own coordinates.
    ///
    /// The browser reports CSS pixels relative to the viewport; the canvas
    /// rect makes them canvas-relative, and `zoom` carries them the rest of
    /// the way into the space the layout was built in. The desktop source
    /// divides physical pixels by the whole layout scale for the same reason —
    /// here the device half is already gone, so only the zoom is left.
    fn local(canvas: &HtmlCanvasElement, x: i32, y: i32, zoom: f32) -> (f32, f32) {
        let rect = canvas.get_bounding_client_rect();
        (
            ((x as f64 - rect.x()) as f32) / zoom,
            ((y as f64 - rect.y()) as f32) / zoom,
        )
    }

    macro_rules! listen {
        ($target:expr, $name:literal, $ty:ty, $handler:expr) => {{
            let closure = Closure::<dyn FnMut($ty)>::new($handler);
            $target
                .add_event_listener_with_callback($name, closure.as_ref().unchecked_ref())
                .map_err(|_| concat!("could not attach ", $name).to_string())?;
            // Deliberately leaked: the listener must outlive this call, and a
            // mounted application lives as long as its page.
            closure.forget();
        }};
    }

    let h = host.clone();
    let c = canvas.clone();
    let w = window.clone();
    listen!(
        canvas,
        "pointerdown",
        web_sys::PointerEvent,
        move |e: web_sys::PointerEvent| {
            let mut host = h.borrow_mut();
            let (x, y) = local(&c, e.client_x(), e.client_y(), host.ui_zoom());
            host.pointer_moved(x, y);
            host.click();
            w.request_redraw();
        }
    );

    let h = host.clone();
    let c = canvas.clone();
    let w = window.clone();
    listen!(
        canvas,
        "pointermove",
        web_sys::PointerEvent,
        move |e: web_sys::PointerEvent| {
            let mut host = h.borrow_mut();
            let (x, y) = local(&c, e.client_x(), e.client_y(), host.ui_zoom());
            host.pointer_moved(x, y);
            w.request_redraw();
        }
    );

    let h = host.clone();
    let c = canvas.clone();
    let w = window.clone();
    listen!(
        canvas,
        "pointerup",
        web_sys::PointerEvent,
        move |e: web_sys::PointerEvent| {
            let mut host = h.borrow_mut();
            let (x, y) = local(&c, e.client_x(), e.client_y(), host.ui_zoom());
            host.pointer_moved(x, y);
            host.release();
            w.request_redraw();
        }
    );

    let h = host.clone();
    let w = window.clone();
    listen!(
        canvas,
        "wheel",
        web_sys::WheelEvent,
        move |e: web_sys::WheelEvent| {
            // The page must not scroll underneath an application that handles the
            // notch itself.
            e.prevent_default();
            let (dx, dy) = wheel_delta_from_dom(&e, WHEEL_LINE_PX, WHEEL_PAGE_PX);
            h.borrow_mut().wheel(dx, dy);
            w.request_redraw();
        }
    );

    let h = host.clone();
    let w = window.clone();
    listen!(
        canvas,
        "keydown",
        web_sys::KeyboardEvent,
        move |e: web_sys::KeyboardEvent| {
            let press = key_press_from_dom(&e);
            // Tab moves focus inside the application, not out of it; the arrows
            // and Space scroll the page unless claimed.
            if !press.modifiers.is_command_chord() {
                e.prevent_default();
            }
            h.borrow_mut().key(&press);
            w.request_redraw();
        }
    );

    for (name, kind) in [
        ("compositionstart", CompositionKind::Start),
        ("compositionupdate", CompositionKind::Update),
        ("compositionend", CompositionKind::End),
    ] {
        let h = host.clone();
        let w = window.clone();
        let closure = Closure::<dyn FnMut(web_sys::CompositionEvent)>::new(
            move |e: web_sys::CompositionEvent| {
                h.borrow_mut()
                    .ime(composition_from_dom(kind, e.data().unwrap_or_default()));
                w.request_redraw();
            },
        );
        canvas
            .add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())
            .map_err(|_| format!("could not attach {name}"))?;
        closure.forget();
    }

    // The canvas has to be focusable to receive keys at all.
    let _ = canvas.set_attribute("tabindex", "0");
    Ok(())
}
