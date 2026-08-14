//! The frame pipeline: retained layout, paint emission, presentation, and
//! accessibility synchronization. Extracted from the woodshed-genet donor.
//!
//! A frame is deliberately in two halves. [`Host::relayout`] brings the
//! retained layout up to date and needs no GPU and no window; everything after
//! it rasterizes and presents. The split is what lets [`Harness`](crate::Harness)
//! run an application's real layout, hit testing, and input routing in an
//! ordinary `cargo test`.

use std::collections::HashMap;

use cambium::PointerClick;
use cambium_winit_a11y::A11yAction;
use genet_layout::{
    Applied, IncrementalLayout, InteractionState, LeafPaintSource, ScrollOffsets, SourceNodeId,
};
use genet_scripted_dom::NodeId;
use layout_dom_api::{DomMutation, LayoutDomMut as _};
use netrender::{ColorLoad, ExternalTexturePlacement};
use paint_list_api::{ColorF, DeviceIntSize, PaintList as _};

use crate::input::to_visual_caret;
use crate::meristem_bounds::RootView;
use crate::{AppCtx, Host};

struct SpriggingSource<'a> {
    rendered: &'a sprigging::RenderedLeaves,
    /// Leaf key → (FragmentId, epoch), synced by `sync_leaf_fragments`
    /// immediately before every emit, so an entry here is current by
    /// construction. Empty in headless runs (no surface, no registry),
    /// which keeps the `Harness` on the pixel-identical inline path.
    fragments: &'a std::collections::HashMap<u64, (u64, u64)>,
}

impl LeafPaintSource for SpriggingSource<'_> {
    fn leaf_commands(&self, key: u64) -> Option<&[paint_list_api::PaintCmd]> {
        self.rendered.get(key)
    }

    fn leaf_fragment(&self, key: u64) -> Option<u64> {
        self.fragments.get(&key).map(|(id, _epoch)| *id)
    }
}

/// The focused text field's paint inputs for this frame: which node, where the
/// caret is, and the selection byte range when one should be drawn.
type FocusedOverlay = (NodeId, genet_layout::VisualCaret, Option<(usize, usize)>);

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// Run the application's per-frame hook. Returns `true` when it wants more
    /// frames (an animation is live).
    fn frame_hook(&mut self) -> bool {
        let animating = {
            let logical_size = self.logical_size();
            let geometry = self.geometry();
            let commands = self.s.commands.clone();
            let window = self.s.window.as_deref();
            let Some(runner) = self.s.runner.as_mut() else {
                return false;
            };
            let mut ctx = AppCtx {
                runner,
                window,
                logical_size,
                leaves: &mut self.s.leaves,
                set_sheet: &mut self.s.pending_sheet,
                close: &mut self.s.close_requested,
                wake: &self.wake,
                capture: &mut self.s.pending_capture,
                pointer: &mut self.s.pending_pointer,
                window_commands: &commands,
                geometry,
            };
            (self.hooks.frame)(&mut ctx)
        };
        if let Some(sheet) = self.s.pending_sheet.take() {
            self.s.sheet = sheet;
            self.s.layout = None;
            self.s.layout_size = (0.0, 0.0);
        }
        animating
    }

    /// Bring the retained layout up to date at `(lw, lh)` logical px: drain the
    /// runner's DOM mutations, apply them incrementally or rebuild, advance the
    /// CSS-transition clock, and re-render the custom-paint leaves at their new
    /// boxes. No GPU, no window. Returns whether an animation is still live.
    pub(crate) fn relayout(&mut self, lw: f32, lh: f32) -> bool {
        let Some(runner) = self.s.runner.as_ref() else {
            return false;
        };
        let now_s = self.s.anim_base.elapsed().as_secs_f64();
        let dom = runner.dom();
        let mut muts: Vec<DomMutation<NodeId>> = Vec::new();
        dom.borrow_mut().drain_mutations(&mut muts);
        let dom_ref = dom.borrow();
        let sheets: Vec<&str> = vec![self.s.sheet.as_str()];
        let structural = muts
            .iter()
            .any(|m| !matches!(m, DomMutation::AttributeChanged { .. }));
        let size_changed = self.s.layout_size != (lw, lh);
        match self.s.layout.as_mut() {
            Some(layout) if !structural && !size_changed => {
                // Advance the CSS-transition clock to now, then apply this
                // frame's mutations, so a transition a class-swap starts runs
                // from *now*, not a stale idle-frozen clock.
                let _ = layout.tick_animations(&*dom_ref, now_s);
                if !muts.is_empty() {
                    let _ = layout.apply(&*dom_ref, &sheets, &muts);
                }
            },
            _ => {
                let mut layout = IncrementalLayout::new(&*dom_ref, &sheets, lw, lh);
                // Carry BOTH scroll planes across rebuilds: element scroll and
                // the document scroll. Dropping the latter snaps a scrolled
                // page back to the top on structural re-render.
                if let Some(prev) = self.s.layout.as_ref() {
                    layout.set_element_scroll(prev.element_scroll().clone());
                    layout.set_viewport_scroll(&*dom_ref, prev.viewport_scroll());
                }
                self.s.layout = Some(layout);
                self.s.layout_size = (lw, lh);
            },
        }
        let layout = self.s.layout.as_ref().expect("layout just ensured");
        let anim_active = layout.has_active_animations();
        let sizes: HashMap<u64, (f32, f32)> = layout.custom_leaf_boxes().into_iter().collect();
        self.s.leaves.render_into(
            |key| {
                sizes
                    .get(&key)
                    .map(|&(width, height)| sprigging::Size { width, height })
            },
            &mut self.s.rendered,
        );
        anim_active
    }

    /// Netrender roadmap E4 — reconcile the renderer's retained-fragment
    /// registry with this frame's rendered leaves. Runs each redraw after
    /// `relayout` (which refreshed `rendered`) and before `emit_scene`, so the
    /// map the emitter consults is current by construction.
    ///
    /// Only Path-A splices free of per-frame state become fragments: a splice
    /// carrying `DrawExternalTexture` or `DrawShadow` keeps the inline path,
    /// because composite textures and blurred shadow masks are rebuilt per
    /// frame by the host painter and cannot be retained in a lowering.
    fn sync_leaf_fragments(&mut self) {
        use paint_list_api::PaintCmd;

        let Some(surface) = self.s.surface.as_ref() else {
            // No surface means no renderer, and any previously registered
            // fragments died with it. Clear so a resumed surface re-registers
            // from scratch instead of placing dangling ids.
            self.s.leaf_fragments.clear();
            return;
        };
        let renderer = surface.core().renderer();

        let mut seen: Vec<u64> = Vec::new();
        for (key, epoch, splice) in self.s.rendered.path_a_entries() {
            let fragmentable = !splice.iter().any(|cmd| {
                matches!(
                    cmd,
                    PaintCmd::DrawExternalTexture(_) | PaintCmd::DrawShadow(_)
                )
            });
            if !fragmentable {
                if let Some((id, _)) = self.s.leaf_fragments.remove(&key) {
                    let _ = renderer.remove_fragment(id);
                }
                continue;
            }
            seen.push(key);
            match self.s.leaf_fragments.get(&key) {
                Some((_, e)) if *e == epoch => {},
                Some(&(id, _)) => {
                    let fragment =
                        paint_list_render::translate_paint_cmds_to_fragment(splice, &[], &[]);
                    if renderer.update_fragment(id, fragment) == Some(true) {
                        self.s.leaf_fragments.insert(key, (id, epoch));
                    }
                },
                None => {
                    let fragment =
                        paint_list_render::translate_paint_cmds_to_fragment(splice, &[], &[]);
                    if let Some(id) = renderer.register_fragment(fragment) {
                        self.s.leaf_fragments.insert(key, (id, epoch));
                    }
                },
            }
        }
        // Sweep leaves that no longer render (removed from the registry or
        // not laid out): their retained lowerings go with them.
        let stale: Vec<u64> = self
            .s
            .leaf_fragments
            .keys()
            .copied()
            .filter(|k| !seen.contains(k))
            .collect();
        for key in stale {
            if let Some((id, _)) = self.s.leaf_fragments.remove(&key) {
                let _ = renderer.remove_fragment(id);
            }
        }
    }

    /// The focused text field's paint inputs, as the application maps them.
    fn focused_overlay(&self) -> Option<FocusedOverlay> {
        let runner = self.s.runner.as_ref()?;
        let slot = (self.hooks.focused_text)(runner)?;
        let input = (slot.get)(runner.state());
        let mut caret = to_visual_caret(input.caret_position());
        caret.byte = input.caret_byte_in_render();
        let selection = if input.composition().is_none() && input.has_selection() {
            Some(input.selection_bytes())
        } else {
            None
        };
        Some((slot.node, caret, selection))
    }

    /// Tell the platform IME where the caret is, so a candidate window opens
    /// beside the text rather than at the window origin.
    fn sync_ime_area(&self) {
        let (Some(window), Some(layout), Some(runner)) = (
            self.s.window.as_ref(),
            self.s.layout.as_ref(),
            self.s.runner.as_ref(),
        ) else {
            return;
        };
        let Some((node, caret, _)) = self.focused_overlay() else {
            return;
        };
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let Some(rect) = layout.caret_rect_for_position(&*dom_ref, node, caret, 2.0) else {
            return;
        };
        window.set_ime_cursor_area(
            winit::dpi::LogicalPosition::new(rect.x as f64, rect.y as f64),
            winit::dpi::LogicalSize::new(rect.width.max(2.0) as f64, rect.height.max(1.0) as f64),
        );
    }

    /// Emit this frame's paint list — content, then the caret/selection
    /// overlay, then whatever overlay scrollbars are mid-hold or mid-fade — and
    /// lower it to a netrender scene.
    fn emit_scene(&self, lw: f32, lh: f32) -> Option<netrender::Scene> {
        let runner = self.s.runner.as_ref()?;
        let layout = self.s.layout.as_ref()?;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let source = SpriggingSource {
            rendered: &self.s.rendered,
            fragments: &self.s.leaf_fragments,
        };
        let mut list = layout.emit_paint_list_with_leaves(
            &*dom_ref,
            &ScrollOffsets::default(),
            DeviceIntSize::new(lw as i32, lh as i32),
            &source,
        );
        if let Some((node, caret, selection)) = self.focused_overlay() {
            if let Some((start, end)) = selection {
                let rects = layout.selection_rects(&*dom_ref, node, start, end);
                let color = layout
                    .selection_style(&*dom_ref, node)
                    .map(|(bg, _)| ColorF {
                        r: bg[0],
                        g: bg[1],
                        b: bg[2],
                        a: bg[3],
                    })
                    .unwrap_or(ColorF {
                        r: 0.20,
                        g: 0.45,
                        b: 0.90,
                        a: 0.35,
                    });
                list.push_selection(&rects, color);
            }
            if let Some(rect) = layout.caret_rect_for_position(&*dom_ref, node, caret, 2.0) {
                let color = layout
                    .caret_color(&*dom_ref, node)
                    .map(|rgba| ColorF {
                        r: rgba[0],
                        g: rgba[1],
                        b: rgba[2],
                        a: rgba[3],
                    })
                    .unwrap_or(ColorF {
                        r: 0.92,
                        g: 0.94,
                        b: 0.98,
                        a: 1.0,
                    });
                list.push_caret(rect, color);
            }
        }
        // Overlay scrollbar thumbs mid-hold/mid-fade: the engine draws the
        // geometry, the shared fade clock supplies alpha.
        let now = std::time::Instant::now();
        let fade = &self.s.scrollbar_fade;
        layout.append_scrollbars(&*dom_ref, &mut list, &|t| fade.alpha(t, now));
        let translated = paint_list_render::translate_paint_cmd_stream(
            list.viewport(),
            list.commands(),
            list.fonts(),
            list.images(),
        );
        Some(translated.scene)
    }

    pub(crate) fn redraw(&mut self) {
        // The application's frame hook first: animation drives, leaf syncs,
        // backend polls. Its return keeps frames coming.
        let animating = self.frame_hook();
        let target_size = self.s.window.as_ref().map(|window| {
            let size = window.inner_size();
            let scale = window.scale_factor() as f32;
            (size.width.max(1), size.height.max(1), scale)
        });
        let (Some((pw, ph, scale)), true) = (target_size, self.s.surface.is_some()) else {
            // No window, or suspended with the surface taken away: there is
            // nothing to present. The layout still advances, so a resume
            // repaints current state rather than a stale one.
            let (lw, lh) = self.logical_size();
            self.relayout(lw, lh);
            return;
        };
        let (lw, lh) = (pw as f32 / scale, ph as f32 / scale);

        let anim_active = self.relayout(lw, lh);
        self.sync_leaf_fragments();
        self.sync_ime_area();
        let Some(scene) = self.emit_scene(lw, lh) else {
            return;
        };

        let Some(surface) = self.s.surface.as_ref() else {
            return;
        };
        let (_tex, view) = surface.core().rasterize_scaled(
            &scene,
            pw,
            ph,
            ColorLoad::Clear(wgpu::Color::BLACK),
            scale,
        );
        let Some(frame) = surface.acquire() else {
            return;
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        surface.renderer().compose_external_texture(
            &view,
            &target,
            surface.format(),
            pw,
            ph,
            ExternalTexturePlacement::new([0.0, 0.0, pw as f32, ph as f32]),
        );
        frame.present();
        // A capture armed by the application: run it while the rasterized
        // view is still alive.
        if let Some(capture) = self.s.pending_capture.take() {
            capture(surface, &view, pw, ph);
        }
        if (animating || anim_active)
            && let Some(window) = self.s.window.as_ref()
        {
            window.request_redraw();
        }
        // A `frame` hook runs before this frame's layout, so anything it queued
        // is delivered here instead — hit-testing against the layout that was
        // just built rather than the previous one.
        self.drain_pointer();
    }

    /// Hand this frame to the accessibility host: build/install/update the
    /// tree, and route the screen reader's requests back into the retained DOM.
    /// Called after `redraw`, once this frame's layout exists. The first sync
    /// reveals the hidden window (install-before-show).
    ///
    /// The two request kinds stay apart, because they mean different things to
    /// the person using the reader: `Click` is "do this control's thing" and
    /// goes through the same dispatch a mouse press does; `Focus` is "put the
    /// cursor here" and only moves focus. Collapsing them would fire every
    /// control a reader navigates across.
    pub(crate) fn sync_a11y(&mut self) {
        let requests = {
            let dom = match self.s.runner.as_ref() {
                Some(runner) => runner.dom(),
                None => return,
            };
            let dom_ref = dom.borrow();
            let (Some(a11y), Some(window), Some(layout)) = (
                self.s.a11y.as_mut(),
                self.s.window.as_ref(),
                self.s.layout.as_ref(),
            ) else {
                return;
            };
            a11y.sync(
                window,
                &dom_ref,
                layout,
                &mut self.s.leaves,
                self.s.last_focus,
            )
        };
        self.apply_a11y_requests(&requests);
    }

    /// Route drained screen-reader requests into the retained DOM. Split out of
    /// [`sync_a11y`] so the routing is exercisable without an OS adapter.
    pub(crate) fn apply_a11y_requests(&mut self, requests: &[cambium_winit_a11y::A11yRequest]) {
        if requests.is_empty() {
            return;
        }
        for request in requests {
            let Some(runner) = self.s.runner.as_mut() else {
                break;
            };
            match request.action {
                A11yAction::Click => {
                    // No cursor is involved, so the local point is genuinely the
                    // element's own origin rather than a hit position.
                    runner.dispatch_click(request.node, PointerClick::at((0.0, 0.0)));
                },
                A11yAction::Focus => runner.set_focus(Some(request.node)),
            }
        }
        // Focus may have moved without any pointer motion; refresh the
        // `:focus` restyle so the visible state matches what the reader says.
        self.hover();
        self.after_dispatch();
    }

    /// Drive `:hover` / `:focus` restyles on target change (engine
    /// `set_interaction`; `Unchanged` when nothing interaction-sensitive
    /// matched, so idle movement stays free).
    pub(crate) fn hover(&mut self) {
        let (Some(runner), Some(layout)) = (self.s.runner.as_ref(), self.s.layout.as_mut()) else {
            return;
        };
        let (x, y) = self.s.cursor;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let hovered = layout
            .hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
            .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        let focused = runner
            .focus()
            .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        if (hovered, focused) == (self.s.last_hover, self.s.last_focus) {
            return;
        }
        self.s.last_hover = hovered;
        self.s.last_focus = focused;
        // Bring the transition clock to now before the flip, so a
        // hover/focus transition runs from now rather than a stale
        // idle-frozen clock.
        let now_s = self.s.anim_base.elapsed().as_secs_f64();
        let _ = layout.tick_animations(&*dom_ref, now_s);
        let state = InteractionState {
            hovered: hovered.map(SourceNodeId),
            focused: focused.map(SourceNodeId),
            ..Default::default()
        };
        if layout.set_interaction(&*dom_ref, &state) != Applied::Unchanged {
            drop(dom_ref);
            if let Some(window) = self.s.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    /// Route Cambium `on_hover` Enter/Leave as the hit node changes. The host
    /// owns transition detection; Move is not routed, so idle motion within a
    /// target stays free. Coordinates are zeroed — a peek only needs which
    /// target.
    pub(crate) fn hover_dispatch(&mut self) {
        use cambium::{HoverEvent, HoverPhase};
        let hit = self.hit_at_cursor();
        if hit == self.s.last_hover_hit {
            return;
        }
        let old = self.s.last_hover_hit.take();
        self.s.last_hover_hit = hit;
        let Some(runner) = self.s.runner.as_mut() else {
            return;
        };
        if let Some(old) = old {
            runner.dispatch_hover(
                old,
                HoverEvent::new(HoverPhase::Leave, (0.0, 0.0), (0.0, 0.0)),
            );
        }
        if let Some(new) = hit {
            runner.dispatch_hover(
                new,
                HoverEvent::new(HoverPhase::Enter, (0.0, 0.0), (0.0, 0.0)),
            );
        }
        self.after_dispatch();
    }
}
