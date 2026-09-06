/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(feature = "livery")]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use genet_scripted::{LiveryScriptedDocument, ResourceFetcher, ScriptedDocumentOptions};
use script_engine_api::ScriptEngine;
use script_runtime_api::{FetchEvent, FetchHandler, FetchOutcome, FetchRequest};

struct EmptyResources;
impl ResourceFetcher for EmptyResources {
    fn fetch(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Default)]
struct Delivery {
    requests: Vec<(u64, String)>,
    events: VecDeque<FetchEvent>,
    pending: bool,
    canceled: Vec<u64>,
    retired: bool,
}

struct Deferred(Rc<RefCell<Delivery>>);
impl FetchHandler for Deferred {
    fn start(&self, id: u64, request: FetchRequest) -> Option<FetchOutcome> {
        let mut delivery = self.0.borrow_mut();
        delivery.requests.push((id, request.url));
        delivery.pending = true;
        None
    }

    fn poll(&self, max_events: usize) -> Vec<FetchEvent> {
        let mut delivery = self.0.borrow_mut();
        let count = max_events.min(delivery.events.len());
        let events = delivery.events.drain(..count).collect::<Vec<_>>();
        if !events.is_empty() {
            delivery.pending = false;
        }
        events
    }

    fn has_pending(&self) -> bool {
        self.0.borrow().pending
    }

    fn cancel(&self, id: u64) {
        let mut delivery = self.0.borrow_mut();
        delivery.canceled.push(id);
        delivery.pending = false;
    }

    fn cancel_all(&self) {
        let mut delivery = self.0.borrow_mut();
        delivery.retired = true;
        delivery.pending = false;
        delivery.events.clear();
    }
}

fn document<E: ScriptEngine>(
    delivery: &Rc<RefCell<Delivery>>,
    script: &str,
) -> LiveryScriptedDocument<E> {
    let html = format!(
        "<style>p {{ display:block }}</style><p id='result'>waiting</p><script>{script}</script>"
    );
    LiveryScriptedDocument::from_body_with_options(
        &html,
        EmptyResources,
        "https://example.test/app/index.html",
        ScriptedDocumentOptions {
            fetch: Some(Box::new(Deferred(delivery.clone()))),
            ..Default::default()
        },
    )
    .expect("document with capabilities")
}

fn complete(delivery: &Rc<RefCell<Delivery>>, body: &str) {
    let id = delivery.borrow().requests[0].0;
    delivery
        .borrow_mut()
        .events
        .push_back(FetchEvent::Complete {
            id,
            outcome: FetchOutcome {
                network_error: false,
                status: 200,
                status_text: "OK".to_owned(),
                response_type: "basic".to_owned(),
                url: "https://example.test/data.json".to_owned(),
                redirected: false,
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: body.as_bytes().to_vec(),
            },
        });
}

fn fetch_updates_live_layout<E: ScriptEngine>() {
    let delivery = Rc::new(RefCell::new(Delivery::default()));
    let mut doc = document::<E>(
        &delivery,
        "fetch('/data.json').then(function(r) { return r.json(); }).then(function(data) { document.getElementById('result').textContent = data.message; });",
    );
    assert_eq!(
        delivery.borrow().requests.len(),
        1,
        "first script reached host"
    );
    assert_eq!(
        delivery.borrow().requests[0].1,
        "https://example.test/data.json"
    );
    assert!(doc.has_pending_work(), "network alone keeps session awake");
    complete(&delivery, "{\"message\":\"loaded from host\"}");
    doc.pump(1.0);
    assert!(
        doc.dom_snapshot().contains(">loaded from host</p>"),
        "{:?}",
        doc.console()
    );
    doc.frame(400, 200);
    assert!(
        doc.text_target("loaded from host").is_some(),
        "fetched text reached layout"
    );
    assert!(!doc.has_pending_work());
}

fn failure_and_abort<E: ScriptEngine>() {
    let delivery = Rc::new(RefCell::new(Delivery::default()));
    let mut doc = document::<E>(
        &delivery,
        "fetch('/fail').catch(function(e) { document.getElementById('result').textContent = e.name; });",
    );
    let id = delivery.borrow().requests[0].0;
    delivery.borrow_mut().events.push_back(FetchEvent::Failed {
        id,
        message: "connection failed".to_owned(),
    });
    doc.pump(1.0);
    assert!(
        doc.dom_snapshot().contains(">TypeError</p>"),
        "{:?}",
        doc.console()
    );

    let delivery = Rc::new(RefCell::new(Delivery::default()));
    let mut doc = document::<E>(
        &delivery,
        "var controller = new AbortController(); fetch('/slow', {signal:controller.signal}).then(function() { document.getElementById('result').textContent = 'unexpected success'; }).catch(function(e) { document.getElementById('result').textContent = e.name; });",
    );
    doc.evaluate("controller.abort()").unwrap();
    assert_eq!(delivery.borrow().canceled.len(), 1);
    complete(&delivery, "{}");
    doc.pump(1.0);
    assert!(
        doc.dom_snapshot().contains(">AbortError</p>"),
        "{:?}",
        doc.console()
    );
}

fn replacement_cannot_receive_old_delivery<E: ScriptEngine>() {
    let old = Rc::new(RefCell::new(Delivery::default()));
    let doc = document::<E>(
        &old,
        "fetch('/old').then(function() { document.getElementById('result').textContent='old'; });",
    );
    drop(doc);
    assert!(old.borrow().retired);
    let new = Rc::new(RefCell::new(Delivery::default()));
    let mut doc = document::<E>(
        &new,
        "fetch('/new').then(function(r) {return r.json();}).then(function(x) {document.getElementById('result').textContent=x.message;});",
    );
    assert_eq!(
        old.borrow().requests[0].0,
        new.borrow().requests[0].0,
        "exercise reused local ID"
    );
    complete(&old, "{\"message\":\"stale\"}");
    doc.pump(1.0);
    assert!(doc.dom_snapshot().contains(">waiting</p>"));
    complete(&new, "{\"message\":\"replacement\"}");
    doc.pump(2.0);
    assert!(doc.dom_snapshot().contains(">replacement</p>"));
}

#[test]
fn boa_host_fetch_updates_live_layout() {
    fetch_updates_live_layout::<script_engine_boa::BoaEngine>();
}
#[test]
fn boa_host_fetch_failure_and_abort() {
    failure_and_abort::<script_engine_boa::BoaEngine>();
}
#[test]
fn boa_host_fetch_replacement_isolation() {
    replacement_cannot_receive_old_delivery::<script_engine_boa::BoaEngine>();
}

#[cfg(all(feature = "scripted-nova", target_pointer_width = "64"))]
mod vano {
    use super::*;
    use script_engine_nova::NovaEngine;
    #[test]
    fn host_fetch_updates_live_layout() {
        fetch_updates_live_layout::<NovaEngine>();
    }
    #[test]
    fn host_fetch_failure_and_abort() {
        failure_and_abort::<NovaEngine>();
    }
    #[test]
    fn host_fetch_replacement_isolation() {
        replacement_cannot_receive_old_delivery::<NovaEngine>();
    }
}
