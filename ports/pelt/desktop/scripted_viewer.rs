/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The on-screen scripted document viewer (`pelt --engine scripted <url>`).
//!
//! The windowed half of the scripted profile: load a [`ScriptedDocument`] on the
//! chosen JS engine and present it through the shared [`ViewerApp`](crate::static_viewer)
//! shell — the same winit loop the static viewer uses, here also driving the page's
//! script timers and the GC tick at frame cadence (via [`ViewerContent::pump`]). Gated
//! on both `viewer` (the present stack) and `scripted` (the runtime); the GPU-free
//! document core lives in [`crate::scripted`].

use script_engine_api::ScriptEngine;

#[cfg(feature = "livery-scripted")]
use crate::LiveryScriptedDocument;
use crate::document::LocalFetcher;
use crate::scripted::{ScriptedDocument, ScriptedEngine};
use crate::static_viewer::run_headed_with;
use crate::static_viewer::windowed::ViewerContent;
use crate::static_viewer::{ViewerScrollKey, incumbent_scroll_key};
use crate::{StaticViewerConfig, StaticViewerOutcome, WindowingMode};

impl<E: ScriptEngine> ViewerContent for ScriptedDocument<E> {
    fn frame(&mut self, width: u32, height: u32) -> netrender::Scene {
        ScriptedDocument::frame(self, width, height)
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        ScriptedDocument::scroll_by(self, dx, dy)
    }
    fn scroll_for_key(&mut self, key: ViewerScrollKey) -> bool {
        ScriptedDocument::scroll_for_key(self, incumbent_scroll_key(key))
    }
    fn click_at(&mut self, x: f32, y: f32) -> bool {
        ScriptedDocument::click_at(self, x, y)
    }
    fn pump(&mut self, now_ms: f64) -> bool {
        // Run due timers + the frame-cadence GC tick, then report whether more timers
        // are pending so the shell keeps the frame loop alive for animation / churn.
        let _ = ScriptedDocument::pump(self, now_ms);
        self.has_pending_work()
    }
}

/// The explicit F4 route uses the same window shell but its frame is produced
/// by the live Livery CSSOM session, not `IncrementalLayout`.
#[cfg(feature = "livery-scripted")]
impl<E: ScriptEngine> ViewerContent for LiveryScriptedDocument<E> {
    fn frame(&mut self, width: u32, height: u32) -> netrender::Scene {
        LiveryScriptedDocument::frame(self, width, height)
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        LiveryScriptedDocument::scroll_by(self, dx, dy)
    }
    fn scroll_for_key(&mut self, key: ViewerScrollKey) -> bool {
        LiveryScriptedDocument::scroll_for_key(self, incumbent_scroll_key(key))
    }
    fn click_at(&mut self, x: f32, y: f32) -> bool {
        LiveryScriptedDocument::click_at(self, x, y)
    }
    fn pump(&mut self, now_ms: f64) -> bool {
        let _ = LiveryScriptedDocument::pump(self, now_ms);
        self.has_pending_work()
    }
}

/// Run the scripted viewer for `config` on `engine`: headed opens a window and
/// presents the live, script-driven document; headless returns immediately with no
/// window (the CI smoke shape). The engine selects the monomorphization — Nova
/// requires the `scripted-nova` feature.
pub fn run_scripted_viewer(
    config: StaticViewerConfig,
    engine: ScriptedEngine,
) -> Result<StaticViewerOutcome, String> {
    match config.profile.windowing {
        WindowingMode::Headless => Ok(StaticViewerOutcome {
            url: config.url,
            created_window: false,
            redraws: 0,
            size: (0, 0),
        }),
        WindowingMode::Headed => run_scripted_headed(config, engine),
    }
}

/// Run the opt-in scripted Livery route. Its JavaScript backend selection is
/// identical to the incumbent scripted viewer, but the live frame and CSSOM
/// route are Livery from construction onward.
#[cfg(feature = "livery-scripted")]
pub fn run_livery_scripted_viewer(
    config: StaticViewerConfig,
    engine: ScriptedEngine,
) -> Result<StaticViewerOutcome, String> {
    match config.profile.windowing {
        WindowingMode::Headless => Ok(StaticViewerOutcome {
            url: config.url,
            created_window: false,
            redraws: 0,
            size: (0, 0),
        }),
        WindowingMode::Headed => run_livery_scripted_headed(config, engine),
    }
}

#[cfg(feature = "livery-scripted")]
fn run_livery_scripted_headed(
    config: StaticViewerConfig,
    engine: ScriptedEngine,
) -> Result<StaticViewerOutcome, String> {
    match engine {
        ScriptedEngine::Boa => {
            let doc = LiveryScriptedDocument::<script_engine_boa::BoaEngine>::load(
                LocalFetcher,
                &config.url,
            )?;
            run_headed_with(config, doc)
        },
        #[cfg(feature = "scripted-nova")]
        ScriptedEngine::Nova => {
            let doc = LiveryScriptedDocument::<script_engine_nova::NovaEngine>::load(
                LocalFetcher,
                &config.url,
            )?;
            run_headed_with(config, doc)
        },
        #[cfg(not(feature = "scripted-nova"))]
        ScriptedEngine::Nova => Err(
            "the Nova engine needs `--features scripted-nova` (this build links Boa only)"
                .to_string(),
        ),
    }
}

fn run_scripted_headed(
    config: StaticViewerConfig,
    engine: ScriptedEngine,
) -> Result<StaticViewerOutcome, String> {
    match engine {
        ScriptedEngine::Boa => {
            let doc =
                ScriptedDocument::<script_engine_boa::BoaEngine>::load(&LocalFetcher, &config.url)?;
            run_headed_with(config, doc)
        },
        #[cfg(feature = "scripted-nova")]
        ScriptedEngine::Nova => {
            let doc = ScriptedDocument::<script_engine_nova::NovaEngine>::load(
                &LocalFetcher,
                &config.url,
            )?;
            run_headed_with(config, doc)
        },
        #[cfg(not(feature = "scripted-nova"))]
        ScriptedEngine::Nova => Err(
            "the Nova engine needs `--features scripted-nova` (this build links Boa only)"
                .to_string(),
        ),
    }
}
