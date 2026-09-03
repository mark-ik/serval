/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Type erasure for retained product-owned Cambium runners.
//!
//! Frozen as v1 with the descriptor vocabulary (2026-08-26): the fourteen
//! trait methods stand, including the three no erased host calls yet —
//! `root`, `focusables`, and `pointer_capture` erase the same capabilities
//! the concrete hosts already rely on (subtree identity, spatial focus, and
//! mid-drag capture routing). Changes are additive until a v2.
//!
//! This boundary starts after a host has translated platform input and resolved
//! any DOM hit target. It intentionally does not perform layout, hit testing,
//! scene conversion, scrolling policy, accessibility hosting, or lifetime
//! management for the host.

use genet_scripted_dom::NodeId;
use mere_surface_api::{SurfaceAvailability, SurfaceDescriptor};
use meristem::View;

use crate::{
    DomHandle, GenetAppRunner, GenetCtx, GenetElement, HoverEvent, KeyEvent, PointerClick,
    PointerEvent, WheelEvent,
};

/// The smallest generic effect a retained surface can request from its host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceEffect {
    Redraw,
}

/// Viewport facts supplied by a host after it has laid out a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceViewport {
    pub width: f32,
    pub height: f32,
    pub scale_factor: f32,
}

/// A Cambium event whose DOM target, when one is needed, was already resolved
/// by the host. Raw host input has no DOM target and therefore cannot drive the
/// runner by itself.
#[derive(Clone, Debug)]
pub enum ResolvedSurfaceEvent {
    Click { target: NodeId, event: PointerClick },
    Key(KeyEvent),
    PointerDown { target: NodeId, event: PointerEvent },
    PointerMove(PointerEvent),
    PointerUp(PointerEvent),
    Hover { target: NodeId, event: HoverEvent },
    Wheel { target: NodeId, event: WheelEvent },
}

/// An object-safe retained product surface.
///
/// The concrete `GenetAppRunner` state, view, and action types remain inside
/// the session. Hosts retain this object and render its DOM through their own
/// layout path.
pub trait RetainedSurfaceSession {
    fn descriptor(&self) -> &SurfaceDescriptor;
    fn availability(&self) -> SurfaceAvailability;
    fn dom(&self) -> DomHandle;
    fn root(&self) -> NodeId;
    fn focus(&self) -> Option<NodeId>;
    fn set_focus(&mut self, node: Option<NodeId>) -> Vec<SurfaceEffect>;
    fn focus_traverse(&mut self, forward: bool) -> Vec<SurfaceEffect>;
    fn focusables(&self) -> Vec<NodeId>;
    fn pointer_capture(&self) -> Option<NodeId>;
    fn pointer_target(&self, hit: NodeId) -> Option<NodeId>;
    fn hover_target(&self, hit: NodeId) -> Option<NodeId>;
    fn wheel_target(&self, hit: NodeId) -> Option<NodeId>;
    fn sync_viewport(&mut self, viewport: SurfaceViewport) -> Vec<SurfaceEffect>;
    fn dispatch(&mut self, event: ResolvedSurfaceEvent) -> Vec<SurfaceEffect>;
}

/// A generic retained-session wrapper around one concrete [`GenetAppRunner`].
///
/// Availability, viewport synchronization, and action effects remain
/// product-owned closures. The host observes their results but does not author
/// product availability or actions.
pub struct RunnerSurfaceSession<State, Logic, V, Action, ViewportSync, ActionEffects>
where
    State: 'static,
    Action: 'static,
    Logic: FnMut(&State) -> V,
    V: View<State, Action, GenetCtx, Element = GenetElement>,
    ViewportSync: FnMut(&mut State, SurfaceViewport),
    ActionEffects: FnMut(Action) -> Vec<SurfaceEffect>,
{
    descriptor: SurfaceDescriptor,
    runner: GenetAppRunner<State, Logic, V, Action>,
    availability: Box<dyn Fn(&State) -> SurfaceAvailability>,
    viewport: Option<SurfaceViewport>,
    viewport_sync: ViewportSync,
    action_effects: ActionEffects,
}

impl<State, Logic, V, Action, ViewportSync, ActionEffects>
    RunnerSurfaceSession<State, Logic, V, Action, ViewportSync, ActionEffects>
where
    State: 'static,
    Action: 'static,
    Logic: FnMut(&State) -> V,
    V: View<State, Action, GenetCtx, Element = GenetElement>,
    ViewportSync: FnMut(&mut State, SurfaceViewport),
    ActionEffects: FnMut(Action) -> Vec<SurfaceEffect>,
{
    pub fn new(
        descriptor: SurfaceDescriptor,
        runner: GenetAppRunner<State, Logic, V, Action>,
        availability: impl Fn(&State) -> SurfaceAvailability + 'static,
        viewport_sync: ViewportSync,
        action_effects: ActionEffects,
    ) -> Self {
        Self {
            descriptor,
            runner,
            availability: Box::new(availability),
            viewport: None,
            viewport_sync,
            action_effects,
        }
    }

    fn effects(&mut self, actions: Vec<Action>) -> Vec<SurfaceEffect> {
        let mut effects = Vec::with_capacity(actions.len() + 1);
        effects.push(SurfaceEffect::Redraw);
        for action in actions {
            effects.extend((self.action_effects)(action));
        }
        effects
    }
}

impl<State, Logic, V, Action, ViewportSync, ActionEffects> RetainedSurfaceSession
    for RunnerSurfaceSession<State, Logic, V, Action, ViewportSync, ActionEffects>
where
    State: 'static,
    Action: 'static,
    Logic: FnMut(&State) -> V,
    V: View<State, Action, GenetCtx, Element = GenetElement>,
    ViewportSync: FnMut(&mut State, SurfaceViewport),
    ActionEffects: FnMut(Action) -> Vec<SurfaceEffect>,
{
    fn descriptor(&self) -> &SurfaceDescriptor {
        &self.descriptor
    }

    fn availability(&self) -> SurfaceAvailability {
        (self.availability)(self.runner.state())
    }

    fn dom(&self) -> DomHandle {
        self.runner.dom()
    }

    fn root(&self) -> NodeId {
        self.runner.root()
    }

    fn focus(&self) -> Option<NodeId> {
        self.runner.focus()
    }

    fn set_focus(&mut self, node: Option<NodeId>) -> Vec<SurfaceEffect> {
        self.runner.set_focus(node);
        vec![SurfaceEffect::Redraw]
    }

    fn focus_traverse(&mut self, forward: bool) -> Vec<SurfaceEffect> {
        self.runner.focus_traverse(forward);
        vec![SurfaceEffect::Redraw]
    }

    fn focusables(&self) -> Vec<NodeId> {
        self.runner.focusables()
    }

    fn pointer_capture(&self) -> Option<NodeId> {
        self.runner.pointer_capture()
    }

    fn pointer_target(&self, hit: NodeId) -> Option<NodeId> {
        self.runner.pointer_target(hit)
    }

    fn hover_target(&self, hit: NodeId) -> Option<NodeId> {
        self.runner.hover_target(hit)
    }

    fn wheel_target(&self, hit: NodeId) -> Option<NodeId> {
        self.runner.wheel_target(hit)
    }

    fn sync_viewport(&mut self, viewport: SurfaceViewport) -> Vec<SurfaceEffect> {
        if self.viewport == Some(viewport) {
            return Vec::new();
        }
        self.viewport = Some(viewport);
        let viewport_sync = &mut self.viewport_sync;
        self.runner.update(|state| viewport_sync(state, viewport));
        vec![SurfaceEffect::Redraw]
    }

    fn dispatch(&mut self, event: ResolvedSurfaceEvent) -> Vec<SurfaceEffect> {
        let actions = match event {
            ResolvedSurfaceEvent::Click { target, event } => {
                self.runner.dispatch_click(target, event)
            },
            ResolvedSurfaceEvent::Key(event) => self.runner.dispatch_key(event),
            ResolvedSurfaceEvent::PointerDown { target, event } => {
                self.runner.dispatch_pointer_down(target, event)
            },
            ResolvedSurfaceEvent::PointerMove(event) => self.runner.dispatch_pointer_move(event),
            ResolvedSurfaceEvent::PointerUp(event) => self.runner.dispatch_pointer_up(event),
            ResolvedSurfaceEvent::Hover { target, event } => {
                self.runner.dispatch_hover(target, event)
            },
            ResolvedSurfaceEvent::Wheel { target, event } => {
                self.runner.dispatch_wheel(target, event)
            },
        };
        self.effects(actions)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use genet_scripted_dom::{NodeId, ScriptedDom};
    use layout_dom_api::{LayoutDom, NodeKind};
    use mere_surface_api::{
        ProviderId, SourceKindId, SurfaceId, SurfaceSourceShape, SurfaceUnavailableReason,
    };

    use crate::{DomHandle, El, GenetAppRunner, OnClick, PointerClick, el, on_click};

    use super::*;

    struct First {
        count: u32,
        width: f32,
    }

    struct Second {
        count: u32,
        width: f32,
    }

    type FirstView = OnClick<El<String, First, ()>, First, (), fn(&mut First, PointerClick)>;

    type SecondView = OnClick<El<String, Second, ()>, Second, (), fn(&mut Second, PointerClick)>;

    fn first_click(state: &mut First, _event: PointerClick) {
        state.count += 1;
    }

    fn second_click(state: &mut Second, _event: PointerClick) {
        state.count += 10;
    }

    fn first_view(state: &First) -> FirstView {
        on_click(
            el::<_, First, ()>(
                "button",
                format!("first:{}:{:.0}", state.count, state.width),
            ),
            first_click as fn(&mut First, PointerClick),
        )
    }

    fn second_view(state: &Second) -> SecondView {
        on_click(
            el::<_, Second, ()>("div", format!("second:{}:{:.0}", state.count, state.width)),
            second_click as fn(&mut Second, PointerClick),
        )
    }

    fn descriptor(id: &str) -> SurfaceDescriptor {
        SurfaceDescriptor {
            provider_id: ProviderId::from("example"),
            surface_id: SurfaceId::from(id),
            label: id.to_owned(),
            accepted_source: SurfaceSourceShape::One(SourceKindId::from("example.source")),
        }
    }

    fn fresh_dom() -> DomHandle {
        Rc::new(RefCell::new(ScriptedDom::new()))
    }

    fn root_text(dom: &DomHandle, root: NodeId) -> String {
        let dom = dom.borrow();
        let text = dom
            .dom_children(root)
            .find(|node| dom.kind(*node) == NodeKind::Text)
            .expect("surface root has a text child");
        dom.text(text).expect("text child has text").to_owned()
    }

    #[test]
    fn erased_sessions_retain_distinct_runner_state_and_dispatch_independently() {
        let first_dom = fresh_dom();
        let first_runner = GenetAppRunner::new(
            first_dom.clone(),
            first_view,
            First {
                count: 0,
                width: 0.0,
            },
        );
        let first = RunnerSurfaceSession::new(
            descriptor("example.first"),
            first_runner,
            |state: &First| {
                if state.count == 0 {
                    SurfaceAvailability::Available
                } else {
                    SurfaceAvailability::Unavailable(SurfaceUnavailableReason::Locked)
                }
            },
            |state: &mut First, viewport| state.width = viewport.width,
            |_action: ()| Vec::new(),
        );
        let first_root = first.root();

        let second_dom = fresh_dom();
        let second_runner = GenetAppRunner::new(
            second_dom.clone(),
            second_view,
            Second {
                count: 10,
                width: 0.0,
            },
        );
        let second = RunnerSurfaceSession::new(
            descriptor("example.second"),
            second_runner,
            |_state: &Second| SurfaceAvailability::Available,
            |state: &mut Second, viewport| state.width = viewport.width,
            |_action: ()| Vec::new(),
        );
        let second_root = second.root();

        let mut sessions: Vec<Box<dyn RetainedSurfaceSession>> =
            vec![Box::new(first), Box::new(second)];

        assert_eq!(sessions[0].root(), first_root);
        assert_eq!(sessions[1].root(), second_root);
        assert_eq!(
            sessions[0].dispatch(ResolvedSurfaceEvent::Click {
                target: first_root,
                event: PointerClick::at((0.0, 0.0)),
            }),
            vec![SurfaceEffect::Redraw]
        );
        assert_eq!(root_text(&first_dom, first_root), "first:1:0");
        assert_eq!(root_text(&second_dom, second_root), "second:10:0");
        assert_eq!(
            sessions[0].availability(),
            SurfaceAvailability::Unavailable(SurfaceUnavailableReason::Locked)
        );

        assert_eq!(
            sessions[1].sync_viewport(SurfaceViewport {
                width: 240.0,
                height: 160.0,
                scale_factor: 1.0,
            }),
            vec![SurfaceEffect::Redraw]
        );
        assert_eq!(root_text(&first_dom, first_root), "first:1:0");
        assert_eq!(root_text(&second_dom, second_root), "second:10:240");
        assert_eq!(
            sessions[1].sync_viewport(SurfaceViewport {
                width: 240.0,
                height: 160.0,
                scale_factor: 1.0,
            }),
            Vec::<SurfaceEffect>::new()
        );

        sessions[1].dispatch(ResolvedSurfaceEvent::Click {
            target: second_root,
            event: PointerClick::at((0.0, 0.0)),
        });
        assert_eq!(root_text(&first_dom, first_root), "first:1:0");
        assert_eq!(root_text(&second_dom, second_root), "second:20:240");
    }
}
