//! The testharness.js lane: run a subset and report per-subtest results.

use super::*;

/// Phase 3: run testharness.js tests and report per-subtest results.
pub(crate) fn testharness(tests: &[TestCase], args: &Args) {
    let tests_root = Path::new(&args.tests_root);
    let th_path = tests_root.join("resources/testharness.js");
    let testharness_js = match fs::read_to_string(&th_path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("testharness.js not found at {}", th_path.display());
            std::process::exit(2);
        },
    };

    // Server mode (netfetch): connect to / spawn a `wpt serve` so `fetch()` hits a
    // real server, `<script src>` is fetched (`.sub.js` substituted), and the
    // document base URL resolves relative URLs. Disk mode leaves this `None`.
    #[cfg(feature = "netfetch")]
    let server = setup_server(args);
    #[cfg(not(feature = "netfetch"))]
    if args.spawn_server || args.server_base.is_some() {
        eprintln!("server mode (--server-base / --spawn-server) needs `--features netfetch`");
        std::process::exit(2);
    }

    // Boa / the bridge can panic on unimplemented paths; report, don't spam.
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let (mut all_pass, mut with_fail, mut errored, mut no_results, mut skipped) = (0, 0, 0, 0, 0);
    let (mut sub_passed, mut sub_total) = (0usize, 0usize);
    let mut actuals = Vec::new();
    let mut nova_template = if args.engine == harness::Engine::Nova {
        match harness::NovaHarnessTemplate::new(&testharness_js) {
            Ok(template) => Some(template),
            Err(e) => {
                eprintln!("Nova harness template init failed: {e}");
                std::process::exit(2);
            },
        }
    } else {
        None
    };

    for test in tests {
        if test.kind != Kind::Testharness {
            skipped += 1;
            actuals.push(ActualRecord::with_reason(test, "skip", "non-testharness"));
            if args.verbose {
                println!("SKIP  non-testharness {}", test.name());
            }
            continue;
        }
        let ext = test.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("xhtml") || ext.eq_ignore_ascii_case("xht") {
            skipped += 1;
            actuals.push(ActualRecord::with_reason(test, "skip", "xhtml"));
            if args.verbose {
                println!("SKIP  xhtml          {}", test.name());
            }
            continue;
        }
        // Build the testharness HTML: a real .html document's contents, or a
        // synthesized wrapper for a `.any.js` / `.window.js` test.
        let html = {
            #[cfg(feature = "netfetch")]
            if let Some(s) = &server {
                match net::http_get(&s.doc_url(test.name())) {
                    Some(t) => t,
                    None => {
                        errored += 1;
                        actuals.push(ActualRecord::with_reason(
                            test,
                            "error",
                            "fetch-load-failed",
                        ));
                        println!("ERROR fetch   {}", test.name());
                        continue;
                    },
                }
            } else {
                match build_test_html_disk(test) {
                    TestHtml::Html(h) => h,
                    TestHtml::Skip(reason) => {
                        skipped += 1;
                        actuals.push(ActualRecord::with_reason(test, "skip", reason));
                        if args.verbose {
                            println!("SKIP  {reason:16} {}", test.name());
                        }
                        continue;
                    },
                    TestHtml::ReadError => {
                        errored += 1;
                        actuals.push(ActualRecord::with_reason(test, "error", "read-failed"));
                        println!("ERROR read    {}", test.name());
                        continue;
                    },
                }
            }
            #[cfg(not(feature = "netfetch"))]
            {
                match build_test_html_disk(test) {
                    TestHtml::Html(h) => h,
                    TestHtml::Skip(reason) => {
                        skipped += 1;
                        actuals.push(ActualRecord::with_reason(test, "skip", reason));
                        if args.verbose {
                            println!("SKIP  {reason:16} {}", test.name());
                        }
                        continue;
                    },
                    TestHtml::ReadError => {
                        errored += 1;
                        actuals.push(ActualRecord::with_reason(test, "error", "read-failed"));
                        println!("ERROR read    {}", test.name());
                        continue;
                    },
                }
            }
        };

        let base_dir = test.path.parent().unwrap_or(tests_root);
        let disk = harness::DiskLoader {
            base_dir,
            tests_root,
        };
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            // Server mode: a fresh per-test fetch-event channel feeds the drive loop,
            // so deferred fetches settle out of band, mid-flight abort works, and a
            // hung fetch hits the per-test deadline. The shared worker routes replies
            // to this channel; a late reply from a prior test lands on a dropped
            // channel and is harmlessly discarded.
            #[cfg(feature = "netfetch")]
            if let Some(s) = &server {
                let (ev_tx, ev_rx) = std::sync::mpsc::channel::<net::FetchEvent>();
                let doc_url = s.doc_url(test.name());
                let loader = s.loader(&doc_url);
                let handler = net::NetFetchHandler::new(ev_tx);
                let completion = net::ChannelCompletion::new(ev_rx);
                if let Some(template) = nova_template.as_mut() {
                    return template.run_test_with_style(
                        &html,
                        &loader,
                        Some(&doc_url),
                        Some(Box::new(handler)),
                        Some(&completion),
                        args.renderer.harness_style(),
                    );
                }
                return harness::run_test_with_style(
                    &testharness_js,
                    &html,
                    &loader,
                    Some(&doc_url),
                    Some(Box::new(handler)),
                    Some(&completion),
                    args.engine,
                    args.renderer.harness_style(),
                );
            }
            let doc_url = test.disk_doc_url();
            if let Some(template) = nova_template.as_mut() {
                return template.run_test_with_style(
                    &html,
                    &disk,
                    Some(&doc_url),
                    None,
                    None,
                    args.renderer.harness_style(),
                );
            }
            harness::run_test_with_style(
                &testharness_js,
                &html,
                &disk,
                Some(&doc_url),
                None,
                None,
                args.engine,
                args.renderer.harness_style(),
            )
        }));
        let name = test.name();

        match result {
            Err(payload) => {
                errored += 1;
                actuals.push(ActualRecord::with_reason(test, "error", "panic"));
                let message = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic");
                println!("ERROR panic   {name}  ({message})");
            },
            Ok(harness::HarnessOutcome::Threw(msg)) => {
                errored += 1;
                actuals.push(ActualRecord::with_reason(test, "error", "evaluation-threw"));
                println!("ERROR {name}  ({msg})");
            },
            Ok(harness::HarnessOutcome::Ran(results)) => {
                let total = results.len();
                let passed = results.iter().filter(|r| r.passed()).count();
                sub_passed += passed;
                sub_total += total;
                if total == 0 {
                    no_results += 1;
                    actuals.push(ActualRecord::with_reason(test, "no-results", "no-subtests"));
                    if args.verbose {
                        println!("NORES {name}  (harness ran but reported no subtests)");
                    }
                } else if passed == total {
                    all_pass += 1;
                    actuals.push(ActualRecord::with_subtests(test, "pass", &results));
                    if args.verbose {
                        println!("PASS  {name}  ({passed}/{total})");
                    }
                } else {
                    with_fail += 1;
                    actuals.push(ActualRecord::with_subtests(test, "fail", &results));
                    println!("FAIL  {name}  ({passed}/{total} subtests)");
                    if args.verbose {
                        for r in results.iter().filter(|r| !r.passed()) {
                            let msg = r.message.as_deref().unwrap_or("");
                            println!("        [{}] {} {msg}", r.status, r.name);
                        }
                    }
                }
            },
        }
    }

    panic::set_hook(prev);

    println!(
        "\ntestharness [{}]: {all_pass} all-pass, {with_fail} with-failures, {errored} errored, \
         {no_results} no-results, {skipped} skipped (of {} files); \
         subtests {sub_passed}/{sub_total} passed",
        args.engine.label(),
        tests.len(),
    );
    finish_expectations(args, "testharness", &actuals);
}
