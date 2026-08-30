//! The test262 command lane: script, module and async harness modes
//! over the runner core in [`crate::test262`], plus the command that
//! walks the corpus.

use super::*;

/// One test262 outcome.
pub(crate) enum T262 {
    Pass,
    Fail,
    Skip,
}

/// Run one test262 test on engine `E` and classify it.
///
/// **module** (`flags: [module]`) installs the harness preamble as a script, then
/// evaluates the test as a module (imports resolved against the test's directory).
/// Otherwise it assembles (harness + includes + test) for each strict variant and
/// evals. A positive test passes iff it does not throw; a negative test passes iff it
/// throws an error of the expected type (matched against the thrown value's toString).
/// `async` tests report completion through `$DONE`; `module` tests run as ES modules.
pub(crate) fn run_262<E: ScriptEngine>(
    hns: &test262::Harness,
    test_src: &str,
    meta: &test262::Test262Meta,
    path: &Path,
) -> T262 {
    if meta.flags.r#async {
        return run_262_async::<E>(hns, test_src, meta);
    }
    if meta.flags.module {
        return run_262_module::<E>(hns, test_src, meta, path);
    }
    let negative = meta.negative.as_ref();
    for &strict in &test262::strict_variants(&meta.flags) {
        let Ok(script) = hns.assemble(test_src, meta, strict) else {
            return T262::Skip; // a missing include file
        };
        // Ok(()) = ran without throwing; Err(desc) = threw, with the error's toString.
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            let mut rt = script_runtime_api::Runtime::<E>::new().map_err(|_| String::new())?;
            match rt.eval(&script) {
                Ok(_) => Ok(()),
                Err(e) => Err(rt.describe_error(&e)),
            }
        }));
        let ran = match outcome {
            Ok(r) => r,
            Err(_) => return T262::Fail, // the engine panicked on this source
        };
        let ok = match (negative, ran) {
            (None, Ok(())) => true,     // positive: must not throw
            (None, Err(_)) => false,    // positive: threw
            (Some(_), Ok(())) => false, // negative: must throw
            (Some(neg), Err(desc)) => negative_matches(&desc, neg), // negative: right type
        };
        if !ok {
            return T262::Fail;
        }
    }
    T262::Pass
}

/// Whether a thrown error's description satisfies a `negative:` expectation. Both
/// engines name the JS constructor (e.g. "TypeError") in the thrown value's `toString`;
/// Nova additionally reports a parse failure as the literal "parse error", so a
/// parse-phase negative also matches that.
pub(crate) fn negative_matches(desc: &str, neg: &test262::Negative) -> bool {
    desc.contains(&neg.error_type)
        || (matches!(neg.phase.as_str(), "parse" | "early") && desc.contains("parse error"))
}

/// Module test: evaluate the harness preamble as a sloppy script (so its globals
/// land on `globalThis`), then run the test as a module. Imports resolve against the
/// importing file's directory (the entry module's referrer is its own path).
pub(crate) fn run_262_module<E: ScriptEngine>(
    hns: &test262::Harness,
    test_src: &str,
    meta: &test262::Test262Meta,
    path: &Path,
) -> T262 {
    let Ok(preamble) = hns.preamble(meta) else {
        return T262::Skip; // a missing include file
    };
    let negative = meta.negative.is_some();
    let base = path.to_string_lossy().into_owned();
    let test_src = test_src.to_string();
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(move || {
        let Ok(mut rt) = script_runtime_api::Runtime::<E>::new() else {
            return true;
        };
        if rt.eval(&preamble).is_err() {
            return true; // the harness itself failed to load
        }
        let mut resolve = |specifier: &str, referrer: &str| -> Option<(String, String)> {
            let target = Path::new(referrer).parent()?.join(specifier);
            let src = std::fs::read_to_string(&target).ok()?;
            Some((target.to_string_lossy().into_owned(), src))
        };
        rt.eval_module(&test_src, &base, &mut resolve).is_err()
    }));
    let threw = match outcome {
        Ok(t) => t,
        Err(_) => return T262::Fail,
    };
    if threw != negative {
        T262::Fail
    } else {
        T262::Pass
    }
}

/// Async test: the test signals completion through `$DONE`, which the harness's
/// `doneprintHandle.js` reports via `print`. We shim `print` into a JS buffer, run the
/// test, drive the event loop to settle promise/timer jobs, then read the buffer back
/// and scan for the `Test262:AsyncTestComplete` sentinel (absent or `…Failure` = fail).
///
/// Re-enabled once per-test worker-subprocess isolation existed: each async test runs
/// in its own reaped process bounded by `--timeout`, so a non-settling test is a clean
/// timeout, not the cross-test memory blow-up that forced the earlier in-process revert.
pub(crate) fn run_262_async<E: ScriptEngine>(
    hns: &test262::Harness,
    test_src: &str,
    meta: &test262::Test262Meta,
) -> T262 {
    let Ok(preamble) = hns.preamble(meta) else {
        return T262::Skip; // a missing include file
    };
    let negative = meta.negative.is_some();
    // `print` is defined before `$DONE` is invoked; `doneprintHandle.js` (in the
    // preamble) calls it on completion. The host captures `console`, but the test262
    // async harness uses `print`, so route it into a buffer we can read back.
    let script = format!(
        "globalThis.__262log='';globalThis.print=function(s){{__262log+=String(s)+'\\n';}};\n{preamble}{test_src}"
    );
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(move || -> bool {
        let Ok(mut rt) = script_runtime_api::Runtime::<E>::new() else {
            return true;
        };
        if rt.eval(&script).is_err() {
            return true; // threw synchronously before completing
        }
        let _ = rt.run_event_loop(1024); // settle promise/timer jobs (breaks when idle)
        let log = rt
            .eval("__262log")
            .ok()
            .and_then(|v| rt.value_to_string(&v).ok())
            .unwrap_or_default();
        let passed =
            log.contains("Test262:AsyncTestComplete") && !log.contains("Test262:AsyncTestFailure");
        !passed // threw-style: true = did not pass
    }));
    let threw = match outcome {
        Ok(t) => t,
        Err(_) => return T262::Fail,
    };
    if threw != negative {
        T262::Fail
    } else {
        T262::Pass
    }
}

/// Dispatch [`run_262`] to the concrete engine, mirroring `harness::run_test`.
pub(crate) fn run_262_on(
    engine: harness::Engine,
    hns: &test262::Harness,
    test_src: &str,
    meta: &test262::Test262Meta,
    path: &Path,
) -> T262 {
    match engine {
        harness::Engine::Boa => run_262::<script_engine_boa::BoaEngine>(hns, test_src, meta, path),
        harness::Engine::Nova => {
            run_262::<script_engine_nova::NovaEngine>(hns, test_src, meta, path)
        },
    }
}

/// Worker mode: run ONE test262 test (both engines) and print per-engine results,
/// each line flushed, so the parent ([`test262_cmd`]) can attribute a hang to the
/// engine that never reported. The parent spawns this as a subprocess per test, so a
/// hanging test (the engines cannot be step-metered) kills only this process.
pub(crate) fn test262_one(args: &Args) {
    use std::io::Write;
    // A panicking test is caught by run_262's catch_unwind (→ Fail); silence the hook.
    panic::set_hook(Box::new(|_| {}));

    let t262_root = Path::new(&args.tests_root).join("third_party/test262");
    let hns = match test262::Harness::load(&t262_root.join("harness")) {
        Ok(h) => h,
        Err(_) => std::process::exit(2), // parent sees no output → counts as skip
    };
    let path = t262_root.join("test").join(&args.subset);
    let Ok(src) = fs::read_to_string(&path) else {
        std::process::exit(2);
    };
    let meta = test262::parse_meta(&src);

    let mut so = std::io::stdout();
    let boa = run_262_on(harness::Engine::Boa, &hns, &src, &meta, &path);
    let _ = writeln!(so, "boa {}", t262_word(&boa));
    let _ = so.flush();
    let nova = run_262_on(harness::Engine::Nova, &hns, &src, &meta, &path);
    let _ = writeln!(so, "nova {}", t262_word(&nova));
    let _ = so.flush();
}

/// The wire word for one engine's outcome (the `test262-one` worker protocol).
pub(crate) fn t262_word(t: &T262) -> &'static str {
    match t {
        T262::Pass => "pass",
        T262::Fail => "fail",
        T262::Skip => "skip",
    }
}

/// Parse a `<engine> <pass|fail|skip>` line from a worker's output. `None` if the
/// engine never reported (it hung, or the worker died before reaching it).
pub(crate) fn parse_engine_result(out: &str, engine: &str) -> Option<T262> {
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix(engine) {
            return Some(match rest.trim() {
                "pass" => T262::Pass,
                "fail" => T262::Fail,
                _ => T262::Skip,
            });
        }
    }
    None
}

pub(crate) fn is_262_test(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "js")
        && !p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_FIXTURE.js"))
}

pub(crate) fn collect_262(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.is_file() {
        if is_262_test(dir) {
            out.push(dir.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_262(&p, out);
        } else if is_262_test(&p) {
            out.push(p);
        }
    }
}

/// `test262 <subset>`: run each test262 test (under `third_party/test262/test/<subset>`)
/// on **both** engines and diff. Boa-pass / Nova-fail is a **Nova JS-engine gap** —
/// the actual Nova worklist, since WPT showed Boa/Nova at parity. Disk only; run in
/// **release** (debug frames overflow on bounded-deep recursion).
pub(crate) fn test262_cmd(args: &Args) {
    let t262_root = Path::new(&args.tests_root).join("third_party/test262");
    // Preflight: fail fast with a clear message if the harness is missing. The actual
    // runs happen in `test262-one` worker subprocesses, which load it themselves.
    if let Err(e) = test262::Harness::load(&t262_root.join("harness")) {
        eprintln!("test262 harness load failed ({}): {e}", t262_root.display());
        std::process::exit(2);
    }
    let subset_dir = t262_root.join("test").join(&args.subset);
    if !subset_dir.exists() {
        eprintln!(
            "test262 subset path does not exist: {}",
            subset_dir.display()
        );
        std::process::exit(2);
    }
    let mut files = Vec::new();
    collect_262(&subset_dir, &mut files);
    let test_root = t262_root.join("test");

    // Boa and Nova cannot be step-metered (eval_bounded is unbounded for both), so a
    // pathological test (e.g. a Promise.race iterator-close infinite loop) would hang
    // the whole run. We isolate each test in a worker subprocess (`test262-one`) with a
    // wall-clock timeout: a hang kills only that process, is recorded as a timeout
    // (attributed to whichever engine never reported), and the run continues. A shared
    // work index keeps the worker pool balanced across the sorted corpus; jemalloc is
    // already linked, so per-test cost is engine-bound, not allocator-bound. Process
    // startup (~0.1s) is modest against per-test engine work, the price of hang-safety.
    let test_timeout = std::time::Duration::from_secs(args.timeout_secs);

    #[derive(Default)]
    struct Tally {
        both_pass: u64,
        both_fail: u64,
        boa_only: u64,
        nova_only: u64,
        skipped: u64,
        timeout: u64,
        worklist: Vec<String>,
        timeouts: Vec<String>,
    }

    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let verbose = args.verbose;
    let test_root = test_root.as_path();
    let files = &files;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let next = &next;
    let tests_root = args.tests_root.as_str();
    let exe = std::env::current_exe().ok();
    let exe = exe.as_deref();
    let subset_label = if args.subset.is_empty() {
        "<all>"
    } else {
        &args.subset
    };
    println!(
        "test262 [{subset_label}]: {} tests x 2 engines on {jobs} worker procs (timeout {}s)…",
        files.len(),
        test_timeout.as_secs(),
    );

    let tally = std::thread::scope(|scope| {
        // A shared work index: workers pull the next test as they finish, so the
        // heterogeneous corpus stays balanced (contiguous chunks imbalance when the
        // slow both-pass tests cluster, as they do in the sorted corpus).
        let handles: Vec<_> = (0..jobs)
            .map(|_| {
                scope.spawn(move || {
                    let mut t = Tally::default();
                    let Some(exe) = exe else {
                        return t; // cannot locate our own binary to spawn workers
                    };
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= files.len() {
                            break;
                        }
                        let path = &files[i];
                        let rel = path.strip_prefix(test_root).unwrap_or(path);
                        let name = rel.to_string_lossy().replace('\\', "/");

                        let spawned = std::process::Command::new(exe)
                            .arg("test262-one")
                            .arg(rel.as_os_str())
                            .arg("--tests-root")
                            .arg(tests_root)
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        let Ok(mut child) = spawned else {
                            t.skipped += 1;
                            continue;
                        };

                        let start = std::time::Instant::now();
                        let timed_out = loop {
                            match child.try_wait() {
                                Ok(Some(_)) => break false,
                                Ok(None) => {},
                                Err(_) => break false,
                            }
                            if start.elapsed() >= test_timeout {
                                let _ = child.kill();
                                let _ = child.wait();
                                break true;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        };
                        let mut out = String::new();
                        if let Some(mut so) = child.stdout.take() {
                            use std::io::Read;
                            let _ = so.read_to_string(&mut out);
                        }
                        let boa = parse_engine_result(&out, "boa");
                        let nova = parse_engine_result(&out, "nova");

                        if timed_out {
                            // Whichever engine never reported is the one still spinning.
                            let eng = if boa.is_none() { "boa" } else { "nova" };
                            if verbose {
                                println!("TIMEOUT[{eng}]  {name}");
                            }
                            t.timeout += 1;
                            t.timeouts.push(format!("{name} ({eng})"));
                            continue;
                        }
                        let (b, n) = match (boa, nova) {
                            (Some(b), Some(n)) => (b, n),
                            (Some(b), None) => (b, T262::Fail), // nova crashed mid-test
                            (None, Some(n)) => (T262::Fail, n), // boa crashed mid-test
                            (None, None) => {
                                t.skipped += 1; // worker produced nothing (load/early crash)
                                continue;
                            },
                        };
                        match (b, n) {
                            (T262::Skip, _) | (_, T262::Skip) => t.skipped += 1,
                            (T262::Pass, T262::Pass) => t.both_pass += 1,
                            (T262::Fail, T262::Fail) => t.both_fail += 1,
                            (T262::Pass, T262::Fail) => {
                                if verbose {
                                    println!("NOVA-GAP  {name}");
                                }
                                t.boa_only += 1;
                                t.worklist.push(name);
                            },
                            (T262::Fail, T262::Pass) => t.nova_only += 1,
                        }
                    }
                    t
                })
            })
            .collect();
        let mut total = Tally::default();
        for h in handles {
            let t = h.join().unwrap_or_default();
            total.both_pass += t.both_pass;
            total.both_fail += t.both_fail;
            total.boa_only += t.boa_only;
            total.nova_only += t.nova_only;
            total.skipped += t.skipped;
            total.timeout += t.timeout;
            total.worklist.extend(t.worklist);
            total.timeouts.extend(t.timeouts);
        }
        total
    });

    let mut nova_worklist = tally.worklist;
    nova_worklist.sort();
    let mut timeouts = tally.timeouts;
    timeouts.sort();
    println!(
        "\ntest262 compare [{subset_label}]: both-pass={} both-fail={} boa-only={} (Nova gap) \
         nova-only={} timeout={} skipped={} (module/async/missing)",
        tally.both_pass,
        tally.both_fail,
        tally.boa_only,
        tally.nova_only,
        tally.timeout,
        tally.skipped,
    );
    if !timeouts.is_empty() {
        println!(
            "\nExceeded {}s — infinite hang or pathological slowness (the engine that \
             never reported) — {} test(s):",
            test_timeout.as_secs(),
            timeouts.len()
        );
        for name in timeouts.iter().take(40) {
            println!("  {name}");
        }
        if timeouts.len() > 40 {
            println!("  … and {} more", timeouts.len() - 40);
        }
    }
    if !nova_worklist.is_empty() {
        println!(
            "\nNova worklist (pass on Boa, fail on Nova) — {} test(s):",
            nova_worklist.len()
        );
        for name in nova_worklist.iter().take(40) {
            println!("  {name}");
        }
        if nova_worklist.len() > 40 {
            println!("  … and {} more", nova_worklist.len() - 40);
        }
    }

    if let Some(out_path) = &args.worklist_out {
        use std::io::Write;
        let mut buf = format!(
            "# test262 worklist [{subset_label}]\n\
             # both-pass={} both-fail={} boa-only={} nova-only={} timeout={} skipped={}\n",
            tally.both_pass,
            tally.both_fail,
            tally.boa_only,
            tally.nova_only,
            tally.timeout,
            tally.skipped,
        );
        buf.push_str(&format!(
            "\n## Timeouts (hang or pathological slowness; engine) — {}\n",
            timeouts.len()
        ));
        for t in &timeouts {
            buf.push_str(t);
            buf.push('\n');
        }
        buf.push_str(&format!(
            "\n## Nova gaps (pass on Boa, fail on Nova) — {}\n",
            nova_worklist.len()
        ));
        for n in &nova_worklist {
            buf.push_str(n);
            buf.push('\n');
        }
        match std::fs::File::create(out_path).and_then(|mut f| f.write_all(buf.as_bytes())) {
            Ok(()) => println!(
                "\nworklist written to {out_path} ({} Nova gaps, {} timeouts)",
                nova_worklist.len(),
                timeouts.len()
            ),
            Err(e) => eprintln!("failed to write worklist to {out_path}: {e}"),
        }
    }
}
