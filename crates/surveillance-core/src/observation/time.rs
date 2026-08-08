//! When a surveillance fact was observed, as distinct from when it arrived.
//!
//! Receiving a report now does not mean the target was observed now. A local
//! radio reception is close enough that the difference does not matter; a
//! network provider's report may describe a position from seconds ago, and a
//! provider that will not say when it measured has told you nothing about
//! age at all.
//!
//! Every time in this module lives in one caller-supplied monotonic domain.
//! Raw UTC or provider clocks are never compared inside the engine: an
//! adapter that trusts its source maps that source's clock into the local
//! domain and declares the uncertainty of the mapping, and an adapter that
//! cannot do so says so explicitly.

use serde::{Deserialize, Serialize};

/// How an observation's time was established.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimeBasis {
    /// The observation was made essentially at ingress, as for a local radio
    /// reception where propagation and decoding are negligible.
    LocalIngress,
    /// A trusted adapter mapped the source's own observation time into the
    /// local monotonic domain.
    Mapped {
        /// Half-width of the mapping's uncertainty, in microseconds.
        ///
        /// Ageing uses the pessimistic end of this interval, so a loosely
        /// mapped time is treated as older than a tightly mapped one rather
        /// than as equally current.
        uncertainty_micros: u64,
    },
    /// The source did not establish when the measurement was made.
    ///
    /// The recorded time is ingress, which is an upper bound on freshness
    /// and nothing more. Such a value never wins a field against one whose
    /// time is established and still fresh.
    UnknownSourceTime,
}

impl TimeBasis {
    /// Whether the recorded time actually describes when the measurement was
    /// made, rather than merely when it arrived.
    #[must_use]
    pub fn is_established(self) -> bool {
        matches!(self, Self::LocalIngress | Self::Mapped { .. })
    }

    /// Extra age to assume because the time is imprecise.
    #[must_use]
    pub fn uncertainty_micros(self) -> u64 {
        match self {
            Self::Mapped { uncertainty_micros } => uncertainty_micros,
            Self::LocalIngress | Self::UnknownSourceTime => 0,
        }
    }
}

/// When one observation was made, in the local monotonic domain.
///
/// Nothing can be measured after it is received, so the invariant
/// `observed_at_micros <= received_at_micros` holds however the value was
/// produced — including when it was decoded from a record. A source clock
/// running ahead would otherwise deserialize into a value that is
/// permanently fresh and outranks live local data, and the replay path is
/// exactly where recorded mapped times arrive.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationTime {
    /// Monotonic time the observation entered this runtime.
    ///
    /// This is always known, because the runtime stamps it. It bounds
    /// freshness even when nothing else about the timing is trustworthy.
    pub received_at_micros: u64,
    /// Monotonic time the measurement was made, as established by `basis`.
    pub observed_at_micros: u64,
    /// How `observed_at_micros` was established.
    pub basis: TimeBasis,
}

/// Wire form of [`ObservationTime`], clamped on the way in so a decoded
/// value cannot carry a measurement time from after its own arrival.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename = "ObservationTime")]
struct WireObservationTime {
    received_at_micros: u64,
    observed_at_micros: u64,
    basis: TimeBasis,
}

impl<'de> Deserialize<'de> for ObservationTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = WireObservationTime::deserialize(deserializer)?;
        Ok(Self {
            received_at_micros: wire.received_at_micros,
            observed_at_micros: wire.observed_at_micros.min(wire.received_at_micros),
            basis: wire.basis,
        })
    }
}

impl ObservationTime {
    /// A locally received observation, measured at ingress.
    #[must_use]
    pub const fn local(received_at_micros: u64) -> Self {
        Self {
            received_at_micros,
            observed_at_micros: received_at_micros,
            basis: TimeBasis::LocalIngress,
        }
    }

    /// An observation whose source time a trusted adapter mapped into the
    /// local domain.
    ///
    /// A mapping that lands after ingress is clamped to ingress: nothing can
    /// have been measured after it was received, and letting a skewed
    /// provider clock claim the future would let it outrank live local data.
    #[must_use]
    pub fn mapped(
        received_at_micros: u64,
        observed_at_micros: u64,
        uncertainty_micros: u64,
    ) -> Self {
        Self {
            received_at_micros,
            observed_at_micros: observed_at_micros.min(received_at_micros),
            basis: TimeBasis::Mapped { uncertainty_micros },
        }
    }

    /// An observation whose source would not say when it was measured.
    #[must_use]
    pub const fn unknown_source_time(received_at_micros: u64) -> Self {
        Self {
            received_at_micros,
            observed_at_micros: received_at_micros,
            basis: TimeBasis::UnknownSourceTime,
        }
    }

    /// Age at `now_micros`, assuming the pessimistic end of any uncertainty.
    #[must_use]
    pub fn age_micros(&self, now_micros: u64) -> u64 {
        now_micros
            .saturating_sub(self.observed_at_micros)
            .saturating_add(self.basis.uncertainty_micros())
    }

    /// Whether the observation is within `window_micros` of `now_micros`.
    #[must_use]
    pub fn is_fresh(&self, now_micros: u64, window_micros: u64) -> bool {
        self.age_micros(now_micros) <= window_micros
    }

    /// Whether the recorded observation time describes the measurement.
    #[must_use]
    pub fn is_established(&self) -> bool {
        self.basis.is_established()
    }
}
