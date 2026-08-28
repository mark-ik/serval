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
    let mut product_receipt: Option<pelt_desktop::ProductReceipt> = None;
    let mut artifact: Option<std::path::PathBuf> = None;
    let mut workspace_size_matrix: Option<Vec<(u32, u32)>> = None;
    let mut with_tiles = false;
    let mut tile_receipt = false;
    let mut capability_receipt = false;
    let mut workspace_receipt: Option<pelt_desktop::WorkspaceReceipt> = None;
    let mut appearance_store: Option<std::path::PathBuf> = None;
    let mut tile_engine_overrides = Vec::new();
    let mut tile_urls = Vec::new();
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
            "--product-receipt" => {
                let Some(value) = args.next() else {
                    eprintln!("--product-receipt requires a named receipt id");
                    std::process::exit(2);
                };
                product_receipt = Some(parse_product_receipt(&value));
            },
            value if value.starts_with("--product-receipt=") => {
                product_receipt = Some(parse_product_receipt(&value["--product-receipt=".len()..]));
            },
            "--artifact" => {
                let Some(value) = args.next() else {
                    eprintln!("--artifact requires a PNG path");
                    std::process::exit(2);
                };
                artifact = Some(value.into());
            },
            value if value.starts_with("--artifact=") => {
                artifact = Some(value["--artifact=".len()..].into());
            },
            "--appearance-store" => {
                let Some(value) = args.next() else {
                    eprintln!("--appearance-store requires a caller-selected file path");
                    std::process::exit(2);
                };
                appearance_store = Some(value.into());
                with_tiles = true;
            },
            value if value.starts_with("--appearance-store=") => {
                appearance_store = Some(value["--appearance-store=".len()..].into());
                with_tiles = true;
            },
            "--tiles" => {
                with_tiles = true;
            },
            "--tile-receipt" => {
                with_tiles = true;
                tile_receipt = true;
            },
            "--capability-receipt" => {
                with_tiles = true;
                capability_receipt = true;
            },
            "--workspace-receipt" => {
                let Some(value) = args.next() else {
                    eprintln!(
                        "--workspace-receipt requires mixed, fallback, chrome, loading-error, appearance, accessibility, accessibility-address, accessibility-children, accessibility-edit, accessibility-scroll, accessibility-click, accessibility-input, narrow-chrome, chrome-dpi, reader, reader-accessibility, tabard-preview, or tabard-reader-preview"
                    );
                    std::process::exit(2);
                };
                workspace_receipt = Some(parse_workspace_receipt(&value));
                with_tiles = true;
            },
            value if value.starts_with("--workspace-receipt=") => {
                workspace_receipt = Some(parse_workspace_receipt(
                    &value["--workspace-receipt=".len()..],
                ));
                with_tiles = true;
            },
            "--workspace-size-matrix" => {
                let Some(value) = args.next() else {
                    eprintln!("--workspace-size-matrix requires comma-separated WxH values");
                    std::process::exit(2);
                };
                workspace_size_matrix = Some(parse_workspace_size_matrix(&value));
            },
            value if value.starts_with("--workspace-size-matrix=") => {
                workspace_size_matrix = Some(parse_workspace_size_matrix(
                    &value["--workspace-size-matrix=".len()..],
                ));
            },
            "--tile-engine" => {
                let Some(value) = args.next() else {
                    eprintln!("--tile-engine requires N=engine-id");
                    std::process::exit(2);
                };
                tile_engine_overrides.push(parse_tile_engine(&value));
                with_tiles = true;
            },
            value if value.starts_with("--tile-engine=") => {
                tile_engine_overrides.push(parse_tile_engine(&value["--tile-engine=".len()..]));
                with_tiles = true;
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
                tile_urls.push(value.to_owned());
            },
        }
    }

    if let Some(receipt) = product_receipt {
        let receipt_engine = if receipt.needs_scripted_profile() {
            SelectedEngine::Scripted
        } else {
            SelectedEngine::Livery
        };
        if selected_engine != receipt_engine {
            eprintln!(
                "product receipt {} is owned by the {} profile",
                receipt.id(),
                receipt_engine.label()
            );
            std::process::exit(2);
        }
        if with_tiles {
            eprintln!(
                "product receipt {} is a single-document receipt",
                receipt.id()
            );
            std::process::exit(2);
        }
        if url.is_some() {
            eprintln!("product receipt {} owns its fixture URL", receipt.id());
            std::process::exit(2);
        }
        if artifact.is_none() {
            eprintln!("--product-receipt {} needs --artifact <png>", receipt.id());
            std::process::exit(2);
        }
        if receipt == pelt_desktop::ProductReceipt::Gemtext && !cfg!(feature = "smolweb") {
            eprintln!("--product-receipt gemtext needs --features smolweb");
            std::process::exit(2);
        }
        url = Some(product_receipt_fixture(receipt));
    } else if artifact.is_some() && workspace_receipt.is_none() {
        eprintln!("--artifact is only accepted with a named receipt");
        std::process::exit(2);
    }

    if let Err(error) = validate_workspace_size_matrix(
        workspace_receipt,
        workspace_size_matrix.as_deref(),
        size.is_some(),
    ) {
        eprintln!("{error}");
        std::process::exit(2);
    }

    let engine_profile = selected_engine.profile();
    let engine = DeferredShellEngine::new(engine_profile);
    let capabilities = engine.capabilities();
    let url = if with_tiles {
        tile_urls
            .first()
            .cloned()
            .unwrap_or_else(|| "about:blank".to_owned())
    } else {
        url.unwrap_or_else(|| "about:blank".to_owned())
    };
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

    if with_tiles {
        if workspace_receipt.is_some() && (tile_receipt || capability_receipt) {
            eprintln!(
                "--workspace-receipt is separate from the P3 tile and P4 native capability receipts"
            );
            std::process::exit(2);
        }
        if let Some(receipt) = workspace_receipt {
            if artifact.is_none() {
                eprintln!(
                    "--workspace-receipt {} needs --artifact <png>",
                    receipt.id()
                );
                std::process::exit(2);
            }
            match receipt {
                pelt_desktop::WorkspaceReceipt::Mixed | pelt_desktop::WorkspaceReceipt::Chrome => {
                    if !cfg!(all(feature = "scripted", feature = "smolweb")) {
                        eprintln!(
                            "--workspace-receipt {} needs `--features scripted,smolweb`",
                            receipt.id()
                        );
                        std::process::exit(2);
                    }
                    tile_urls = capability_fixture_urls();
                    tile_engine_overrides = match receipt {
                        pelt_desktop::WorkspaceReceipt::Mixed => vec![
                            (1, inker::routing::ENGINE_NEMATIC_GEMTEXT.to_owned()),
                            (2, inker::routing::ENGINE_GENET_LIVERY.to_owned()),
                            (3, inker::routing::ENGINE_GENET_SCRIPTED.to_owned()),
                            (4, inker::routing::ENGINE_SCRYING_WEB.to_owned()),
                        ],
                        // P6 begins with a real automatic HTML route, then uses
                        // its retained control to pin and release that one tile.
                        pelt_desktop::WorkspaceReceipt::Chrome => vec![
                            (1, inker::routing::ENGINE_NEMATIC_GEMTEXT.to_owned()),
                            (3, inker::routing::ENGINE_GENET_SCRIPTED.to_owned()),
                            (4, inker::routing::ENGINE_SCRYING_WEB.to_owned()),
                        ],
                        pelt_desktop::WorkspaceReceipt::Fallback
                        | pelt_desktop::WorkspaceReceipt::LoadingError
                        | pelt_desktop::WorkspaceReceipt::Appearance
                        | pelt_desktop::WorkspaceReceipt::Accessibility
                        | pelt_desktop::WorkspaceReceipt::AccessibilityAddress
                        | pelt_desktop::WorkspaceReceipt::AccessibilityChildren
                        | pelt_desktop::WorkspaceReceipt::AccessibilityEdit
                        | pelt_desktop::WorkspaceReceipt::AccessibilityScroll
                        | pelt_desktop::WorkspaceReceipt::AccessibilityClick
                        | pelt_desktop::WorkspaceReceipt::AccessibilityInput
                        | pelt_desktop::WorkspaceReceipt::NarrowChrome
                        | pelt_desktop::WorkspaceReceipt::ChromeDpi
                        | pelt_desktop::WorkspaceReceipt::Reader
                        | pelt_desktop::WorkspaceReceipt::ReaderAccessibility
                        | pelt_desktop::WorkspaceReceipt::TabardPreview
                        | pelt_desktop::WorkspaceReceipt::TabardReaderPreview => unreachable!(
                            "fallback was handled by the separate workspace receipt branch"
                        ),
                    };
                },
                pelt_desktop::WorkspaceReceipt::Fallback => {
                    tile_urls = vec![fallback_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::LoadingError => {
                    tile_urls = vec![loading_error_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::Appearance => {
                    tile_urls = vec![appearance_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::Accessibility => {
                    tile_urls = vec![accessibility_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::AccessibilityAddress => {
                    let fixture = loading_error_fixture_url();
                    tile_urls = vec![fixture.clone(), fixture];
                },
                pelt_desktop::WorkspaceReceipt::AccessibilityChildren => {
                    if !cfg!(feature = "livery") {
                        eprintln!(
                            "--workspace-receipt accessibility-children needs `--features livery`"
                        );
                        std::process::exit(2);
                    }
                    tile_urls = vec![accessibility_children_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::AccessibilityEdit => {
                    if !cfg!(feature = "livery") {
                        eprintln!(
                            "--workspace-receipt accessibility-edit needs `--features livery`"
                        );
                        std::process::exit(2);
                    }
                    let fixture = accessibility_edit_fixture_url();
                    tile_urls = vec![fixture.clone(), fixture];
                },
                pelt_desktop::WorkspaceReceipt::AccessibilityScroll => {
                    if !cfg!(feature = "livery") {
                        eprintln!(
                            "--workspace-receipt accessibility-scroll needs `--features livery`"
                        );
                        std::process::exit(2);
                    }
                    let fixture = accessibility_scroll_fixture_url();
                    tile_urls = vec![fixture.clone(), fixture];
                },
                pelt_desktop::WorkspaceReceipt::AccessibilityClick => {
                    if !cfg!(feature = "livery") {
                        eprintln!(
                            "--workspace-receipt accessibility-click needs `--features livery`"
                        );
                        std::process::exit(2);
                    }
                    let fixture = accessibility_scroll_fixture_url();
                    tile_urls = vec![fixture.clone(), fixture];
                },
                pelt_desktop::WorkspaceReceipt::AccessibilityInput => {
                    if !cfg!(feature = "livery") {
                        eprintln!(
                            "--workspace-receipt accessibility-input needs `--features livery`"
                        );
                        std::process::exit(2);
                    }
                    let fixture = accessibility_input_fixture_url();
                    tile_urls = vec![fixture.clone(), fixture];
                },
                pelt_desktop::WorkspaceReceipt::NarrowChrome => {
                    tile_urls = vec![loading_error_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::ChromeDpi => {
                    tile_urls = vec![appearance_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::Reader => {
                    if !cfg!(feature = "reader") {
                        eprintln!("--workspace-receipt reader needs `--features reader`");
                        std::process::exit(2);
                    }
                    tile_urls = reader_fixture_urls();
                },
                pelt_desktop::WorkspaceReceipt::ReaderAccessibility => {
                    if !cfg!(all(feature = "livery", feature = "reader")) {
                        eprintln!(
                            "--workspace-receipt reader-accessibility needs `--features livery,reader`"
                        );
                        std::process::exit(2);
                    }
                    let fixture = reader_accessibility_fixture_url();
                    tile_urls = vec![fixture.clone(), fixture];
                    tile_engine_overrides = vec![
                        (1, inker::routing::ENGINE_GENET_READER.to_owned()),
                        (2, inker::routing::ENGINE_GENET_READER.to_owned()),
                    ];
                },
                pelt_desktop::WorkspaceReceipt::TabardPreview => {
                    if !cfg!(feature = "tabard-preview") {
                        eprintln!(
                            "--workspace-receipt tabard-preview needs `--features tabard-preview`"
                        );
                        std::process::exit(2);
                    }
                    tile_urls = vec![appearance_fixture_url()];
                },
                pelt_desktop::WorkspaceReceipt::TabardReaderPreview => {
                    if !cfg!(feature = "tabard-reader-preview") {
                        eprintln!(
                            "--workspace-receipt tabard-reader-preview needs `--features tabard-reader-preview`"
                        );
                        std::process::exit(2);
                    }
                    tile_urls = reader_fixture_urls();
                },
            }
        }
        if capability_receipt {
            if !cfg!(all(feature = "scripted", feature = "smolweb")) {
                eprintln!("--capability-receipt needs `--features scripted,smolweb`");
                std::process::exit(2);
            }
            tile_urls = capability_fixture_urls();
            tile_engine_overrides = vec![
                (1, inker::routing::ENGINE_NEMATIC_GEMTEXT.to_owned()),
                (2, inker::routing::ENGINE_GENET_LIVERY.to_owned()),
                (3, inker::routing::ENGINE_GENET_SCRIPTED.to_owned()),
                (4, inker::routing::ENGINE_SCRYING_WEB.to_owned()),
            ];
        }
        if tile_urls.is_empty() {
            tile_urls.push(url.clone());
        }
        if !capability_receipt && workspace_receipt.is_none() {
            let profile_engine = match selected_engine {
                SelectedEngine::Livery => None,
                SelectedEngine::Reader => {
                    if !cfg!(feature = "reader") {
                        eprintln!("the reader workspace route needs `--features reader`");
                        std::process::exit(2);
                    }
                    Some(inker::routing::ENGINE_GENET_READER)
                },
                SelectedEngine::Scripted => {
                    if !cfg!(feature = "scripted") {
                        eprintln!("the scripted workspace route needs `--features scripted`");
                        std::process::exit(2);
                    }
                    if js_engine == "nova" {
                        if !cfg!(feature = "scripted-nova") {
                            eprintln!("the Nova workspace route needs `--features scripted-nova`");
                            std::process::exit(2);
                        }
                        Some(inker::routing::ENGINE_GENET_SCRIPTED_NOVA)
                    } else {
                        Some(inker::routing::ENGINE_GENET_SCRIPTED)
                    }
                },
            };
            if let Some(engine) = profile_engine {
                let mut profile_overrides = (1..=tile_urls.len())
                    .map(|tile| (tile as u64, engine.to_owned()))
                    .collect::<Vec<_>>();
                // A per-tile choice is more specific than the profile-wide
                // `--engine` choice and therefore wins on duplicates.
                profile_overrides.extend(tile_engine_overrides);
                tile_engine_overrides = profile_overrides;
            }
        }
        #[cfg(feature = "livery")]
        run_workspace_profile(
            tile_urls,
            size,
            frames,
            tile_receipt,
            capability_receipt,
            workspace_receipt,
            artifact,
            appearance_store,
            workspace_size_matrix,
            tile_engine_overrides,
        );
        #[cfg(not(feature = "livery"))]
        {
            eprintln!("the Pelt workspace needs `--features livery`");
            std::process::exit(2);
        }
        return;
    }

    match selected_engine {
        SelectedEngine::Reader => run_reader_profile(url, size, frames),
        SelectedEngine::Livery => {
            // Protocol-native content bypasses the HTML engine while retaining
            // the same headed host. P4 will move this choice into Inker routing.
            #[cfg(feature = "smolweb")]
            if is_smolweb_url(&url) {
                run_smolweb_profile(url, size, frames, product_receipt, artifact);
                return;
            }

            #[cfg(feature = "livery")]
            run_livery_profile(url, size, frames, product_receipt, artifact);
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
            run_scripted_profile(url, js_engine, size, frames, product_receipt, artifact);
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
    match parse_size_value(value) {
        Ok(size) => size,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        },
    }
}

fn parse_size_value(value: &str) -> Result<(u32, u32), String> {
    let Some((width, height)) = value.split_once(['x', 'X']) else {
        return Err(format!(
            "--size expects WxH in physical pixels (got '{value}')"
        ));
    };
    let width = width.parse::<u32>().ok().filter(|value| *value > 0);
    let height = height.parse::<u32>().ok().filter(|value| *value > 0);
    match (width, height) {
        (Some(width), Some(height)) => Ok((width, height)),
        _ => Err(format!(
            "--size expects positive WxH dimensions (got '{value}')"
        )),
    }
}

fn parse_workspace_size_matrix(value: &str) -> Vec<(u32, u32)> {
    match parse_workspace_size_matrix_value(value) {
        Ok(sizes) => sizes,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        },
    }
}

fn parse_workspace_size_matrix_value(value: &str) -> Result<Vec<(u32, u32)>, String> {
    if value.trim().is_empty() {
        return Err("--workspace-size-matrix requires at least two sizes".to_owned());
    }
    let mut sizes = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            return Err("--workspace-size-matrix expects comma-separated WxH values".to_owned());
        }
        let size = parse_size_value(item)
            .map_err(|error| error.replacen("--size", "--workspace-size-matrix", 1))?;
        if sizes.contains(&size) {
            return Err(format!(
                "--workspace-size-matrix contains duplicate size {}x{}",
                size.0, size.1
            ));
        }
        sizes.push(size);
    }
    if sizes.len() < 2 {
        return Err("--workspace-size-matrix requires at least two sizes".to_owned());
    }
    Ok(sizes)
}

fn validate_workspace_size_matrix(
    receipt: Option<pelt_desktop::WorkspaceReceipt>,
    sizes: Option<&[(u32, u32)]>,
    has_single_size: bool,
) -> Result<(), String> {
    let Some(_sizes) = sizes else {
        return Ok(());
    };
    if receipt != Some(pelt_desktop::WorkspaceReceipt::Mixed) {
        return Err("--workspace-size-matrix requires --workspace-receipt mixed".to_owned());
    }
    if has_single_size {
        return Err(
            "--workspace-size-matrix cannot be combined with --size; put the initial size first"
                .to_owned(),
        );
    }
    Ok(())
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

fn parse_product_receipt(value: &str) -> pelt_desktop::ProductReceipt {
    match value {
        "article" => pelt_desktop::ProductReceipt::Article,
        "controls" => pelt_desktop::ProductReceipt::Controls,
        "responsive" => pelt_desktop::ProductReceipt::Responsive,
        "scripted" => pelt_desktop::ProductReceipt::Scripted,
        "text-fragment" => pelt_desktop::ProductReceipt::TextFragment,
        "resources" => pelt_desktop::ProductReceipt::Resources,
        "gemtext" => pelt_desktop::ProductReceipt::Gemtext,
        _ => {
            eprintln!(
                "--product-receipt expects article, controls, responsive, scripted, text-fragment, resources, or gemtext (got '{value}')"
            );
            std::process::exit(2);
        },
    }
}

fn product_receipt_fixture(receipt: pelt_desktop::ProductReceipt) -> String {
    match receipt {
        pelt_desktop::ProductReceipt::Article => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("livery-route")
            .join("index.html")
            .to_string_lossy()
            .into_owned(),
        pelt_desktop::ProductReceipt::Controls => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("p5-controls")
            .join("index.html")
            .to_string_lossy()
            .into_owned(),
        pelt_desktop::ProductReceipt::Responsive => {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join("p5-responsive")
                .join("index.html")
                .to_string_lossy()
                .into_owned()
        },
        pelt_desktop::ProductReceipt::Scripted => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("p5-scripted")
            .join("index.html")
            .to_string_lossy()
            .into_owned(),
        pelt_desktop::ProductReceipt::Resources => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("p5-resources")
            .join("start")
            .join("index.html")
            .to_string_lossy()
            .into_owned(),
        pelt_desktop::ProductReceipt::TextFragment => {
            let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join("text-fragment")
                .join("index.html")
                .to_string_lossy()
                .into_owned();
            format!("{fixture}#:~:text=The%20retained%20text%20fragment%20target")
        },
        pelt_desktop::ProductReceipt::Gemtext => {
            "gemini://pelt.test/p5-gemtext/index.gmi".to_owned()
        },
    }
}

/// Dispatch a smolweb URL to the owned headed document viewer.
#[cfg(feature = "smolweb")]
fn run_smolweb_profile(
    url: String,
    size: Option<(u32, u32)>,
    frames: Option<u32>,
    product_receipt: Option<pelt_desktop::ProductReceipt>,
    artifact: Option<std::path::PathBuf>,
) {
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
    let outcome = match product_receipt {
        Some(receipt) => pelt_desktop::run_smolweb_receipt(
            config.with_product_receipt(
                receipt,
                artifact.expect("the CLI requires an artifact for a product receipt"),
            ),
            include_str!("examples/p5-gemtext/index.gmi"),
        ),
        None => pelt_desktop::run_smolweb_viewer(config),
    };
    match outcome {
        Ok(outcome) => {
            println!(
                "pelt smolweb viewer url={} window={} redraws={} size={}x{}",
                outcome.url,
                outcome.created_window,
                outcome.redraws,
                outcome.size.0,
                outcome.size.1
            );
            if let Some(receipt) = outcome.product_receipt {
                println!(
                    "pelt product receipt={} assertion={} artifact={} digest={:016x}",
                    receipt.id,
                    receipt.assertion,
                    receipt.artifact.display(),
                    receipt.digest
                );
            }
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

/// Dispatch the scripted profile to the on-screen scripted viewer on the chosen JS
/// backend. Present only when built with `--features scripted`.
#[cfg(feature = "scripted")]
fn run_scripted_profile(
    url: String,
    js: String,
    size: Option<(u32, u32)>,
    frames: Option<u32>,
    product_receipt: Option<pelt_desktop::ProductReceipt>,
    artifact: Option<std::path::PathBuf>,
) {
    let Some(engine) = pelt_desktop::ScriptedEngine::parse(&js) else {
        eprintln!("--js expects boa or nova (got '{js}')");
        std::process::exit(2);
    };
    let mut config = pelt_desktop::StaticViewerConfig::new(
        EngineProfile::Scripted,
        pelt_desktop::WindowingMode::Headed,
        url,
    );
    if let Some(receipt) = product_receipt {
        config = config.with_product_receipt(
            receipt,
            artifact.expect("the CLI requires an artifact for a product receipt"),
        );
    }
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    match pelt_desktop::run_scripted_viewer(config, engine) {
        Ok(outcome) => {
            println!(
                "pelt scripted viewer engine={} url={} window={} redraws={} size={}x{}",
                engine.label(),
                outcome.url,
                outcome.created_window,
                outcome.redraws,
                outcome.size.0,
                outcome.size.1,
            );
            if let Some(receipt) = outcome.product_receipt {
                println!(
                    "pelt product receipt={} assertion={} artifact={} digest={:016x}",
                    receipt.id,
                    receipt.assertion,
                    receipt.artifact.display(),
                    receipt.digest
                );
            }
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

/// Dispatch script-free HTML to the owned Livery/Buckram document engine.
#[cfg(feature = "livery")]
fn run_livery_profile(
    url: String,
    size: Option<(u32, u32)>,
    frames: Option<u32>,
    product_receipt: Option<pelt_desktop::ProductReceipt>,
    artifact: Option<std::path::PathBuf>,
) {
    let mut config = pelt_desktop::StaticViewerConfig::new(
        EngineProfile::Livery,
        pelt_desktop::WindowingMode::Headed,
        url,
    );
    if let Some(receipt) = product_receipt {
        config = config.with_product_receipt(
            receipt,
            artifact.expect("the CLI requires an artifact for a product receipt"),
        );
    }
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    match pelt_desktop::run_livery_viewer(config) {
        Ok(outcome) => {
            println!(
                "pelt livery viewer engine=genet.livery url={} window={} redraws={} size={}x{}",
                outcome.url,
                outcome.created_window,
                outcome.redraws,
                outcome.size.0,
                outcome.size.1
            );
            if let Some(receipt) = outcome.product_receipt {
                println!(
                    "pelt product receipt={} assertion={} artifact={} digest={:016x}",
                    receipt.id,
                    receipt.assertion,
                    receipt.artifact.display(),
                    receipt.digest
                );
            }
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

/// Dispatch multiple document URLs into the recursive TileTree + Frisket host.
#[cfg(feature = "livery")]
fn run_workspace_profile(
    urls: Vec<String>,
    size: Option<(u32, u32)>,
    frames: Option<u32>,
    interaction_receipt: bool,
    capability_receipt: bool,
    workspace_receipt: Option<pelt_desktop::WorkspaceReceipt>,
    artifact: Option<std::path::PathBuf>,
    appearance_store: Option<std::path::PathBuf>,
    workspace_size_matrix: Option<Vec<(u32, u32)>>,
    route_overrides: Vec<(u64, String)>,
) {
    let mut config =
        pelt_desktop::WorkspaceViewerConfig::new(urls, pelt_desktop::WindowingMode::Headed);
    if let Some(path) = appearance_store {
        let store = match pelt_desktop::FileAppearanceStore::load(&path) {
            Ok(store) => store,
            Err(error) => {
                eprintln!(
                    "could not load Pelt appearance store {}: {error}",
                    path.display()
                );
                std::process::exit(2);
            },
        };
        config = config.with_appearance_store(store);
    }
    if let Some(receipt) = workspace_receipt {
        config = config.with_workspace_receipt(
            receipt,
            artifact.expect("CLI validates workspace receipt artifacts"),
        );
    }
    if let Some((width, height)) = size {
        config = config.with_size(width, height);
    }
    if let Some(limit) = frames {
        config = config.with_frame_limit(limit);
    }
    if let Some(sizes) = workspace_size_matrix {
        config = config.with_workspace_size_matrix(sizes);
    }
    if interaction_receipt {
        config = config.with_interaction_receipt();
    }
    for (tile, engine) in route_overrides {
        config = config.with_route_override(tile, engine);
    }
    if capability_receipt {
        config = config.with_capability_receipt();
    }
    match pelt_desktop::run_livery_workspace_viewer(config) {
        Ok(outcome) => {
            println!(
                "pelt workspace first_url={} window={} redraws={} size={}x{} tiles={} interaction_receipt={} capability_receipt={} workspace_receipt={} routes={}",
                outcome.first_url,
                outcome.created_window,
                outcome.redraws,
                outcome.size.0,
                outcome.size.1,
                outcome.tile_count,
                outcome.interaction_receipt,
                outcome.capability_receipt,
                outcome
                    .workspace_receipt
                    .as_ref()
                    .map_or("", |receipt| receipt.id),
                outcome.routes.join(","),
            );
            if let Some(receipt) = outcome.workspace_receipt.as_ref() {
                let verified_sizes = receipt
                    .verified_sizes
                    .iter()
                    .map(|(width, height)| format!("{width}x{height}"))
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "pelt workspace product receipt={} assertion={} artifact={} digest={:016x} verified_sizes={verified_sizes}",
                    receipt.id,
                    receipt.assertion,
                    receipt.artifact.display(),
                    receipt.digest,
                );
            }
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    }
}

fn parse_tile_engine(value: &str) -> (u64, String) {
    let Some((tile, engine)) = value.split_once('=') else {
        eprintln!("--tile-engine expects N=engine-id (got '{value}')");
        std::process::exit(2);
    };
    let tile = tile.parse::<u64>().ok().filter(|tile| *tile > 0);
    if let (Some(tile), false) = (tile, engine.trim().is_empty()) {
        (tile, engine.trim().to_owned())
    } else {
        eprintln!("--tile-engine expects N=engine-id (got '{value}')");
        std::process::exit(2);
    }
}

fn capability_fixture_urls() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p4");
    ["native.gmi", "static.html", "scripted.html", "surface.html"]
        .into_iter()
        .map(|name| root.join(name).to_string_lossy().into_owned())
        .collect()
}

fn fallback_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p5-fallback")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn loading_error_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p6-load-error")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn appearance_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p6-appearance")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn accessibility_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p6-accessibility")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn accessibility_children_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p7-accessibility-children")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn accessibility_edit_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p7-accessibility-edit")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn accessibility_scroll_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p7-accessibility-scroll")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn accessibility_input_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p7-accessibility-input")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn reader_fixture_urls() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("reader");
    ["index.html", "neighbor.html"]
        .into_iter()
        .map(|name| root.join(name).to_string_lossy().into_owned())
        .collect()
}

fn reader_accessibility_fixture_url() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("workspace")
        .join("p7-reader-accessibility")
        .join("index.html")
        .to_string_lossy()
        .into_owned()
}

fn parse_workspace_receipt(value: &str) -> pelt_desktop::WorkspaceReceipt {
    match value {
        "mixed" => pelt_desktop::WorkspaceReceipt::Mixed,
        "fallback" => pelt_desktop::WorkspaceReceipt::Fallback,
        "chrome" => pelt_desktop::WorkspaceReceipt::Chrome,
        "loading-error" => pelt_desktop::WorkspaceReceipt::LoadingError,
        "appearance" => pelt_desktop::WorkspaceReceipt::Appearance,
        "accessibility" => pelt_desktop::WorkspaceReceipt::Accessibility,
        "accessibility-address" => pelt_desktop::WorkspaceReceipt::AccessibilityAddress,
        "accessibility-children" => pelt_desktop::WorkspaceReceipt::AccessibilityChildren,
        "accessibility-edit" => pelt_desktop::WorkspaceReceipt::AccessibilityEdit,
        "accessibility-scroll" => pelt_desktop::WorkspaceReceipt::AccessibilityScroll,
        "accessibility-click" => pelt_desktop::WorkspaceReceipt::AccessibilityClick,
        "accessibility-input" => pelt_desktop::WorkspaceReceipt::AccessibilityInput,
        "narrow-chrome" => pelt_desktop::WorkspaceReceipt::NarrowChrome,
        "chrome-dpi" => pelt_desktop::WorkspaceReceipt::ChromeDpi,
        "reader" => pelt_desktop::WorkspaceReceipt::Reader,
        "reader-accessibility" => pelt_desktop::WorkspaceReceipt::ReaderAccessibility,
        "tabard-preview" => pelt_desktop::WorkspaceReceipt::TabardPreview,
        "tabard-reader-preview" => pelt_desktop::WorkspaceReceipt::TabardReaderPreview,
        _ => {
            eprintln!(
                "--workspace-receipt expects mixed, fallback, chrome, loading-error, appearance, accessibility, accessibility-address, accessibility-children, accessibility-edit, accessibility-scroll, accessibility-click, accessibility-input, narrow-chrome, chrome-dpi, reader, reader-accessibility, tabard-preview, or tabard-reader-preview (got '{value}')"
            );
            std::process::exit(2);
        },
    }
}

#[cfg(test)]
mod workspace_receipt_tests {
    use super::*;

    #[test]
    fn workspace_receipt_parser_keeps_named_workspace_receipts() {
        assert_eq!(
            parse_workspace_receipt("mixed"),
            pelt_desktop::WorkspaceReceipt::Mixed
        );
        assert_eq!(
            parse_workspace_receipt("fallback"),
            pelt_desktop::WorkspaceReceipt::Fallback
        );
        assert_eq!(
            parse_workspace_receipt("chrome"),
            pelt_desktop::WorkspaceReceipt::Chrome
        );
        assert_eq!(
            parse_workspace_receipt("loading-error"),
            pelt_desktop::WorkspaceReceipt::LoadingError
        );
        assert_eq!(
            parse_workspace_receipt("appearance"),
            pelt_desktop::WorkspaceReceipt::Appearance
        );
        assert_eq!(
            parse_workspace_receipt("accessibility"),
            pelt_desktop::WorkspaceReceipt::Accessibility
        );
        assert_eq!(
            parse_workspace_receipt("accessibility-address"),
            pelt_desktop::WorkspaceReceipt::AccessibilityAddress
        );
        assert_eq!(
            parse_workspace_receipt("accessibility-children"),
            pelt_desktop::WorkspaceReceipt::AccessibilityChildren
        );
        assert_eq!(
            parse_workspace_receipt("accessibility-edit"),
            pelt_desktop::WorkspaceReceipt::AccessibilityEdit
        );
        assert_eq!(
            parse_workspace_receipt("accessibility-scroll"),
            pelt_desktop::WorkspaceReceipt::AccessibilityScroll
        );
        assert_eq!(
            parse_workspace_receipt("accessibility-click"),
            pelt_desktop::WorkspaceReceipt::AccessibilityClick
        );
        assert_eq!(
            parse_workspace_receipt("accessibility-input"),
            pelt_desktop::WorkspaceReceipt::AccessibilityInput
        );
        assert_eq!(
            parse_workspace_receipt("narrow-chrome"),
            pelt_desktop::WorkspaceReceipt::NarrowChrome
        );
        assert_eq!(
            parse_workspace_receipt("chrome-dpi"),
            pelt_desktop::WorkspaceReceipt::ChromeDpi
        );
        assert_eq!(
            parse_workspace_receipt("reader"),
            pelt_desktop::WorkspaceReceipt::Reader
        );
        assert_eq!(
            parse_workspace_receipt("reader-accessibility"),
            pelt_desktop::WorkspaceReceipt::ReaderAccessibility
        );
        assert_eq!(
            parse_workspace_receipt("tabard-preview"),
            pelt_desktop::WorkspaceReceipt::TabardPreview
        );
        assert_eq!(
            parse_workspace_receipt("tabard-reader-preview"),
            pelt_desktop::WorkspaceReceipt::TabardReaderPreview
        );
        assert!(
            fallback_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p5-fallback/index.html")
        );
        assert!(
            loading_error_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p6-load-error/index.html")
        );
        assert!(
            appearance_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p6-appearance/index.html")
        );
        assert!(
            accessibility_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p6-accessibility/index.html")
        );
        assert!(
            accessibility_children_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p7-accessibility-children/index.html")
        );
        assert!(
            accessibility_edit_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p7-accessibility-edit/index.html")
        );
        assert!(
            accessibility_input_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p7-accessibility-input/index.html")
        );
        let reader = reader_fixture_urls();
        assert_eq!(reader.len(), 2);
        assert!(
            reader[0]
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/reader/index.html")
        );
        assert!(
            reader[1]
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/reader/neighbor.html")
        );
        assert!(
            reader_accessibility_fixture_url()
                .replace('\\', "/")
                .ends_with("/ports/pelt/examples/workspace/p7-reader-accessibility/index.html")
        );
    }

    #[test]
    fn workspace_size_matrix_parser_keeps_order_and_accepts_mixed_case() {
        assert_eq!(
            parse_workspace_size_matrix_value("960x640, 1024X768,1280x800"),
            Ok(vec![(960, 640), (1024, 768), (1280, 800)])
        );
    }

    #[test]
    fn workspace_size_matrix_parser_rejects_bad_or_duplicate_values() {
        for (value, expected) in [
            ("", "at least two"),
            ("960x640", "at least two"),
            ("960x640,960x640", "duplicate"),
            ("960x640,", "comma-separated"),
            ("960x0,1280x800", "positive"),
        ] {
            assert!(
                parse_workspace_size_matrix_value(value)
                    .expect_err("invalid matrix")
                    .contains(expected),
                "{value}"
            );
        }
    }

    #[test]
    fn workspace_size_matrix_requires_mixed_workspace_receipt() {
        let sizes = [(960, 640), (1024, 768)];
        assert!(validate_workspace_size_matrix(None, Some(&sizes), false).is_err());
        assert!(
            validate_workspace_size_matrix(
                Some(pelt_desktop::WorkspaceReceipt::Fallback),
                Some(&sizes),
                false
            )
            .is_err()
        );
        assert!(
            validate_workspace_size_matrix(
                Some(pelt_desktop::WorkspaceReceipt::Mixed),
                Some(&sizes),
                true
            )
            .is_err()
        );
        assert!(
            validate_workspace_size_matrix(
                Some(pelt_desktop::WorkspaceReceipt::Mixed),
                Some(&sizes),
                false
            )
            .is_ok()
        );
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
    _product_receipt: Option<pelt_desktop::ProductReceipt>,
    _artifact: Option<std::path::PathBuf>,
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

Usage: pelt [--engine <profile>] [<url-or-file> ...] [options]

Pelt is Genet's reference host. Livery/Buckram renders script-free HTML by
default (file://, bare paths, data: URLs, and http(s)). `--engine scripted`
runs a page's <script> through the same owned document route (needs
--features scripted). The former viewer, static, and livery-scripted spellings
remain accepted as input aliases. Smoke runners validate the present backends.

Options:
    --engine <livery|reader|scripted>  (diagnostic override; legacy aliases accepted)
    --js <boa|nova>                    (scripted profile; nova needs --features scripted-nova)
    --size <WxH>                       (physical client size)
    --workspace-size-matrix <WxH,...>  (live physical-size matrix; mixed receipt only)
    --frames <N>                       (headed profiles: exit after N presented frames)
    --product-receipt <article|controls|responsive|scripted|text-fragment|resources|gemtext> (bounded fixture + semantic assertion + PNG)
    --artifact <path.png>              (required with a named receipt)
    --appearance-store <path>          (persist Pelt Chrome appearance at this caller-selected path; implies --tiles)
    --tiles                            (route positional URLs in a recursive Frisket workspace)
    --tile-engine <N=engine-id>        (override one workspace tile; repeatable)
    --tile-receipt                     (drive the bounded P3 split/tab/navigation receipt)
    --capability-receipt               (drive the mixed P4 routing receipt)
    --workspace-receipt <mixed|fallback|chrome|loading-error|appearance|accessibility|accessibility-address|accessibility-children|accessibility-edit|accessibility-scroll|accessibility-click|accessibility-input|narrow-chrome|chrome-dpi|reader|reader-accessibility|tabard-preview|tabard-reader-preview> (named workspace receipt; needs --artifact; Tabard receipts need their matching feature)
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
