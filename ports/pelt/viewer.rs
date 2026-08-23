/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Standalone Pelt reference-host entrypoint.

use std::env;

use genet_host_api::{DeferredShellEngine, EngineProfile, ShellEngine};

use crate::VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedEngine {
    Livery,
    Scripted,
    Reader,
}

impl SelectedEngine {
    fn profile(self) -> EngineProfile {
        match self {
            Self::Livery | Self::Reader => EngineProfile::Livery,
            Self::Scripted => EngineProfile::Scripted,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Livery => "livery",
            Self::Scripted => "scripted",
            Self::Reader => "reader",
        }
    }
}

pub(crate) fn main() {
    let mut selected_engine = SelectedEngine::Livery;
    let mut url = None;
    // JS backend for `--engine scripted` (boa default; nova needs --features
    // scripted-nova). Parsed as a string so the flag exists even in builds without
    // the scripted profile.
    let mut js_engine = String::from("boa");
    // Physical client size for headed viewers.
    let mut size: Option<(u32, u32)> = None;
    // Bounded headed capture/smoke run. Interactive profiles leave this unset.
    let mut frames: Option<u32> = None;
    let mut netrender_smoke = false;
    let mut webgl_wgpu_smoke = false;
    #[cfg(feature = "windows-present")]
    let mut windows_present_smoke = false;
    #[cfg(feature = "windows-present")]
    let mut windows_present_surfaces_smoke = false;
    #[cfg(feature = "macos-present")]
    let mut macos_present_smoke = false;
    #[cfg(feature = "macos-present")]
    let mut macos_present_surfaces_smoke = false;
    #[cfg(feature = "linux-present")]
    let mut wayland_present_smoke = false;
    #[cfg(feature = "linux-present")]
    let mut wayland_present_surfaces_smoke = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            },
            "--version" => {
                println!("{VERSION}");
                return;
            },
            "--engine" => {
                let Some(value) = args.next() else {
                    eprintln!("--engine requires livery, reader, or scripted");
                    std::process::exit(2);
                };
                selected_engine = parse_engine(&value);
            },
            value if value.starts_with("--engine=") => {
                selected_engine = parse_engine(&value["--engine=".len()..]);
            },
            "--js" => {
                let Some(value) = args.next() else {
                    eprintln!("--js requires boa or nova");
                    std::process::exit(2);
                };
                js_engine = value;
            },
            value if value.starts_with("--js=") => {
                js_engine = value["--js=".len()..].to_owned();
            },
            "--size" => {
                let Some(value) = args.next() else {
                    eprintln!("--size requires WxH in physical pixels");
                    std::process::exit(2);
                };
                size = Some(parse_size(&value));
            },
            value if value.starts_with("--size=") => {
                size = Some(parse_size(&value["--size=".len()..]));
            },
            "--frames" => {
                let Some(value) = args.next() else {
                    eprintln!("--frames requires a positive integer");
                    std::process::exit(2);
                };
                frames = Some(parse_frames(&value));
            },
            value if value.starts_with("--frames=") => {
                frames = Some(parse_frames(&value["--frames=".len()..]));
            },
            "--netrender-smoke" => {
                netrender_smoke = true;
            },
            "--webgl-wgpu-smoke" => {
                webgl_wgpu_smoke = true;
            },
            #[cfg(feature = "windows-present")]
            "--windows-present-smoke" => {
                windows_present_smoke = true;
            },
            #[cfg(feature = "windows-present")]
            "--windows-present-surfaces-smoke" => {
                windows_present_surfaces_smoke = true;
            },
            #[cfg(feature = "macos-present")]
            "--macos-present-smoke" => {
                macos_present_smoke = true;
            },
            #[cfg(feature = "macos-present")]
            "--macos-present-surfaces-smoke" => {
                macos_present_surfaces_smoke = true;
            },
            #[cfg(feature = "linux-present")]
            "--wayland-present-smoke" => {
                wayland_present_smoke = true;
            },
            #[cfg(feature = "linux-present")]
            "--wayland-present-surfaces-smoke" => {
                wayland_present_surfaces_smoke = true;
            },
            value if value.starts_with('-') => {
                eprintln!("unsupported pelt option: {value}");
                std::process::exit(2);
            },
            value => {
                url = Some(value.to_owned());
            },
        }
    }

    let engine_profile = selected_engine.profile();
    let engine = DeferredShellEngine::new(engine_profile);
    let capabilities = engine.capabilities();
    let url = url.unwrap_or_else(|| "about:blank".to_owned());
    println!(
        "pelt host profile={} url={} javascript={} webdriver={} devtools={} webgpu={} webxr={}",
        selected_engine.label(),
        url,
        capabilities.javascript,
        capabilities.webdriver,
        capabilities.devtools,
        capabilities.webgpu,
        capabilities.webxr
    );

    if netrender_smoke {
        // Presentation-backend smoke: it is independent of the selected
        // document engine and exits after its bounded receipt.
        run_optional_netrender_smoke();
        return;
    }

    if webgl_wgpu_smoke {
        run_optional_webgl_wgpu_smoke();
        return;
    }

    #[cfg(feature = "windows-present")]
    if windows_present_smoke {
        run_optional_windows_present_smoke();
        return;
    }

    #[cfg(feature = "windows-present")]
    if windows_present_surfaces_smoke {
        run_optional_windows_present_surfaces_smoke();
        return;
    }

    #[cfg(feature = "macos-present")]
    if macos_present_smoke {
        run_optional_macos_present_smoke();
        return;
    }

    #[cfg(feature = "macos-present")]
    if macos_present_surfaces_smoke {
        run_optional_macos_present_surfaces_smoke();
        return;
    }

    #[cfg(feature = "linux-present")]
    if wayland_present_smoke {
        run_optional_wayland_present_smoke();
        return;
    }

    #[cfg(feature = "linux-present")]
    if wayland_present_surfaces_smoke {
        run_optional_wayland_present_surfaces_smoke();
        return;
    }

    match selected_engine {
        SelectedEngine::Reader => run_reader_profile(url, size, frames),
        SelectedEngine::Livery => {
            // Protocol-native content bypasses the HTML engine while retaining
            // the same headed host. P4 will move this choice into Inker routing.
            #[cfg(feature = "smolweb")]
            if is_smolweb_url(&url) {
                run_smolweb_profile(url, size, frames);
                return;
            }

            #[cfg(feature = "livery")]
            run_livery_profile(url, size, frames);
            #[cfg(not(feature = "livery"))]
            {
                eprintln!(
                    "pelt has no registered engine 'genet.livery'; rebuild with `--features livery`"
                );
                std::process::exit(2);
            }
        },
        SelectedEngine::Scripted => {
            // Runs inline scripts on the chosen backend and retains the mutated
            // document through the same owned Livery/Buckram route.
            run_scripted_profile(url, js_engine, size, frames);
        },
    }
}

/// Dispatch held HTML to the shared fleece reader lane.
#[cfg(feature = "reader")]
fn run_reader_profile(url: String, size: Option<(u32, u32)>, frames: Option<u32>) {
    let mut config = pelt_desktop::StaticViewerConfig::new(
        EngineProfile::Livery,
        pelt_desktop::WindowingMode::Headed,
        url,
    );
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    match pelt_desktop::run_reader_viewer(config) {
        Ok(outcome) => println!(
            "pelt reader viewer engine=genet.reader url={} window={} redraws={} size={}x{}",
            outcome.url, outcome.created_window, outcome.redraws, outcome.size.0, outcome.size.1
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(not(feature = "reader"))]
fn run_reader_profile(_url: String, _size: Option<(u32, u32)>, _frames: Option<u32>) {
    eprintln!("pelt has no registered engine 'genet.reader'; rebuild with `--features reader`");
    std::process::exit(2);
}

/// Whether `url` is a smolweb scheme the native smolweb viewer handles.
#[cfg(feature = "smolweb")]
fn is_smolweb_url(url: &str) -> bool {
    [
        "gemini://",
        "gopher://",
        "nex://",
        "finger://",
        "spartan://",
        "guppy://",
    ]
    .iter()
    .any(|scheme| url.starts_with(scheme))
}

/// Parse a physical client size accepted by headed profiles.
fn parse_size(value: &str) -> (u32, u32) {
    let Some((width, height)) = value.split_once(['x', 'X']) else {
        eprintln!("--size expects WxH in physical pixels (got '{value}')");
        std::process::exit(2);
    };
    let width = width.parse::<u32>().ok().filter(|value| *value > 0);
    let height = height.parse::<u32>().ok().filter(|value| *value > 0);
    match (width, height) {
        (Some(width), Some(height)) => (width, height),
        _ => {
            eprintln!("--size expects positive WxH dimensions (got '{value}')");
            std::process::exit(2);
        },
    }
}

/// Parse a deterministic headed-frame limit. Zero would open and immediately close a
/// window without proving it presented, so it is deliberately rejected.
fn parse_frames(value: &str) -> u32 {
    match value.parse::<u32>().ok().filter(|value| *value > 0) {
        Some(value) => value,
        None => {
            eprintln!("--frames expects a positive integer (got '{value}')");
            std::process::exit(2);
        },
    }
}

/// Dispatch a smolweb URL to the owned headed document viewer.
#[cfg(feature = "smolweb")]
fn run_smolweb_profile(url: String, size: Option<(u32, u32)>, frames: Option<u32>) {
    let mut config = pelt_desktop::StaticViewerConfig::new(
        EngineProfile::Livery,
        pelt_desktop::WindowingMode::Headed,
        url,
    );
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    match pelt_desktop::run_smolweb_viewer(config) {
        Ok(outcome) => println!(
            "pelt smolweb viewer url={} window={} redraws={} size={}x{}",
            outcome.url, outcome.created_window, outcome.redraws, outcome.size.0, outcome.size.1
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

/// Dispatch the scripted profile to the on-screen scripted viewer on the chosen JS
/// backend. Present only when built with `--features scripted`.
#[cfg(feature = "scripted")]
fn run_scripted_profile(url: String, js: String, size: Option<(u32, u32)>, frames: Option<u32>) {
    let Some(engine) = pelt_desktop::ScriptedEngine::parse(&js) else {
        eprintln!("--js expects boa or nova (got '{js}')");
        std::process::exit(2);
    };
    let mut config = pelt_desktop::StaticViewerConfig::new(
        EngineProfile::Scripted,
        pelt_desktop::WindowingMode::Headed,
        url,
    );
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    match pelt_desktop::run_scripted_viewer(config, engine) {
        Ok(outcome) => println!(
            "pelt scripted viewer engine={} url={} window={} redraws={} size={}x{}",
            engine.label(),
            outcome.url,
            outcome.created_window,
            outcome.redraws,
            outcome.size.0,
            outcome.size.1,
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

/// Dispatch script-free HTML to the owned Livery/Buckram document engine.
#[cfg(feature = "livery")]
fn run_livery_profile(url: String, size: Option<(u32, u32)>, frames: Option<u32>) {
    let mut config = pelt_desktop::StaticViewerConfig::new(
        EngineProfile::Livery,
        pelt_desktop::WindowingMode::Headed,
        url,
    );
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    match pelt_desktop::run_livery_viewer(config) {
        Ok(outcome) => println!(
            "pelt livery viewer engine=genet.livery url={} window={} redraws={} size={}x{}",
            outcome.url, outcome.created_window, outcome.redraws, outcome.size.0, outcome.size.1
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

/// Without the scripted profile compiled in, `--engine scripted` is a clean error
/// pointing at the feature to enable.
#[cfg(not(feature = "scripted"))]
fn run_scripted_profile(
    _url: String,
    _js: String,
    _size: Option<(u32, u32)>,
    _frames: Option<u32>,
) {
    eprintln!(
        "pelt was built without the scripted profile; rebuild with `--features scripted` \
         (or `--features scripted-nova` for the Nova backend)"
    );
    std::process::exit(2);
}

#[cfg(feature = "viewer-netrender")]
fn run_optional_netrender_smoke() {
    match pelt_desktop::run_netrender_smoke() {
        Ok(outcome) => {
            println!(
                "pelt netrender smoke rendered {}x{} painted_pixels={}",
                outcome.width, outcome.height, outcome.painted_pixels
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(not(feature = "viewer-netrender"))]
fn run_optional_netrender_smoke() {
    eprintln!("the netrender smoke needs `--features viewer-netrender`");
    std::process::exit(2);
}

#[cfg(feature = "viewer-netrender")]
fn run_optional_webgl_wgpu_smoke() {
    match pelt_desktop::run_webgl_wgpu_smoke() {
        Ok(outcome) => {
            println!(
                "pelt webgl-wgpu smoke rendered {}x{} painted_pixels={} canvas_center={:?} overlay_center={:?}",
                outcome.width,
                outcome.height,
                outcome.painted_pixels,
                outcome.canvas_center,
                outcome.overlay_center
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(not(feature = "viewer-netrender"))]
fn run_optional_webgl_wgpu_smoke() {
    eprintln!("the WebGL smoke needs `--features viewer-netrender`");
    std::process::exit(2);
}

#[cfg(feature = "windows-present")]
fn run_optional_windows_present_smoke() {
    let config = pelt_desktop::WindowsDxgiPresentSmokeConfig::default();
    match pelt_desktop::run_windows_dxgi_present_smoke(config) {
        Ok(outcome) => {
            println!(
                "pelt windows-present smoke {}x{} frames={} created_window={} declared_subsurface={}",
                outcome.width,
                outcome.height,
                outcome.frames_presented,
                outcome.created_window,
                outcome.declared_subsurface
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(feature = "windows-present")]
fn run_optional_windows_present_surfaces_smoke() {
    let config = pelt_desktop::WindowsDxgiPresentSmokeConfig {
        title: "pelt — windows-dxgi present smoke (with declared surface)".into(),
        declare_subsurface: true,
        frames: 0,
        ..pelt_desktop::WindowsDxgiPresentSmokeConfig::default()
    };
    match pelt_desktop::run_windows_dxgi_present_smoke(config) {
        Ok(outcome) => {
            println!(
                "pelt windows-present surfaces smoke {}x{} frames={} created_window={} declared_subsurface={}",
                outcome.width,
                outcome.height,
                outcome.frames_presented,
                outcome.created_window,
                outcome.declared_subsurface
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(feature = "macos-present")]
fn run_optional_macos_present_smoke() {
    let config = pelt_desktop::MacosCALayerPresentSmokeConfig::default();
    match pelt_desktop::run_macos_calayer_present_smoke(config) {
        Ok(outcome) => {
            println!(
                "pelt macos-present smoke {}x{} frames={} created_window={}",
                outcome.width, outcome.height, outcome.frames_presented, outcome.created_window
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(feature = "macos-present")]
fn run_optional_macos_present_surfaces_smoke() {
    // Same shape as the basic smoke but flips `declare_subsurface`
    // on so the smoke also exercises the per-`SurfaceKey`
    // declare/destroy/present path through `MacosCALayerBackend`.
    // Visual: red full-viewport master with a green top-left
    // quarter; the per-surface CALayer overlays the green region
    // at 50% opacity, producing a yellow-ish blend if the
    // per-surface path is correctly composited above the master
    // CALayer.
    let config = pelt_desktop::MacosCALayerPresentSmokeConfig {
        title: "pelt — macos-calayer present smoke (with declared surface)".into(),
        declare_subsurface: true,
        // `frames: 0` keeps the window open until the user closes
        // it, so they can take a screenshot at their leisure
        // (instead of the basic smoke's auto-exit after ~1s).
        frames: 0,
        ..pelt_desktop::MacosCALayerPresentSmokeConfig::default()
    };
    match pelt_desktop::run_macos_calayer_present_smoke(config) {
        Ok(outcome) => {
            println!(
                "pelt macos-present surfaces smoke {}x{} frames={} created_window={}",
                outcome.width, outcome.height, outcome.frames_presented, outcome.created_window
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(feature = "linux-present")]
fn run_optional_wayland_present_smoke() {
    let config = pelt_desktop::WaylandPresentSmokeConfig::default();
    match pelt_desktop::run_wayland_subsurface_present_smoke(config) {
        Ok(outcome) => {
            println!(
                "pelt wayland-present smoke {}x{} frames={} created_window={} declared_subsurface={}",
                outcome.width,
                outcome.height,
                outcome.frames_presented,
                outcome.created_window,
                outcome.declared_subsurface
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

#[cfg(feature = "linux-present")]
fn run_optional_wayland_present_surfaces_smoke() {
    // Same shape as the basic smoke but flips `declare_subsurface`
    // on and runs frames=0 (held until window close) so the per-
    // surface composition is visible long enough for the visual
    // receipt: red master + green declared-quarter at 50% opacity
    // producing olive blend where they compose.
    let config = pelt_desktop::WaylandPresentSmokeConfig {
        title: "pelt — wayland-subsurface present smoke (with declared surface)".into(),
        declare_subsurface: true,
        frames: 0,
        ..pelt_desktop::WaylandPresentSmokeConfig::default()
    };
    match pelt_desktop::run_wayland_subsurface_present_smoke(config) {
        Ok(outcome) => {
            println!(
                "pelt wayland-present surfaces smoke {}x{} frames={} created_window={} declared_subsurface={}",
                outcome.width,
                outcome.height,
                outcome.frames_presented,
                outcome.created_window,
                outcome.declared_subsurface
            );
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

fn parse_engine(value: &str) -> SelectedEngine {
    if value.eq_ignore_ascii_case("reader") || value.eq_ignore_ascii_case("genet.reader") {
        return SelectedEngine::Reader;
    }
    match value.parse() {
        Ok(EngineProfile::Livery) => SelectedEngine::Livery,
        Ok(EngineProfile::Scripted) => SelectedEngine::Scripted,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        },
    }
}

fn print_help() {
    println!(
        "\
pelt {VERSION}

Usage: pelt [--engine <profile>] [<url-or-file>] [options]

Pelt is Genet's reference host. Livery/Buckram renders script-free HTML by
default (file://, bare paths, data: URLs, and http(s)). `--engine scripted`
runs a page's <script> through the same owned document route (needs
--features scripted). The former viewer, static, and livery-scripted spellings
remain accepted as input aliases. Smoke runners validate the present backends.

Options:
    --engine <livery|reader|scripted>  (diagnostic override; legacy aliases accepted)
    --js <boa|nova>                    (scripted profile; nova needs --features scripted-nova)
    --size <WxH>                       (physical client size)
    --frames <N>                       (headed profiles: exit after N presented frames)
    --netrender-smoke
    --webgl-wgpu-smoke
    --windows-present-smoke            (requires --features windows-present, target_os = \"windows\")
    --windows-present-surfaces-smoke   (same as --windows-present-smoke + a declared compositor surface)
    --macos-present-smoke              (requires --features macos-present, target_vendor = \"apple\")
    --macos-present-surfaces-smoke     (same as --macos-present-smoke + a declared compositor surface)
    --wayland-present-smoke            (requires --features linux-present, target_os = \"linux\")
    --wayland-present-surfaces-smoke   (same as --wayland-present-smoke + a declared compositor surface)
    --version
    -h, --help"
    );
}
