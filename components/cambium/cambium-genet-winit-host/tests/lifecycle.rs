// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The long-lived-host seam: worker wake and negotiated close.

use std::thread;

use cambium::{AnyView, GenetCtx, GenetElement, el, text};
use cambium_genet_winit_host::{
    CloseDisposition, CloseRequest, Harness, HostHooks, Init, inert_hooks,
};

type Child = Box<dyn AnyView<State, (), GenetCtx, GenetElement>>;
type Logic = fn(&State) -> Child;

#[derive(Default)]
struct State {
    wakes: usize,
    closes: Vec<CloseRequest>,
}

fn root(_state: &State) -> Child {
    Box::new(el("div", text("lifecycle")))
}

fn harness_with(
    configure: impl FnOnce(&mut HostHooks<State, Logic, Child>),
) -> Harness<State, Logic, Child> {
    let mut hooks: HostHooks<State, Logic, Child> = inert_hooks();
    configure(&mut hooks);
    Harness::with_hooks(
        Init {
            state: State::default(),
            logic: root as Logic,
            sheet: String::new(),
        },
        hooks,
    )
}

#[test]
fn armillary_shaped_wake_coalesces_and_drains_without_frames() {
    let mut host = harness_with(|hooks| {
        hooks.after_wake = Box::new(|ctx| {
            ctx.runner.update(|state| state.wakes += 1);
        });
    });

    // This is exactly the `Arc<dyn Fn() + Send + Sync>` shape Armillary takes.
    let wake = host.wake().callback();
    let second = wake.clone();
    thread::spawn(move || wake()).join().expect("wake thread");
    second();

    assert!(host.process_wake(), "one queued wake reaches the UI thread");
    assert_eq!(host.state().wakes, 1, "two worker sends coalesce");
    assert!(!host.process_wake(), "a drained wake does not spin frames");
}

#[test]
fn native_and_command_close_share_policy_and_can_hide_or_exit() {
    let mut host = harness_with(|hooks| {
        hooks.close_request = Box::new(|ctx, request| {
            ctx.runner.update(|state| state.closes.push(request));
            match request {
                CloseRequest::Native => CloseDisposition::Hide,
                CloseRequest::Command => CloseDisposition::Exit,
            }
        });
    });

    host.request_close(CloseRequest::Native);
    assert!(
        host.hidden(),
        "native close may hide without ending the host"
    );
    assert!(!host.close_requested(), "hide is not exit");

    host.commands().show();
    host.after_dispatch();
    assert!(
        !host.hidden(),
        "a later app command restores the retained root"
    );

    host.commands().close();
    host.after_dispatch();
    assert!(
        host.close_requested(),
        "app close is negotiated through the hook"
    );
    assert_eq!(
        host.state().closes,
        vec![CloseRequest::Native, CloseRequest::Command],
    );
}

#[test]
fn application_can_keep_a_close_request_visible() {
    let mut host = harness_with(|hooks| {
        hooks.close_request = Box::new(|ctx, request| {
            ctx.runner.update(|state| state.closes.push(request));
            CloseDisposition::KeepVisible
        });
    });

    host.request_close(CloseRequest::Native);
    assert!(!host.hidden());
    assert!(!host.close_requested());
    assert_eq!(host.state().closes, vec![CloseRequest::Native]);
}
