// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The frame pipeline: retained layout, paint emission, presentation, and
//! accessibility synchronization. Extracted from the woodshed-genet donor.
//!
//! A frame is deliberately in two halves. [`Host::relayout`] brings the
//! retained layout up to date and needs no GPU and no window; everything after
//! it rasterizes and presents. The split is what lets [`Harness`](crate::Harness)
//! run an application's real layout, hit testing, and input routing in an
//! ordinary `cargo test`.

use std::collections::HashMap;

use crate::A11yAction;
use cambium::PointerClick;
use genet_render::VisualCaret;
use genet_scripted_dom::NodeId;
use layout_dom_api::{DomMutation, LayoutDomMut as _};
use netrender::{ColorLoad, ExternalTexturePlacement};
use paint_list_api::{ColorF, DeviceIntSize, PaintList as _};

use crate::input::to_visual_caret;
use crate::meristem_bounds::RootView;
use crate::{AppCtx, FrameProfile, Host};

fn elapsed_us(elapsed: crate::Duration) -> u64 {
    elapsed.as_micros().min(u64::MAX as u128) as u64
}

struct SpriggingSource<'a> {
    rendered: &'a sprigging::RenderedLeaves,
}

impl SpriggingSource<'_> {
    fn leaf_commands(&self, key: u64) -> Option<Vec<paint_list_api::PaintCmd>> {
        self.rendered.get(key).map(<[_]>::to_vec)
    }
}

/// The focused text field's paint inputs for this frame: which node, where the
/// caret is, and the selection byte range when one should be drawn.
type FocusedOverlay = (NodeId, VisualCaret, Option<(usize, usize)>);

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
            let (ui_zoom, zoom_changed) = self.take_zoom_edge();
            let geometry = self.s.geometry;
            let frame_profile = self.s.last_frame_profile;
            let commands = self.s.commands.clone();
            let window = self.s.window.as_deref();
            let Some(runner) = self.s.runner.as_mut() else {
                return false;
            };
            let mut ctx = AppCtx {
                runner,
                window,
                logical_size,
                ui_zoom,
                zoom_changed,
                leaves: &mut self.s.leaves,
                set_sheet: &mut self.s.pending_sheet,
                set_ui_zoom: &mut self.s.pending_ui_zoom,
                close: &mut self.s.close_requested,
                wake: &self.wake,
                capture: &mut self.s.pending_capture,
                pointer: &mut self.s.pending_pointer,
                window_commands: &commands,
                geometry,
                frame_profile,
            };
            (self.hooks.frame)(&mut ctx)
        };
        if let Some(sheet) = self.s.pending_sheet.take() {
            self.s.sheet = sheet;
            self.s.layout = None;
            self.s.layout_size = (0.0, 0.0);
        }
        if let Some(zoom) = self.s.pending_ui_zoom.take() {
            self.set_ui_zoom(zoom);
        }
        animating
    }

    /// Bring the retained layout up to date at `(lw, lh)` logical px: drain the
    /// runner's DOM mutations, rebuild the host-owned Livery frame when its
    /// inputs changed, and re-render custom-paint leaves at their new boxes. No
    /// GPU, no window. Returns whether an animation is still live.
    /// Republish the Window-Controls-Overlay titlebar area when it changes.
    ///
    /// As a stylesheet rule rather than an inline style on the root: the root's
    /// `style` attribute belongs to the application, and a host that
    /// overwrites it each frame would silently drop whatever the app put
    /// there. A `:root` rule declares the same inheriting custom properties
    /// without touching the DOM at all.
    ///
    /// Returns whether the values moved, because a changed sheet has to force
    /// the full relayout path — the incremental one applies DOM mutations, and
    /// this is not one.
    fn publish_titlebar_area(&mut self, lw: f32) -> bool {
        // The platform reports what it reserved in *device*-logical pixels —
        // macOS's traffic lights are a fixed size on screen, not a fixed
        // number of CSS pixels — and the sheet declares CSS ones. Under zoom
        // those are different units, so the strip is divided into layout space
        // before it is published; otherwise the page's content would clear a
        // gap of the wrong height at every zoom but 1.
        let zoom = self.ui_zoom();
        let insets = self
            .s
            .window
            .as_ref()
            .map_or(crate::TitlebarInsets::NONE, |w| {
                let insets = w.titlebar_insets();
                crate::TitlebarInsets {
                    left: insets.left / zoom,
                    right: insets.right / zoom,
                    height: insets.height / zoom,
                }
            });
        if self.s.titlebar_published == Some((insets, lw)) {
            return false;
        }
        self.s.titlebar_published = Some((insets, lw));
        self.s.titlebar_sheet = format!(":root {{ {} }}", insets.declarations(lw));
        true
    }

    pub fn relayout(&mut self, lw: f32, lh: f32) -> bool {
        if self.s.runner.is_none() {
            return false;
        }
        let layout_update_started = crate::Instant::now();
        let now_s = self.s.anim_base.elapsed().as_secs_f64();
        let titlebar_moved = self.publish_titlebar_area(lw);
        let runner = self.s.runner.as_ref().expect("checked above");
        let dom = runner.dom();
        let mut muts: Vec<DomMutation<NodeId>> = Vec::new();
        dom.borrow_mut().drain_mutations(&mut muts);
        let dom_ref = dom.borrow();
        let sheets: Vec<&str> = vec![self.s.sheet.as_str(), self.s.titlebar_sheet.as_str()];
        let mutation_count = muts.len() as u64;
        let size_changed = self.s.layout_size != (lw, lh);
        let mut tick_us = 0;
        let apply_us = 0;
        let mut rebuild_us = 0;
        let mut rebuilt = false;
        match self.s.layout.as_mut() {
            Some(layout) if muts.is_empty() && !size_changed && !titlebar_moved => {
                let phase = crate::Instant::now();
                let _ = layout.tick_animations(&*dom_ref, now_s);
                tick_us = elapsed_us(phase.elapsed());
            },
            Some(layout) if !size_changed && !titlebar_moved => {
                rebuilt = true;
                let phase = crate::Instant::now();
                layout.rebuild(&*dom_ref, lw, lh);
                rebuild_us = elapsed_us(phase.elapsed());
            },
            _ => {
                rebuilt = true;
                let phase = crate::Instant::now();
                let mut layout = crate::OwnedLayout::new(&*dom_ref, &sheets, lw, lh);
                // Carry BOTH scroll planes across rebuilds: element scroll and
                // the document scroll. Dropping the latter snaps a scrolled
                // page back to the top on structural re-render.
                if let Some(prev) = self.s.layout.as_ref() {
                    layout.set_element_scroll(prev.element_scroll().clone());
                    layout.set_viewport_scroll(prev.viewport_scroll());
                }
                self.s.layout = Some(layout);
                self.s.layout_size = (lw, lh);
                rebuild_us = elapsed_us(phase.elapsed());
            },
        }
        let layout = self.s.layout.as_ref().expect("layout just ensured");
        let anim_active = layout.has_active_animations();
        let layout_update_us = elapsed_us(layout_update_started.elapsed());
        let leaf_boxes_started = crate::Instant::now();
        let sizes: HashMap<u64, (f32, f32)> =
            layout.custom_leaf_boxes(&*dom_ref).into_iter().collect();
        let leaf_boxes_us = elapsed_us(leaf_boxes_started.elapsed());
        let leaf_render_started = crate::Instant::now();
        let leaf_repaints = self.s.leaves.render_into(
            |key| {
                sizes
                    .get(&key)
                    .map(|&(width, height)| sprigging::Size { width, height })
            },
            &mut self.s.rendered,
        );
        self.s.last_layout_update_us = layout_update_us;
        self.s.last_layout_tick_us = tick_us;
        self.s.last_layout_apply_us = apply_us;
        self.s.last_layout_rebuild_us = rebuild_us;
        self.s.last_layout_mutations = mutation_count;
        self.s.last_layout_rebuilt = rebuilt;
        self.s.last_leaf_boxes_us = leaf_boxes_us;
        self.s.last_leaf_render_us = elapsed_us(leaf_render_started.elapsed());
        self.s.last_leaf_repaints = leaf_repaints as u64;
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
        // The seam takes the *platform's* logical coordinates (winit multiplies
        // them by the device scale, a browser by nothing), and the caret rect
        // is in layout coordinates. Those differ by exactly the zoom, so the
        // rect is carried back across it here — a candidate window that opened
        // at four fifths of the caret's position would be a zoom bug the user
        // sees before any other.
        let zoom = f64::from(self.ui_zoom());
        window.set_ime_cursor_area(
            rect.x as f64 * zoom,
            rect.y as f64 * zoom,
            (rect.width.max(2.0) as f64) * zoom,
            (rect.height.max(1.0) as f64) * zoom,
        );
    }

    /// Emit this frame's paint list — content, then the caret/selection
    /// overlay, then whatever overlay scrollbars are mid-hold or mid-fade — and
    /// lower it to a netrender scene plus the GPU resources that scene names.
    fn emit_scene(&mut self, lw: f32, lh: f32) -> Option<paint_list_render::TranslatedDisplayList> {
        let focused_overlay = self.focused_overlay();
        let runner = self.s.runner.as_ref()?;
        let layout = self.s.layout.as_mut()?;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let source = SpriggingSource {
            rendered: &self.s.rendered,
        };
        let mut list = layout.emit_paint_list_with_leaves(
            &*dom_ref,
            DeviceIntSize::new(lw as i32, lh as i32),
            |key| source.leaf_commands(key),
            // A retained fragment has no CSS clip or layer identity in the
            // renderer yet. Keep custom leaves in the recorded Livery slot as
            // ordinary commands until that composition boundary is real.
            // This preserves their stacking relation to DOM overlays.
            |_| None,
        );
        if let Some((node, caret, selection)) = focused_overlay {
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
                for rect in rects {
                    crate::OwnedLayout::push_rect(&mut list, rect, color);
                }
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
                crate::OwnedLayout::push_rect(&mut list, rect, color);
            }
        }
        // Overlay scrollbar thumbs mid-hold/mid-fade: the engine draws the
        // geometry, the shared fade clock supplies alpha.
        let now = crate::Instant::now();
        let fade = &self.s.scrollbar_fade;
        layout.append_scrollbars(&*dom_ref, &mut list, &|t| fade.alpha(t, now));
        let translated = paint_list_render::translate_paint_cmd_stream(
            list.viewport(),
            list.commands(),
            list.fonts(),
            list.images(),
        );
        Some(translated)
    }

    pub fn redraw(&mut self) {
        let frame_started = crate::Instant::now();
        let mut profile = FrameProfile::default();
        // The application's frame hook first: animation drives, leaf syncs,
        // backend polls. Its return keeps frames coming.
        let phase = crate::Instant::now();
        let animating = self.frame_hook();
        profile.frame_hook_us = elapsed_us(phase.elapsed());
        // One scale for the whole frame: device times zoom. Layout runs at
        // `physical / layout_scale` and the rasterizer composes that scene
        // under the same factor, so a zoomed frame is laid out at its new size
        // and drawn from outlines at full device resolution — not a smaller
        // frame resampled upward.
        let layout_scale = self.layout_scale() as f32;
        let target_size = self.s.window.as_ref().map(|window| {
            let size = window.inner_size();
            (size.0.max(1), size.1.max(1), layout_scale)
        });
        let (Some((pw, ph, scale)), true) = (target_size, self.s.surface.is_some()) else {
            // No window, or suspended with the surface taken away: there is
            // nothing to present. The layout still advances, so a resume
            // repaints current state rather than a stale one.
            let (lw, lh) = self.logical_size();
            let phase = crate::Instant::now();
            self.relayout(lw, lh);
            profile.relayout_us = elapsed_us(phase.elapsed());
            profile.total_us = elapsed_us(frame_started.elapsed());
            self.s.last_frame_profile = Some(profile);
            return;
        };
        let (lw, lh) = (pw as f32 / scale, ph as f32 / scale);

        let phase = crate::Instant::now();
        let anim_active = self.relayout(lw, lh);
        profile.relayout_us = elapsed_us(phase.elapsed());
        profile.layout_update_us = self.s.last_layout_update_us;
        profile.layout_tick_us = self.s.last_layout_tick_us;
        profile.layout_apply_us = self.s.last_layout_apply_us;
        profile.layout_rebuild_us = self.s.last_layout_rebuild_us;
        profile.layout_mutations = self.s.last_layout_mutations;
        profile.layout_rebuilt = self.s.last_layout_rebuilt;
        profile.leaf_boxes_us = self.s.last_leaf_boxes_us;
        profile.leaf_render_us = self.s.last_leaf_render_us;
        profile.leaf_repaints = self.s.last_leaf_repaints;
        let phase = crate::Instant::now();
        self.sync_leaf_fragments();
        profile.leaf_fragments_us = elapsed_us(phase.elapsed());
        let phase = crate::Instant::now();
        self.sync_ime_area();
        profile.ime_us = elapsed_us(phase.elapsed());
        let phase = crate::Instant::now();
        let Some(translated) = self.emit_scene(lw, lh) else {
            profile.emit_scene_us = elapsed_us(phase.elapsed());
            profile.total_us = elapsed_us(frame_started.elapsed());
            self.s.last_frame_profile = Some(profile);
            return;
        };
        profile.emit_scene_us = elapsed_us(phase.elapsed());

        let Some(surface) = self.s.surface.as_ref() else {
            profile.total_us = elapsed_us(frame_started.elapsed());
            self.s.last_frame_profile = Some(profile);
            return;
        };
        // Blurred shadows lower to image ops backed by per-frame GPU masks.
        // Keeping only `translated.scene` drops those masks and leaves the
        // image keys unresolved, which makes every blurred CSS box-shadow
        // disappear in this host even though the paint list is correct.
        let phase = crate::Instant::now();
        for mask in &translated.box_shadow_masks {
            surface.renderer().build_box_shadow_mask(
                mask.key,
                mask.dim,
                mask.bounds,
                mask.corner_radius,
                mask.blur_radius_px,
                mask.invert,
            );
        }
        profile.shadows_us = elapsed_us(phase.elapsed());
        let scene = &translated.scene;
        let clear = if self.options.app_frame_is_transparent() {
            wgpu::Color::TRANSPARENT
        } else {
            wgpu::Color::BLACK
        };
        let phase = crate::Instant::now();
        let (_tex, view) =
            surface
                .core()
                .rasterize_scaled(scene, pw, ph, ColorLoad::Clear(clear), scale);
        profile.raster_us = elapsed_us(phase.elapsed());
        if let Some(timings) = surface.renderer().last_frame_timings() {
            let span = |name: &str| timings.span(name).map(elapsed_us).unwrap_or_default();
            profile.raster_total_us = elapsed_us(timings.total);
            profile.tile_invalidate_us = span("tile_invalidate");
            profile.dirty_tile_rebuild_us = span("dirty_tile_rebuild");
            profile.master_compose_us = span("master_compose");
            profile.vello_render_us = span("vello_render");
        }
        profile.dirty_tiles = surface
            .renderer()
            .vello_last_dirty_count()
            .unwrap_or_default() as u64;
        let phase = crate::Instant::now();
        let Some(frame) = surface.acquire() else {
            profile.acquire_us = elapsed_us(phase.elapsed());
            profile.total_us = elapsed_us(frame_started.elapsed());
            self.s.last_frame_profile = Some(profile);
            return;
        };
        profile.acquire_us = elapsed_us(phase.elapsed());
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // The composition pass blends and therefore loads its target. Clear a
        // fresh swapchain texture first so transparent app-frame margins do
        // not preserve undefined pixels from the compositor-owned image.
        let phase = crate::Instant::now();
        let mut encoder =
            surface
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("cambium surface clear"),
                });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cambium surface clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        surface.queue().submit([encoder.finish()]);
        profile.clear_us = elapsed_us(phase.elapsed());
        let phase = crate::Instant::now();
        surface.renderer().compose_external_texture(
            &view,
            &target,
            surface.format(),
            pw,
            ph,
            ExternalTexturePlacement::new([0.0, 0.0, pw as f32, ph as f32]),
        );
        profile.compose_us = elapsed_us(phase.elapsed());
        // wgpu 30 moved presentation from SurfaceTexture to Queue.
        let phase = crate::Instant::now();
        surface.queue().present(frame);
        profile.present_us = elapsed_us(phase.elapsed());
        // A capture armed by the application: run it while the rasterized
        // view is still alive.
        let phase = crate::Instant::now();
        if let Some(capture) = self.s.pending_capture.take() {
            capture(&**surface, &view, pw, ph);
        }
        profile.capture_us = elapsed_us(phase.elapsed());
        if (animating || anim_active)
            && let Some(window) = self.s.window.as_ref()
        {
            window.request_redraw();
        }
        // A `frame` hook runs before this frame's layout, so anything it queued
        // is delivered here instead — hit-testing against the layout that was
        // just built rather than the previous one.
        let phase = crate::Instant::now();
        self.drain_pointer();
        profile.pointer_us = elapsed_us(phase.elapsed());
        profile.total_us = elapsed_us(frame_started.elapsed());
        self.s.last_frame_profile = Some(profile);
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
    pub fn sync_a11y(&mut self) {
        // A screen reader is told physical client pixels, and the tree it is
        // told them about is laid out in layout pixels: the transform between
        // them is the layout scale, zoom included. A reader that read the
        // device scale alone would point at a control's unzoomed position.
        let layout_scale = self.layout_scale();
        let requests = {
            let dom = match self.s.runner.as_ref() {
                Some(runner) => runner.dom(),
                None => return,
            };
            let dom_ref = dom.borrow();
            let (Some(a11y), Some(layout)) = (self.s.a11y.as_mut(), self.s.layout.as_ref()) else {
                return;
            };
            // The window is the adapter's own now, so the seam does not carry it.
            a11y.sync(
                &dom_ref,
                layout,
                &mut self.s.leaves,
                self.s.last_focus,
                layout_scale,
            )
        };
        self.apply_a11y_requests(&requests);
    }

    /// Route drained screen-reader requests into the retained DOM. Split out of
    /// [`sync_a11y`] so the routing is exercisable without an OS adapter.
    pub fn apply_a11y_requests(&mut self, requests: &[crate::A11yRequest]) {
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
    pub fn hover(&mut self) {
        let (Some(runner), Some(layout)) = (self.s.runner.as_ref(), self.s.layout.as_mut()) else {
            return;
        };
        let (x, y) = self.s.cursor;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let hovered_node = layout.hit_test(&*dom_ref, x, y);
        let focused_node = runner.focus();
        let hovered = hovered_node.map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        let focused = focused_node.map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
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
        if layout.set_interaction(&*dom_ref, hovered_node, focused_node) {
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
    pub fn hover_dispatch(&mut self) {
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
