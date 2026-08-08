//! Where an observation entered the runtime.
//!
//! Surveillance accepts traffic from local radios, installed avionics,
//! internet providers, replay, and simulation. The engine therefore describes
//! a source by what a consumer can reason about — a runtime-local identity
//! and the kind of path it arrived on — and never by a receiver, socket,
//! provider, or SDK object. Those belong to whichever adapter produced the
//! observation.

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
/// The identity only needs to be stable while the runtime is alive. Durable
/// hardware or account identity belongs in inventory or recording metadata
/// rather than on every observation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceRef {
    /// Nonzero input identifier, or zero when the producer is unknown.
    pub id: u32,
    /// Kind of path the observation arrived on.
    #[serde(default)]
    pub transport: SourceTransport,
}

impl SourceRef {
    /// Name a known runtime-local input.
    #[must_use]
    pub const fn new(id: u32, transport: SourceTransport) -> Self {
        Self { id, transport }
    }

    /// Whether the producer is unknown.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        self.id == 0 && self.transport == SourceTransport::Unspecified
    }
}

/// Kind of path an observation arrived on.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTransport {
    /// The producer did not attach transport metadata.
    #[default]
    Unspecified,
    /// USB host controller and a receiver-specific class driver.
    Usb,
    /// Asynchronous serial byte stream.
    Uart,
    /// SPI peripheral or coprocessor link.
    Spi,
    /// Parallel, PIO, or memory-mapped sample interface.
    Parallel,
    /// Network stream carrying observations.
    Network,
    /// Recorded data replayed through the engine.
    Replay,
}
