//! How an observation entered the runtime.
//!
//! Delivery is a separate fact from origin. A report that arrives over the
//! network may have been produced by radar, by multilateration, or by another
//! aircraft's own transmission relayed twice; the path it took to reach this
//! process says nothing about which. Keeping them apart is what lets the
//! engine accept a provider or a panel without either pretending to be a
//! radio.
//!
//! Delivery alone never grants authority in fusion. It is recorded so a
//! detail view can explain where a value came from, and so a reconnecting
//! source cannot be confused with its previous incarnation.

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// Radio band an observation arrived on, where that is a meaningful fact.
///
/// A band describes RF reception. Observations that never touched a radio in
/// this runtime — a provider report, a panel-fused track — do not carry one.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// 1090 MHz Mode S and extended-squitter ADS-B.
    Adsb1090,
    /// 978 MHz Universal Access Transceiver.
    Uat978,
}

/// Runtime-local identity of the input that delivered an observation.
///
/// The identity only needs to be stable while one incarnation of that input
/// is alive. Durable hardware or account identity belongs in inventory or
/// recording metadata rather than on every observation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceRef {
    /// Nonzero input identifier, or zero when the producer is unknown.
    pub id: u32,
    /// Incarnation of that input.
    ///
    /// A provider that drops and reconnects, or a receiver that is unplugged
    /// and replaced, comes back as the same `id` with a higher `epoch`. Two
    /// observations from different epochs are from different incarnations
    /// even when the identifier matches, so provenance from a dead session
    /// cannot be refreshed or collided with by its successor.
    #[serde(default)]
    pub epoch: u32,
    /// Kind of path the observation arrived on.
    #[serde(default)]
    pub delivery: DeliveryPath,
}

impl SourceRef {
    /// Name a known runtime-local input at a given incarnation.
    #[must_use]
    pub const fn new(id: u32, epoch: u32, delivery: DeliveryPath) -> Self {
        Self {
            id,
            epoch,
            delivery,
        }
    }

    /// Whether the producer is unknown.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        self.id == 0 && self.delivery == DeliveryPath::Unspecified
    }

    /// Whether two references name the same incarnation of the same input.
    ///
    /// Matching identifiers across different epochs are deliberately not the
    /// same source: that is the whole point of carrying an incarnation.
    #[must_use]
    pub fn is_same_incarnation(&self, other: Self) -> bool {
        self.id == other.id && self.epoch == other.epoch
    }
}

/// Kind of path an observation took into this runtime.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPath {
    /// The producer did not declare a path.
    #[default]
    Unspecified,
    /// A radio receiver attached to this runtime.
    LocalReceiver,
    /// Installed avionics reporting over an aircraft bus or link.
    InstalledAvionics,
    /// A traffic provider reached over a network.
    NetworkProvider,
    /// Recorded observations replayed through the engine.
    Replay,
    /// Synthetic observations from a simulator.
    Simulator,
}
