/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Data-only vocabulary for product surface discovery and admission.
//!
//! A descriptor says what a provider may offer. It deliberately does not carry
//! executable factories, settings values, commands, product snapshots, or live
//! handles. Those stay with the product and its admitted runtime session.
//!
//! This vocabulary is frozen as v1 (2026-08-26), proven by two unrelated
//! products and one erased host. Changes are additive until a v2; the
//! pre-freeze census and rulings live in Mere's
//! `design_docs/mere_docs/implementation_strategy/2026-08-24_knot_shared_surface_and_port_contribution_plan.md`.
//! The census removed the speculative descriptor tail (roles, multiplicity,
//! placement hint, potential capabilities); each returns additively with the
//! consumer that actually reads it.

macro_rules! owned_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}

owned_id!(ProviderId, "A stable, provider-owned surface namespace.");
owned_id!(
    SurfaceId,
    "A stable surface identity within a provider namespace."
);
owned_id!(
    SourceKindId,
    "A product-defined kind of source a surface accepts."
);

/// The cardinality and source kind a surface can be admitted against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceSourceShape {
    /// The surface has no external source at admission time.
    None,
    /// The surface admits one source of the named kind.
    One(SourceKindId),
    /// The surface admits an ordered set of sources of the named kind.
    Many(SourceKindId),
}

/// Stable facts a product publishes before a host admits a surface.
///
/// `accepted_source` is the admission truth, not a parallel claim: a host
/// that also keeps a provider-local admission schema must assert the two
/// agree when the provider registers, so one stated fact governs admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceDescriptor {
    pub provider_id: ProviderId,
    pub surface_id: SurfaceId,
    pub label: String,
    pub accepted_source: SurfaceSourceShape,
}

/// The current availability of one admitted surface session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceAvailability {
    Available,
    Unavailable(SurfaceUnavailableReason),
}

impl SurfaceAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Why a product declined or could not maintain an admitted surface.
///
/// The unproduced variants are deliberate reservations for the broader
/// per-authority status surfaces the shared-surface plan's F0 names: a
/// locked vault, a stale snapshot, an unconfigured provider, an unhealthy
/// resident.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceUnavailableReason {
    Absent,
    Denied,
    Locked,
    Stale,
    Unconfigured,
    Unhealthy,
    Unsupported,
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_contains_only_stable_data_and_availability_is_typed() {
        let descriptor = SurfaceDescriptor {
            provider_id: ProviderId::from("example.provider"),
            surface_id: SurfaceId::from("example.surface.v1"),
            label: "Example surface".to_owned(),
            accepted_source: SurfaceSourceShape::One(SourceKindId::from("example.source")),
        };

        assert_eq!(descriptor.provider_id.as_str(), "example.provider");
        assert_eq!(descriptor.surface_id.as_str(), "example.surface.v1");
        assert!(SurfaceAvailability::Available.is_available());
        assert!(!SurfaceAvailability::Unavailable(SurfaceUnavailableReason::Locked).is_available());
    }
}
