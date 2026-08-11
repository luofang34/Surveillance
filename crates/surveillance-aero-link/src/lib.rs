//! Turning one borrowed `aero-link` traffic projection into a canonical
//! [`TrafficObservation`](surveillance_core::TrafficObservation).
//!
//! This crate is the seam between one radio reception and the source-neutral
//! engine. `surveillance-core` knows nothing about `aero-link`, so a
//! consumer that never receives RF never links a decoder; a consumer that
//! does adds this adapter and nothing changes in the engine.
//!
//! Normalization is stateless and synchronous: it classifies the reported
//! identity into an address namespace and a link class, converts measured
//! fields into explicit units, and carries provenance through. It decodes no
//! positions from CPR and remembers nothing between receptions — everything
//! that spans receptions belongs to the engine.
//!
//! Classification reads `aero-link`'s typed fields — the UAT address
//! qualifier and the DF 18 control field — never raw message bytes. A
//! reception that names no target, or that carries service-management data
//! rather than a target, produces a typed error instead of a fabricated
//! identity.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adsb1090;
mod normalize;
pub mod replay;
mod source;
mod uat;
mod vocabulary;

pub use normalize::{NormalizeError, normalize};

#[cfg(test)]
mod tests;
