//! Stable umbrella exports for the portable Surveillance tracking library.
//!
//! Surveillance converts canonical traffic observations into coherent
//! aircraft tracks. The engine is synchronous, single-writer, and
//! runtime-neutral; it emits [`core::TrackDelta`] and [`core::TrackSnapshotHandle`]
//! values that cross process and machine boundaries through a versioned
//! schema.
//!
//! The engine accepts traffic from local radios, installed panels, network
//! providers, replay, and simulation alike. Turning one radio reception into
//! a canonical observation is an adapter's job: enable the `aero-link`
//! feature for that, and a build that does not never links a decoder.
//!
//! The default build is `no_std + alloc`. Host-side testing tools and output
//! adapters are separate, opt-in components. A library consumer does not
//! compile them.

#![cfg_attr(not(feature = "std"), no_std)]

/// Source-neutral tracking engine, track model, and exported schema.
pub mod core {
    pub use surveillance_core::*;
}

/// Normalization of `aero-link` radio receptions into canonical observations.
#[cfg(feature = "aero-link")]
pub mod aero_link {
    pub use surveillance_aero_link::*;
}

pub use surveillance_core::*;
