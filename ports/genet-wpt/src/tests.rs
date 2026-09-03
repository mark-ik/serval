// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use super::*;

fn solid_image(width: u32, height: u32, value: u8) -> render::Image {
    image::RgbaImage::from_pixel(width, height, image::Rgba([value, value, value, 255]))
}

#[test]
fn fuzzy_matching_checks_max_difference_and_total_pixels_independently() {
    let viewport = render::RenderViewport::new(800, 600, 1.0).expect("default viewport");
    assert_eq!(fuzz_floor_pixels(viewport), 4_800);

    let exact = solid_image(2, 1, 0);
    assert!(images_match(&exact, &exact, None));

    let one_low_delta =
        image::RgbaImage::from_vec(2, 1, vec![1, 1, 1, 255, 0, 0, 0, 255]).expect("two pixels");
    assert!(images_match(&exact, &one_low_delta, Some(((0, 1), (0, 1)))));

    let two_low_deltas = solid_image(2, 1, 1);
    assert!(!images_match(
        &exact,
        &two_low_deltas,
        Some(((0, 1), (0, 1)))
    ));

    let one_large_delta =
        image::RgbaImage::from_vec(2, 1, vec![2, 0, 0, 255, 0, 0, 0, 255]).expect("two pixels");
    assert!(!images_match(
        &exact,
        &one_large_delta,
        Some(((0, 1), (0, 1)))
    ));
    assert!(!images_match(
        &exact,
        &solid_image(1, 1, 0),
        Some(((0, 255), (0, 2)))
    ));
}

#[test]
fn fuzzy_matching_honors_lower_bounds_and_wpt_zero_exceptions() {
    let exact = solid_image(2, 1, 0);
    let one_low_delta =
        image::RgbaImage::from_vec(2, 1, vec![1, 1, 1, 255, 0, 0, 0, 255]).expect("two pixels");
    assert!(!images_match(
        &exact,
        &one_low_delta,
        Some(((2, 3), (2, 3)))
    ));
    assert!(!images_match(&exact, &exact, Some(((1, 3), (1, 3)))));
    assert!(images_match(&exact, &exact, Some(((1, 3), (0, 3)))));
}

#[test]
fn fuzzy_content_accepts_reference_keys_and_clamps_max_difference() {
    assert_eq!(
        parse_fuzzy_content("ref.html:maxDifference=0-2;totalPixels=0-100"),
        Some(((0, 2), (0, 100)))
    );
    assert_eq!(
        parse_fuzzy_content("https://web-platform.test/ref.html:70000;9"),
        Some(((u16::MAX, u16::MAX), (9, 9)))
    );
}

#[test]
fn checked_reference_verification_is_scoped_and_exact() {
    let verification =
        ReferenceVerification::parse(CHECKED_REFERENCE_VERIFICATION, "checked fixture", "livery")
            .expect("checked verification map parses");
    assert_eq!(verification.scopes.len(), 2);
    assert_eq!(verification.tests.len(), 20);
    assert_eq!(
        verification.reason_for("css/css-multicol/example.html"),
        Some("reference-unverified")
    );
    assert_eq!(
        verification
            .reason_for("css/css-position/multicol/static-position/vrl-ltr-ltr-in-multicol.html"),
        Some("reference-unverified")
    );
    assert_eq!(
        verification.reason_for("css/css-position/position-relative-001.html"),
        None
    );
}

#[test]
fn reference_verification_rejects_duplicates_and_renderer_drift() {
    let duplicate = r#"{
        "version": 1,
        "renderer": "livery",
        "reason": "reference-unverified",
        "source": "fixture",
        "scopes": ["css/example/", "css/example/"],
        "tests": []
    }"#;
    let error = ReferenceVerification::parse(duplicate, "fixture", "livery")
        .err()
        .expect("duplicate scope is invalid");
    assert!(error.contains("repeats scope"), "{error}");

    let wrong_reason = r#"{
        "version": 1,
        "renderer": "livery",
        "reason": "known-gap",
        "source": "fixture",
        "scopes": [],
        "tests": []
    }"#;
    let error = ReferenceVerification::parse(wrong_reason, "fixture", "livery")
        .err()
        .expect("an unrecognized reason is invalid");
    assert!(error.contains("reason `reference-unverified`"), "{error}");

    let error =
        ReferenceVerification::parse(CHECKED_REFERENCE_VERIFICATION, "checked fixture", "stylo")
            .err()
            .expect("renderer mismatch is invalid");
    assert!(error.contains("records renderer `livery`"), "{error}");
}

#[test]
fn about_blank_reference_is_an_empty_html_document() {
    let tests_root = Path::new("tests/wpt/tests");
    let (path, html) = resolve_reftest_reference(
        Path::new("tests/wpt/tests/css/example.html"),
        "about:blank#fragment",
        MatchKind::Match,
        tests_root,
    )
    .expect("about:blank is resolvable")
    .expect("about:blank is a built-in reference");

    assert_eq!(path.parent(), Some(tests_root));
    assert_eq!(html, ABOUT_BLANK_DOCUMENT);
    assert!(about_blank_reference("refs/blank.html", tests_root).is_none());
}

fn temp_expectations_path(name: &str) -> String {
    std::env::temp_dir()
        .join(format!("genet-wpt-{name}-{}.json", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

fn test_result_metadata<'a>(renderer: &'a str, subset: &'a str) -> ResultFileMetadata<'a> {
    ResultFileMetadata {
        command: "testharness",
        engine: "boa",
        renderer,
        subset,
        manifest_sha256: Some("manifest"),
        runner_sha256: Some("runner"),
        policy: ExpectationPolicy::Exact,
    }
}

#[test]
fn expectations_accept_exact_statuses() {
    let path = temp_expectations_path("exact");
    let actuals = vec![
        ActualRecord {
            test: "dom/example-a.html".into(),
            status: "pass",
            reason: None,
            subtests: None,
            subtest_results: None,
        },
        ActualRecord {
            test: "dom/example-b.html".into(),
            status: "fail",
            reason: None,
            subtests: None,
            subtest_results: None,
        },
    ];
    write_expectations(&path, test_result_metadata("stylo", "dom"), &actuals)
        .expect("write expectations");
    check_expectations(&path, "stylo", &actuals).expect("expectations match exactly");
    let _ = fs::remove_file(path);
}

#[test]
fn expectations_reject_changed_statuses() {
    let path = temp_expectations_path("changed");
    let expected = vec![ActualRecord {
        test: "dom/example.html".into(),
        status: "pass",
        reason: None,
        subtests: None,
        subtest_results: None,
    }];
    write_expectations(&path, test_result_metadata("stylo", "dom"), &expected)
        .expect("write expectations");
    let actual = vec![ActualRecord {
        test: "dom/example.html".into(),
        status: "fail",
        reason: None,
        subtests: None,
        subtest_results: None,
    }];
    let err =
        check_expectations(&path, "stylo", &actual).expect_err("changed status is unexpected");
    assert!(err.contains("expected pass, got fail"), "{err}");
    let _ = fs::remove_file(path);
}

#[test]
fn expectations_accept_pinned_reasons_and_legacy_strings() {
    let path = temp_expectations_path("reason");
    let expected = vec![
        ActualRecord {
            test: "dom/example-a.html".into(),
            status: "skip",
            reason: Some("worker-only".into()),
            subtests: None,
            subtest_results: None,
        },
        ActualRecord {
            test: "dom/example-b.html".into(),
            status: "pass",
            reason: None,
            subtests: None,
            subtest_results: None,
        },
    ];
    write_expectations(&path, test_result_metadata("stylo", "dom"), &expected)
        .expect("write expectations");
    check_expectations(&path, "stylo", &expected).expect("expectations match exact reason");
    let _ = fs::remove_file(path);
}

#[test]
fn expectations_reject_changed_pinned_reason() {
    let path = temp_expectations_path("reason-changed");
    let expected = vec![ActualRecord {
        test: "dom/example.html".into(),
        status: "skip",
        reason: Some("worker-only".into()),
        subtests: None,
        subtest_results: None,
    }];
    write_expectations(&path, test_result_metadata("stylo", "dom"), &expected)
        .expect("write expectations");
    let actual = vec![ActualRecord {
        test: "dom/example.html".into(),
        status: "skip",
        reason: Some("xhtml".into()),
        subtests: None,
        subtest_results: None,
    }];
    let err =
        check_expectations(&path, "stylo", &actual).expect_err("changed reason is unexpected");
    assert!(
        err.contains("expected skip (worker-only), got skip (xhtml)"),
        "{err}"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn expectations_pin_subtest_counts_within_a_failing_file() {
    let path = temp_expectations_path("subtests");
    let expected = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((40, 47)),
        subtest_results: None,
    }];
    write_expectations(&path, test_result_metadata("livery", "css"), &expected)
        .expect("write expectations");

    // The same counts pass.
    check_expectations(&path, "livery", &expected).expect("same counts match");

    // A subtest regression inside a still-failing file is unexpected --
    // this is the case a status-only pin cannot see.
    let regressed = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((38, 47)),
        subtest_results: None,
    }];
    let err =
        check_expectations(&path, "livery", &regressed).expect_err("a lost subtest is unexpected");
    assert!(err.contains("expected fail 40/47, got fail 38/47"), "{err}");

    // A progression is also unexpected, so the baseline gets re-pinned
    // rather than silently drifting below reality.
    let progressed = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((43, 47)),
        subtest_results: None,
    }];
    let err = check_expectations(&path, "livery", &progressed)
        .expect_err("a gained subtest wants a re-pin");
    assert!(err.contains("expected fail 40/47, got fail 43/47"), "{err}");
    let _ = fs::remove_file(path);
}

#[test]
fn expectations_pin_named_subtests_even_when_aggregate_counts_do_not_move() {
    let path = temp_expectations_path("named-subtests");
    let expected = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((1, 2)),
        subtest_results: Some(vec![
            ActualSubtest {
                name: "alpha".into(),
                status: "pass".into(),
            },
            ActualSubtest {
                name: "beta".into(),
                status: "fail".into(),
            },
        ]),
    }];
    write_expectations(&path, test_result_metadata("livery", "css"), &expected)
        .expect("write named expectations");
    check_expectations(&path, "livery", &expected).expect("named subtests match");

    let changed = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((1, 2)),
        subtest_results: Some(vec![
            ActualSubtest {
                name: "alpha".into(),
                status: "fail".into(),
            },
            ActualSubtest {
                name: "beta".into(),
                status: "pass".into(),
            },
        ]),
    }];
    let error = check_expectations(&path, "livery", &changed)
        .expect_err("a named subtest change is unexpected");
    assert!(
        error.contains("subtest `alpha` expected pass, got fail"),
        "{error}"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn opt_in_expected_nonpasses_require_a_reason() {
    let path = temp_expectations_path("opt-in-reason");
    fs::write(
        &path,
        r#"{
            "version": 1,
            "command": "testharness",
            "engine": "boa",
            "renderer": "stylo",
            "policy": "opt-in",
            "tests": {
                "dom/example.html": {
                    "status": "fail",
                    "subtests_passed": 0,
                    "subtests_total": 1,
                    "subtests": [
                        {"name": "known gap", "status": "fail"}
                    ]
                }
            }
        }"#,
    )
    .expect("write opt-in expectations");
    let error = load_expectations(&path)
        .err()
        .expect("unreasoned opt-in failure must fail");
    assert!(
        error.contains("must explain the expected `fail` result with `reason`"),
        "{error}"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn generated_opt_in_todo_is_not_an_accepted_reason() {
    let path = temp_expectations_path("opt-in-todo");
    let actuals = vec![ActualRecord {
        test: "dom/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((0, 1)),
        subtest_results: Some(vec![ActualSubtest {
            name: "known gap".into(),
            status: "fail".into(),
        }]),
    }];
    let mut metadata = test_result_metadata("stylo", "dom/example.html");
    metadata.policy = ExpectationPolicy::OptIn;
    write_expectations(&path, metadata, &actuals).expect("write opt-in skeleton");

    let error = load_expectations(&path)
        .err()
        .expect("generated TODO must be replaced before checking");
    assert!(
        error.contains("meaningful `reason`, not an empty or TODO placeholder"),
        "{error}"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn opt_in_policy_rejects_file_level_nonexecution_statuses() {
    let path = temp_expectations_path("opt-in-skip");
    fs::write(
        &path,
        r#"{
            "version": 1,
            "command": "testharness",
            "engine": "boa",
            "renderer": "stylo",
            "policy": "opt-in",
            "tests": {
                "dom/example.html": {
                    "status": "skip",
                    "reason": "xhtml"
                }
            }
        }"#,
    )
    .expect("write opt-in skip");
    let error = load_expectations(&path)
        .err()
        .expect("a runtime skip reason is not policy justification");
    assert!(
        error.contains("runtime `reason` is not a human policy reason"),
        "{error}"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn opt_in_policy_uses_the_expectation_map_as_its_include_list() {
    let mut tests = vec![
        TestCase {
            path: PathBuf::from("dom/selected.html"),
            url: "dom/selected.html".into(),
            kind: Kind::Testharness,
            refs: Vec::new(),
            fuzzy: None,
            long_timeout: false,
            from_manifest: true,
        },
        TestCase {
            path: PathBuf::from("dom/new.html"),
            url: "dom/new.html".into(),
            kind: Kind::Testharness,
            refs: Vec::new(),
            fuzzy: None,
            long_timeout: false,
            from_manifest: true,
        },
    ];
    let expectations = Expectations {
        renderer: Some("stylo".into()),
        policy: ExpectationPolicy::OptIn,
        tests: BTreeMap::from([(
            "dom/selected.html".into(),
            ExpectedRecord {
                status: "pass".into(),
                reason: None,
                subtests: None,
                subtest_results: None,
            },
        )]),
    };
    retain_opted_in_tests(&mut tests, &expectations);
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name(), "dom/selected.html");
}

#[test]
fn expectations_without_counts_pin_status_only() {
    let path = temp_expectations_path("legacy-counts");
    // A hand-shaped legacy file: bare status, no counts, no renderer.
    fs::write(
        &path,
        r#"{"version":1,"command":"testharness","engine":"boa",
           "tests":{"css/example.html":"fail"}}"#,
    )
    .expect("write legacy file");
    let actual = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "fail",
        reason: None,
        subtests: Some((38, 47)),
        subtest_results: None,
    }];
    check_expectations(&path, "livery", &actual)
        .expect("a legacy count-free file pins status only");
    let _ = fs::remove_file(path);
}

#[test]
fn expectations_reject_a_renderer_mismatch() {
    let path = temp_expectations_path("renderer");
    let expected = vec![ActualRecord {
        test: "css/example.html".into(),
        status: "pass",
        reason: None,
        subtests: Some((5, 5)),
        subtest_results: None,
    }];
    write_expectations(&path, test_result_metadata("livery", "css"), &expected)
        .expect("write expectations");
    let err = check_expectations(&path, "stylo", &expected)
        .expect_err("a Livery baseline must not vouch for a Stylo run");
    assert!(err.contains("written under renderer `livery`"), "{err}");
    let _ = fs::remove_file(path);
}

#[test]
fn reftest_renderer_accepts_only_the_owned_lane() {
    assert_eq!(ReftestRenderer::parse("stylo"), None);
    assert_eq!(
        ReftestRenderer::parse("LIVERY"),
        Some(ReftestRenderer::Livery)
    );
    assert_eq!(ReftestRenderer::parse("boa"), None);
}

#[test]
fn a_document_without_script_still_runs() {
    assert!(!needs_script("<p>hello</p>"));
    assert!(!needs_script("<div onload-ish>no attribute here</div>"));
}

#[test]
fn a_read_only_script_does_not_need_script() {
    // The WPT flush idiom, in both the shapes the corpus uses.
    assert!(!needs_script(
        "<div/><script type=\"text/javascript\">document.body.offsetWidth</script>"
    ));
    assert!(!needs_script(
        "<script>\n  document.body.offsetTop;\n</script>"
    ));
    assert!(!needs_script(
        "<script><![CDATA[ document.body.offsetWidth; ]]></script>"
    ));
    assert!(!needs_script("<script></script>"));
}

#[test]
fn anything_a_script_could_change_still_skips() {
    assert!(needs_script("<script>document.body.remove();</script>"));
    assert!(needs_script("<script>t.style.color = 'red'</script>"));
    assert!(needs_script(
        "<script>document.body.offsetWidth; x.remove()</script>"
    ));
    assert!(needs_script("<script>// just a comment</script>"));
    assert!(needs_script(
        "<script src=\"/common/rendering-utils.js\"></script>"
    ));
    assert!(needs_script("<script SRC='x.js'></script>"));
}

#[test]
fn an_inert_script_does_not_excuse_the_rest_of_the_document() {
    // A body that reads nothing still leaves these two ways in.
    assert!(needs_script(
        "<html class=\"reftest-wait\"><script>document.body.offsetTop;</script>"
    ));
    assert!(needs_script(
        "<body onload=\"doTest()\"><script>document.body.offsetTop;</script>"
    ));
    assert!(needs_script(
        "<body ONLOAD = 'doTest()'><script>document.body.offsetTop;</script>"
    ));
}

#[test]
fn unreadable_script_markup_skips() {
    assert!(needs_script("<script>document.body.offsetTop;"));
    assert!(needs_script("<script"));
}
