/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The on-screen scripted document viewer (`pelt --engine scripted <url>`).
//!
//! The windowed half of the scripted profile: load a [`ScriptedDocument`] on the
//! chosen JS engine and present it through Pelt's public controller and shared
//! single-document shell. The controller retains document replacement and history;
//! the shell drives script timers and the GC tick at frame cadence. Gated on both
//! `present` (the present stack) and `scripted` (the runtime).

use script_engine_api::ScriptEngine;

use crate::scripted::ScriptedEngine;
use crate::static_viewer::{
    ControllerViewerContent, ViewerClock, run_headed_with, validate_receipt_profile,
};
use crate::{StaticViewerConfig, StaticViewerOutcome, WindowingMode};
use genet_documents::{LocalFetcher, ResourceFetchPolicy};
use inker::{SessionRegistry, SurfaceEngineRegistry};
use netrender::Scene;
use pelt_core::{PeltController, PeltControllerConfig};

/// Run the scripted viewer for `config` on `engine`: headed opens a window and
/// presents the live, script-driven document; headless returns immediately with no
/// window (the CI smoke shape). The engine selects the monomorphization — Nova
/// requires the `scripted-nova` feature.
pub fn run_scripted_viewer(
    config: StaticViewerConfig,
    engine: ScriptedEngine,
) -> Result<StaticViewerOutcome, String> {
    validate_receipt_profile(&config, true)?;
    match config.profile.windowing {
        WindowingMode::Headless => Ok(StaticViewerOutcome {
            url: config.url,
            created_window: false,
            redraws: 0,
            size: (0, 0),
            product_receipt: None,
        }),
        WindowingMode::Headed => run_scripted_headed(config, engine),
    }
}

fn run_scripted_headed(
    config: StaticViewerConfig,
    engine: ScriptedEngine,
) -> Result<StaticViewerOutcome, String> {
    match engine {
        ScriptedEngine::Boa => {
            let content = scripted_controller::<script_engine_boa::BoaEngine>(
                &config,
                inker::routing::ENGINE_GENET_SCRIPTED,
                "Scripted · Boa",
            )?;
            run_headed_with(config, content)
        },
        #[cfg(feature = "scripted-nova")]
        ScriptedEngine::Nova => {
            let content = scripted_controller::<script_engine_nova::NovaEngine>(
                &config,
                inker::routing::ENGINE_GENET_SCRIPTED_NOVA,
                "Scripted · Nova",
            )?;
            run_headed_with(config, content)
        },
        #[cfg(not(feature = "scripted-nova"))]
        ScriptedEngine::Nova => Err(
            "the Nova engine needs `--features scripted-nova` (this build links Boa only)"
                .to_string(),
        ),
    }
}

fn scripted_controller<E: ScriptEngine + 'static>(
    config: &StaticViewerConfig,
    engine_id: &str,
    posture: &str,
) -> Result<ControllerViewerContent, String> {
    let (width, height) = config.size.unwrap_or((800, 600));
    let mut registry: SessionRegistry<Scene> = SessionRegistry::new();
    let fetcher = LocalFetcher::with_resource_policy(ResourceFetchPolicy::default());
    registry.register(Box::new(
        genet_documents::ScriptedSessionEngine::<E, _>::new(engine_id, fetcher),
    ));
    let controller = PeltController::new(
        registry,
        SurfaceEngineRegistry::new(),
        PeltControllerConfig::new(engine_id, &config.url, (width, height)),
        ViewerClock::new(),
    )?;
    Ok(ControllerViewerContent::new(
        controller,
        Some(posture.to_owned()),
    ))
}

#[cfg(test)]
mod tests {
    use super::run_scripted_viewer;
    use crate::{ProductReceipt, ScriptedEngine, StaticViewerConfig, WindowingMode};
    use genet_host_api::EngineProfile;

    #[test]
    fn scripted_entrypoint_rejects_a_livery_receipt_before_windowing() {
        let config = StaticViewerConfig::new(
            EngineProfile::Scripted,
            WindowingMode::Headless,
            "about:blank",
        )
        .with_product_receipt(ProductReceipt::Article, "unused.png");
        assert_eq!(
            run_scripted_viewer(config, ScriptedEngine::Boa)
                .expect_err("livery receipt must not enter scripted"),
            "product receipt article is owned by the livery profile"
        );
    }
}
