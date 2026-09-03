/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Application host contracts above Genet.
//!
//! The settings projection a host renders for a configured product, and the
//! data-only vocabulary by which a product surface is discovered and admitted.
//! Neither names an engine type: a Mere host implements these and hands the
//! results downward, and Genet never calls up into them. They were the
//! application half of `genet-host-api` until the platform boundary plan
//! (mere `design_docs/mere_docs/implementation_strategy/`
//! `2026-09-02_platform_boundary_and_repository_topology_plan.md`, P1) split
//! that crate by authority; this crate is Mere's and lives here only until the
//! plan moves it.

pub mod settings;
pub mod surface;

pub use surface::{
    ProviderId, SourceKindId, SurfaceAvailability, SurfaceDescriptor, SurfaceId,
    SurfaceSourceShape, SurfaceUnavailableReason,
};
