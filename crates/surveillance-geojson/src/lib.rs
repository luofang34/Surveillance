//! Renderer-neutral aircraft features from Surveillance track output.
//!
//! [`map_track_delta`] converts a [`surveillance_core::TrackDelta`] into a
//! typed feature change. Position-absent tracks produce no feature until a
//! position becomes available. The optional `geojson` feature adds RFC 7946
//! serialization without changing the typed API.

#![no_std]

extern crate alloc;

mod feature;
#[cfg(feature = "geojson")]
mod geojson;

pub use feature::{AircraftFeature, FeatureDelta, map_track_delta};
#[cfg(feature = "geojson")]
pub use geojson::{GeoJsonError, encode_feature_delta};
