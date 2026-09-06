/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(feature = "scripted")]

use std::sync::{Arc, Mutex};

use document_session_api::session_engine::{SessionEngine, SessionSpawnRequest};
use genet_documents::{ResourceFetcher, ScriptedSessionEngine};
use genet_scripted::ScriptedDocumentOptions;
use script_runtime_api::{FetchHandler, FetchOutcome, FetchRequest};

#[derive(Clone)]
struct EmptyResources;
impl ResourceFetcher for EmptyResources {
    fn fetch(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

struct PageFetch(String);
impl FetchHandler for PageFetch {
    fn fetch(&self, _: FetchRequest) -> FetchOutcome {
        FetchOutcome {
            network_error: false,
            status: 200,
            status_text: "OK".to_owned(),
            response_type: "basic".to_owned(),
            url: "https://example.test/data".to_owned(),
            redirected: false,
            headers: Vec::new(),
            body: self.0.as_bytes().to_vec(),
        }
    }
}

#[test]
fn session_factory_installs_fresh_capabilities_before_authored_scripts() {
    let addresses = Arc::new(Mutex::new(Vec::new()));
    let observed = addresses.clone();
    let engine = ScriptedSessionEngine::<script_engine_boa::BoaEngine, _>::new(
        "genet.scripted",
        EmptyResources,
    )
    .with_options_factory(move |address| {
        let mut addresses = observed.lock().unwrap();
        addresses.push(address.to_owned());
        Ok(ScriptedDocumentOptions {
            fetch: Some(Box::new(PageFetch(format!("page {}", addresses.len())))),
            ..Default::default()
        })
    });
    let html = "<p id='result'>waiting</p><script>fetch('/data').then(function(r) {return r.text();}).then(function(t) {document.getElementById('result').textContent=t;});</script>";
    let mut first = engine
        .spawn(&SessionSpawnRequest::new("https://example.test/one#:~:text=secret").with_body(html))
        .unwrap();
    first.frame(320, 240);
    assert!(first.text_target("page 1").is_some());
    let mut second = engine
        .spawn(&SessionSpawnRequest::new("https://example.test/two").with_body(html))
        .unwrap();
    second.frame(320, 240);
    assert!(second.text_target("page 2").is_some());
    assert_eq!(
        *addresses.lock().unwrap(),
        ["https://example.test/one#", "https://example.test/two"]
    );
}

#[test]
fn failed_capability_construction_rejects_spawn() {
    let engine = ScriptedSessionEngine::<script_engine_boa::BoaEngine, _>::new(
        "genet.scripted",
        EmptyResources,
    )
    .with_options_factory(|_| Err("host unavailable".to_owned()));
    let request = SessionSpawnRequest::new("https://example.test/").with_body("<p>body</p>");
    match engine.spawn(&request) {
        Err(document_session_api::session_engine::SessionError::SpawnFailed(message)) => {
            assert_eq!(message, "host unavailable");
        },
        _ => panic!("host initialization failure must prevent document execution"),
    }
}

#[derive(Clone)]
struct RedirectResources;
impl ResourceFetcher for RedirectResources {
    fn fetch(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn fetch_response(&self, url: &str) -> Option<genet_host_api::ResourceResponse> {
        assert_eq!(url, "https://initial.test/start");
        Some(genet_host_api::ResourceResponse::new(
            "https://destination.test/app",
            b"<p id='result'>waiting</p><script>fetch('/data').then(function(r){return r.text();}).then(function(t){document.getElementById('result').textContent=t;});</script>".to_vec(),
        ))
    }
}

#[test]
fn redirected_document_binds_capabilities_to_final_origin() {
    let addresses = Arc::new(Mutex::new(Vec::new()));
    let observed = addresses.clone();
    let engine = ScriptedSessionEngine::<script_engine_boa::BoaEngine, _>::new(
        "genet.scripted",
        RedirectResources,
    )
    .with_options_factory(move |address| {
        observed.lock().unwrap().push(address.to_owned());
        Ok(ScriptedDocumentOptions {
            fetch: Some(Box::new(PageFetch("redirected page".to_owned()))),
            ..Default::default()
        })
    });
    let mut session = engine
        .spawn(&SessionSpawnRequest::new(
            "https://initial.test/start#:~:text=secret",
        ))
        .unwrap();
    // Removing a Text Directive preserves the explicitly empty fragment.
    assert_eq!(
        *addresses.lock().unwrap(),
        ["https://destination.test/app#"]
    );
    session.frame(320, 240);
    assert!(session.text_target("redirected page").is_some());
}
