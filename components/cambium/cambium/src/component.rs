/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A state-owning boundary for reusable Cambium controls.
//!
//! A component receives ordinary parent-owned props, retains its own local
//! state, and emits a typed event which the parent translates into its action
//! vocabulary. The erasure is deliberately narrow: [`AnyView`] hides the
//! concrete child tree while `Local` and `Event` stay statically typed.
//!
//! This belongs in Cambium rather than Meristem. It fixes the backend to
//! [`GenetCtx`] and [`GenetElement`], and its optional probe identity is a DOM
//! attribute. Meristem remains the renderer-neutral diff and message core.

use std::marker::PhantomData;

use layout_dom_api::{LayoutDom, LayoutDomMut};
use meristem::{AnyView, MessageCtx, MessageResult, Mut, View, ViewMarker};

use crate::{GenetCtx, GenetElement, attr_qual};

/// The DOM attribute stamped by [`Component::probe_id`].
///
/// It is automation metadata only. Keyed sequence identity and Meristem message
/// routing continue to use their existing typed identities.
pub const COMPONENT_PROBE_ATTR: &str = "data-cambium-component";

/// The erased child tree retained inside a [`Component`].
pub type ComponentView<Local, Event> = Box<dyn AnyView<Local, Event, GenetCtx, GenetElement>>;

/// A reusable control boundary with props in, retained local state, and typed
/// events out.
///
/// `reconcile` runs on every parent rebuild and receives both the previous and
/// current props. It should update only parent-controlled axes of `Local`.
/// Component-owned interaction state should remain untouched when its props did
/// not change.
#[must_use = "View values do nothing unless provided to a Cambium runner"]
pub struct Component<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent> {
    props: Props,
    init: Init,
    reconcile: Reconcile,
    body: Body,
    on_event: OnEvent,
    probe_id: Option<String>,
    // A stored fn pointer rather than a `Props: PartialEq` bound on the View
    // impl: only `memo()` requires equality, so un-memoized components keep
    // working with non-comparable props (closures, trait objects).
    props_unchanged: Option<fn(&Props, &Props) -> bool>,
    phantom: PhantomData<fn(Local, Event, State) -> Action>,
}

/// Construct a state-owning Cambium component.
///
/// `body` receives the current props and local state and returns one erased
/// child view. `on_event` is the sole point where the child's event vocabulary
/// meets the parent's state and action vocabulary.
pub fn component<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>(
    props: Props,
    init: Init,
    reconcile: Reconcile,
    body: Body,
    on_event: OnEvent,
) -> Component<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>
where
    Local: 'static,
    Event: 'static,
    State: 'static,
    Action: 'static,
    Init: Fn(&Props) -> Local + 'static,
    Reconcile: Fn(&Props, &Props, &mut Local) + 'static,
    Body: Fn(&Props, &Local) -> ComponentView<Local, Event> + 'static,
    OnEvent: Fn(&mut State, Event) -> Action + 'static,
{
    Component {
        props,
        init,
        reconcile,
        body,
        on_event,
        probe_id: None,
        props_unchanged: None,
        phantom: PhantomData,
    }
}

impl<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>
    Component<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>
{
    /// Stamp a caller-owned, DOM-visible identity on the component root for
    /// `genet-probe` attribute selectors.
    #[must_use]
    pub fn probe_id(mut self, id: impl Into<String>) -> Self {
        self.probe_id = Some(id.into());
        self
    }

    /// Skip `reconcile`, `body`, and the child rebuild when the props are
    /// unchanged and no message has touched the local state since the last
    /// rebuild.
    ///
    /// This is the memo boundary: correct only because a component's child
    /// tree depends on nothing but its props and its local state. A message
    /// delivered to the subtree marks the local state dirty, so an
    /// interaction always renders even under equal props.
    #[must_use]
    pub fn memo(mut self) -> Self
    where
        Props: PartialEq,
    {
        self.props_unchanged = Some(|prev, next| prev == next);
        self
    }
}

/// Retained state for [`Component`]. Public only because it is a `View`
/// associated type.
#[doc(hidden)]
pub struct ComponentState<Local: 'static, Event: 'static> {
    local: Local,
    child: ComponentView<Local, Event>,
    child_state: <ComponentView<Local, Event> as View<Local, Event, GenetCtx>>::ViewState,
    /// Set when a message reaches this subtree (it may have mutated `local`),
    /// cleared when a rebuild has run `body`. Gates the `memo()` skip.
    local_dirty: bool,
}

impl<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent> ViewMarker
    for Component<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>
{
}

impl<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>
    View<State, Action, GenetCtx>
    for Component<Props, Local, Event, State, Action, Init, Reconcile, Body, OnEvent>
where
    Props: 'static,
    Local: 'static,
    Event: 'static,
    State: 'static,
    Action: 'static,
    Init: Fn(&Props) -> Local + 'static,
    Reconcile: Fn(&Props, &Props, &mut Local) + 'static,
    Body: Fn(&Props, &Local) -> ComponentView<Local, Event> + 'static,
    OnEvent: Fn(&mut State, Event) -> Action + 'static,
{
    type Element = GenetElement;
    type ViewState = ComponentState<Local, Event>;

    fn build(
        &self,
        ctx: &mut GenetCtx,
        _app_state: &mut State,
    ) -> (Self::Element, Self::ViewState) {
        let mut local = (self.init)(&self.props);
        let child = (self.body)(&self.props, &local);
        let (element, child_state) = child.build(ctx, &mut local);
        apply_probe_id(&element.dom, element.node, self.probe_id.as_deref());
        (
            element,
            ComponentState {
                local,
                child,
                child_state,
                local_dirty: false,
            },
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        state: &mut Self::ViewState,
        ctx: &mut GenetCtx,
        mut element: Mut<'_, Self::Element>,
        _app_state: &mut State,
    ) {
        if let Some(unchanged) = self.props_unchanged {
            if !state.local_dirty && unchanged(&prev.props, &self.props) {
                // The probe stamp is cheap and idempotent, and `probe_id` is
                // not covered by the props comparison, so keep it honest.
                apply_probe_id(&element.dom, *element.node, self.probe_id.as_deref());
                return;
            }
        }
        (self.reconcile)(&prev.props, &self.props, &mut state.local);
        let child = (self.body)(&self.props, &state.local);
        child.rebuild(
            &state.child,
            &mut state.child_state,
            ctx,
            element.reborrow_mut(),
            &mut state.local,
        );
        state.child = child;
        state.local_dirty = false;
        apply_probe_id(&element.dom, *element.node, self.probe_id.as_deref());
    }

    fn teardown(
        &self,
        state: &mut Self::ViewState,
        ctx: &mut GenetCtx,
        element: Mut<'_, Self::Element>,
    ) {
        state.child.teardown(&mut state.child_state, ctx, element);
    }

    fn message(
        &self,
        state: &mut Self::ViewState,
        message: &mut MessageCtx,
        element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<Action> {
        // Any delivered message may have mutated `local` through a handler, so
        // the next rebuild must run `body` even under `memo()` with equal props.
        state.local_dirty = true;
        match state
            .child
            .message(&mut state.child_state, message, element, &mut state.local)
        {
            MessageResult::Action(event) => {
                MessageResult::Action((self.on_event)(app_state, event))
            },
            MessageResult::RequestRebuild => MessageResult::RequestRebuild,
            MessageResult::Nop => MessageResult::Nop,
            MessageResult::Stale => MessageResult::Stale,
        }
    }
}

fn apply_probe_id(dom: &crate::DomHandle, node: genet_scripted_dom::NodeId, id: Option<&str>) {
    let mut dom = dom.borrow_mut();
    let qual = attr_qual(COMPONENT_PROBE_ATTR);
    // This runs on every rebuild, and `set_attribute` records a mutation even
    // for an unchanged value (faithful MutationObserver semantics), which
    // would feed a spurious restyle per rebuild. Skip when already stamped.
    if dom.attribute(node, &qual.ns, &qual.local) == id {
        return;
    }
    match id {
        Some(id) => dom.set_attribute(node, qual, id),
        None => dom.remove_attribute(node, qual),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use genet_scripted_dom::ScriptedDom;
    use layout_dom_api::{LayoutDom, LocalName, Namespace};

    use super::*;
    use crate::{
        Action, CommandEvent, CommandItem, CommandState, DomHandle, GenetAppRunner, Key, KeyEvent,
        NamedKey, command_picker,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct PickerProps {
        label: String,
        items: Vec<CommandItem>,
    }

    #[derive(Debug)]
    struct AppState {
        label: String,
        activated: Vec<Vec<usize>>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum AppEvent {
        Activated(Vec<usize>),
        Dismissed,
    }

    impl Action for AppEvent {}

    fn items() -> Vec<CommandItem> {
        vec![
            CommandItem::new("Open"),
            CommandItem::new("Export"),
            CommandItem::new("Close").disabled_because("Keep one document open"),
        ]
    }

    fn init_picker(props: &PickerProps) -> CommandState {
        CommandState::default()
            .with_id("component-picker")
            .with_label(props.label.clone())
    }

    fn reconcile_picker(prev: &PickerProps, next: &PickerProps, local: &mut CommandState) {
        if prev.label != next.label {
            local.label.clone_from(&next.label);
        }
    }

    fn picker_body(
        props: &PickerProps,
        local: &CommandState,
    ) -> ComponentView<CommandState, CommandEvent> {
        Box::new(command_picker(local, &props.items))
    }

    fn lower_picker_event(app: &mut AppState, event: CommandEvent) -> AppEvent {
        match event {
            CommandEvent::Activate(path) => {
                app.activated.push(path.clone());
                AppEvent::Activated(path)
            },
            CommandEvent::Dismiss => AppEvent::Dismissed,
        }
    }

    fn app_view(
        app: &AppState,
    ) -> impl View<AppState, AppEvent, GenetCtx, Element = GenetElement> + use<> {
        component(
            PickerProps {
                label: app.label.clone(),
                items: items(),
            },
            init_picker,
            reconcile_picker,
            picker_body,
            lower_picker_event,
        )
        .probe_id("primary-picker")
    }

    fn attr<'a>(
        dom: &'a ScriptedDom,
        node: genet_scripted_dom::NodeId,
        name: &str,
    ) -> Option<&'a str> {
        dom.attribute(node, &Namespace::from(""), &LocalName::from(name))
    }

    #[test]
    fn component_retains_local_state_reconciles_props_and_emits_parent_events() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner = GenetAppRunner::new(
            dom.clone(),
            app_view,
            AppState {
                label: "Actions".into(),
                activated: Vec::new(),
            },
        );
        let root = runner.root();
        assert_eq!(
            attr(&dom.borrow(), root, COMPONENT_PROBE_ATTR),
            Some("primary-picker")
        );

        runner.set_focus(Some(root));
        runner.dispatch_key(KeyEvent::new(Key::Named(NamedKey::ArrowDown)));
        assert_eq!(
            attr(&dom.borrow(), root, "aria-activedescendant"),
            Some("component-picker-item-1"),
            "component-owned selection survives the dispatch rebuild"
        );

        runner.update(|app| app.label = "Commands".into());
        assert_eq!(attr(&dom.borrow(), root, "aria-label"), Some("Commands"));
        assert_eq!(
            attr(&dom.borrow(), root, "aria-activedescendant"),
            Some("component-picker-item-1"),
            "reconciling a controlled label must not reset local selection"
        );

        let events = runner.dispatch_key(KeyEvent::new(Key::Named(NamedKey::Enter)));
        assert_eq!(events, [AppEvent::Activated(vec![1])]);
        assert_eq!(runner.state().activated, [vec![1]]);
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Bump;
    impl Action for Bump {}

    #[test]
    fn memo_skips_body_on_equal_props_until_a_message_dirties_local_state() {
        use std::cell::Cell;

        use crate::{PointerClick, button};

        let body_runs = Rc::new(Cell::new(0_u32));
        let seen = body_runs.clone();
        let app_view = move |step: &i64| {
            let seen = seen.clone();
            component(
                *step,
                |_: &i64| 0_i64,
                |_: &i64, _: &i64, _: &mut i64| {},
                move |_: &i64, _: &i64| -> ComponentView<i64, Bump> {
                    seen.set(seen.get() + 1);
                    Box::new(button("+", |count: &mut i64, _: PointerClick| {
                        *count += 1;
                    }))
                },
                |_: &mut i64, _: Bump| {},
            )
            .memo()
        };

        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner = GenetAppRunner::new(dom.clone(), app_view, 1_i64);
        assert_eq!(body_runs.get(), 1, "build runs the body once");

        runner.update(|_| {});
        assert_eq!(
            body_runs.get(),
            1,
            "equal props and clean local skip the body"
        );

        runner.update(|step| *step += 1);
        assert_eq!(body_runs.get(), 2, "changed props run the body");

        let root = runner.root();
        runner.dispatch_click(root, PointerClick::at((2.0, 2.0)));
        assert_eq!(
            body_runs.get(),
            3,
            "a message marks local dirty, so the dispatch rebuild runs the body"
        );

        runner.update(|_| {});
        assert_eq!(
            body_runs.get(),
            3,
            "the skip resumes once local is clean again"
        );
    }
}
