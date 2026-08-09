//! The frame pipeline: retained layout, paint emission, presentation, and
//! accessibility synchronization. Extracted from the woodshed-genet donor.

use std::collections::HashMap;

use cambium::{PointerClick, Propagation};
use genet_layout::{
    Applied, IncrementalLayout, InteractionState, LeafPaintSource, ScrollOffsets,
    SourceNodeId,
};
use genet_scripted_dom::NodeId;
use layout_dom_api::{DomMutation, LayoutDomMut as _};
use netrender::{ColorLoad, ExternalTexturePlacement};
use paint_list_api::{ColorF, DeviceIntSize, PaintList as _};

use crate::input::to_visual_caret;
use crate::meristem_bounds::RootView;
use crate::{AppCtx, Host};

struct SpriggingSource<'a>(&'a sprigging::RenderedLeaves);

impl LeafPaintSource for SpriggingSource<'_> {
    fn leaf_commands(&self, key: u64) -> Option<&[paint_list_api::PaintCmd]> {
        self.0.get(key)
    }
}

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    pub(crate) fn redraw(&mut self) {
        // The application's frame hook first: animation drives, leaf syncs,
        // backend polls. Its return keeps frames coming.
        let animating = {
            let (Some(window), Some(runner)) =
                (self.s.window.as_ref(), self.s.runner.as_mut())
            else {
                return;
            };
            let mut ctx = AppCtx {
                runner,
                window,
                leaves: &mut self.s.leaves,
                set_sheet: &mut self.s.pending_sheet,
                close: &mut self.s.close_requested,
                capture: &mut self.s.pending_capture,
            };
            (self.hooks.frame)(&mut ctx)
        };
        if let Some(sheet) = self.s.pending_sheet.take() {
            self.s.sheet = sheet;
            self.s.layout = None;
            self.s.layout_size = (0.0, 0.0);
        }

        let (Some(window), Some(surface), Some(runner)) = (
            self.s.window.as_ref(),
            self.s.surface.as_ref(),
            self.s.runner.as_ref(),
        ) else {
            return;
        };
        let size = window.inner_size();
        let (pw, ph) = (size.width.max(1), size.height.max(1));
        let scale = window.scale_factor() as f32;
        let (lw, lh) = (pw as f32 / scale, ph as f32 / scale);

        let now_s = self.s.anim_base.elapsed().as_secs_f64();
        let (scene, anim_active) = {
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
                    // frame's mutations, so a transition a class-swap starts
                    // runs from *now*, not a stale idle-frozen clock.
                    let _ = layout.tick_animations(&*dom_ref, now_s);
                    if !muts.is_empty() {
                        let _ = layout.apply(&*dom_ref, &sheets, &muts);
                    }
                }
                _ => {
                    let mut layout = IncrementalLayout::new(&*dom_ref, &sheets, lw, lh);
                    // Carry BOTH scroll planes across rebuilds: element scroll
                    // and the document scroll. Dropping the latter snaps a
                    // scrolled page back to the top on structural re-render.
                    if let Some(prev) = self.s.layout.as_ref() {
                        layout.set_element_scroll(prev.element_scroll().clone());
                        layout.set_viewport_scroll(&*dom_ref, prev.viewport_scroll());
                    }
                    self.s.layout = Some(layout);
                    self.s.layout_size = (lw, lh);
                }
            }
            let layout = self.s.layout.as_ref().expect("layout just ensured");
            let focused_overlay = (self.hooks.focused_text)(runner).map(|slot| {
                let input = (slot.get)(runner.state());
                let mut caret = to_visual_caret(input.caret_position());
                caret.byte = input.caret_byte_in_render();
                let selection = if input.composition().is_none() && input.has_selection() {
                    Some(input.selection_bytes())
                } else {
                    None
                };
                (slot.node, caret, selection)
            });
            if let Some((node, caret, _)) = focused_overlay
                && let Some(rect) =
                    layout.caret_rect_for_position(&*dom_ref, node, caret, 2.0)
            {
                window.set_ime_cursor_area(
                    winit::dpi::LogicalPosition::new(rect.x as f64, rect.y as f64),
                    winit::dpi::LogicalSize::new(
                        rect.width.max(2.0) as f64,
                        rect.height.max(1.0) as f64,
                    ),
                );
            }
            let anim_active = layout.has_active_animations();
            let sizes: HashMap<u64, (f32, f32)> =
                layout.custom_leaf_boxes().into_iter().collect();
            self.s.leaves.render_into(
                |key| {
                    sizes
                        .get(&key)
                        .map(|&(width, height)| sprigging::Size { width, height })
                },
                &mut self.s.rendered,
            );
            let source = SpriggingSource(&self.s.rendered);
            let mut list = layout.emit_paint_list_with_leaves(
                &*dom_ref,
                &ScrollOffsets::default(),
                DeviceIntSize::new(lw as i32, lh as i32),
                &source,
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
                    list.push_selection(&rects, color);
                }
                if let Some(rect) = layout.caret_rect_for_position(&*dom_ref, node, caret, 2.0)
                {
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
            (translated.scene, anim_active)
        };

        let (_tex, view) = surface.core().rasterize_scaled(
            &scene,
            pw,
            ph,
            ColorLoad::Clear(wgpu::Color::BLACK),
            scale,
        );
        let Some(frame) = surface.acquire() else { return };
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
        if animating || anim_active {
            window.request_redraw();
        }
    }

    /// Hand this frame to the accessibility host: build/install/update the
    /// tree, and route any screen-reader activations through the same click
    /// path a mouse uses. Called after `redraw`, once this frame's layout
    /// exists. The first sync reveals the hidden window (install-before-show).
    pub(crate) fn sync_a11y(&mut self) {
        let nodes = {
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
            a11y.sync(window, &dom_ref, layout, &mut self.s.leaves, self.s.last_focus)
        };
        if nodes.is_empty() {
            return;
        }
        for node in nodes {
            if let Some(runner) = self.s.runner.as_mut() {
                runner.dispatch_click(
                    node,
                    PointerClick {
                        local: (0.0, 0.0),
                        prop: Propagation::new(),
                    },
                );
            }
        }
        self.after_dispatch();
    }

    /// Drive `:hover` / `:focus` restyles on target change (engine
    /// `set_interaction`; `Unchanged` when nothing interaction-sensitive
    /// matched, so idle movement stays free).
    pub(crate) fn hover(&mut self) {
        let (Some(runner), Some(layout)) =
            (self.s.runner.as_ref(), self.s.layout.as_mut())
        else {
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
        let (x, y) = self.s.cursor;
        let hit = {
            let (Some(runner), Some(layout)) =
                (self.s.runner.as_ref(), self.s.layout.as_ref())
            else {
                return;
            };
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            layout.hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
        };
        if hit == self.s.last_hover_hit {
            return;
        }
        let old = self.s.last_hover_hit.take();
        self.s.last_hover_hit = hit;
        let Some(runner) = self.s.runner.as_mut() else {
            return;
        };
        if let Some(old) = old {
            runner.dispatch_hover(old, HoverEvent::new(HoverPhase::Leave, (0.0, 0.0), (0.0, 0.0)));
        }
        if let Some(new) = hit {
            runner.dispatch_hover(new, HoverEvent::new(HoverPhase::Enter, (0.0, 0.0), (0.0, 0.0)));
        }
        self.after_dispatch();
    }
}
