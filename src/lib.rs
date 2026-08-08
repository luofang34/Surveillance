//! Stable umbrella exports for the portable Surveillance tracking library.
//!
//! Surveillance converts the traffic subset of `aero_link::ReceptionEvent`
//! into coherent aircraft tracks. The engine is synchronous, single-writer,
//! and runtime-neutral; it emits [`core::TrackDelta`] and
//! [`core::TrackSnapshot`] values that cross process and machine boundaries
//! through a versioned schema.
//!
//! The default build is `no_std + alloc`. Host-side testing applications
//! (a replay CLI, a MapLibre viewer) and output adapters (GDL 90, GeoLibre)
//! are opt-in and never load-bearing for library consumers.

#![cfg_attr(not(feature = "std"), no_std)]

/// Portable tracking engine, track model, and exported schema.
pub mod core {
    pub use surveillance_core::*;
}

pub use surveillance_core::*;
