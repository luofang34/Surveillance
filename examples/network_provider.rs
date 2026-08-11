//! Track traffic from a network provider without a radio decoder.
//!
//! This example imports no link decoder. A provider supplies canonical
//! observations directly. The engine applies the same tracking, fusion, and
//! lifecycle behavior to all input sources.
//!
//! Run it with `cargo run --example network_provider`.

use surveillance::{
    AddressNamespace, DeliveryPath, EngineConfig, ObservationOrigin, ObservationTime,
    PositionObservation, ProducerInstanceId, SourceRef, SurveillanceEngine, TrackDelta, TrackKey,
    TrafficObservation, Wgs84Position,
};
use tracing::{info, warn};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt().with_ansi(false).try_init()?;
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(1))?;

    // A provider reached over the network, on its first connection.
    let provider = SourceRef::new(7, 1, DeliveryPath::NetworkProvider);
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);

    for step in 0..4_u64 {
        // Reports arrive one second apart, each describing a moment 400 ms
        // before it arrived. Declaring that latency is what stops the engine
        // reading a delayed report as though it were live.
        let received_at = 1_000_000 + step * 1_000_000;
        let time = ObservationTime::mapped(received_at, received_at - 400_000, 50_000);

        let mut observation =
            TrafficObservation::new(time, provider, ObservationOrigin::ProviderFused, key);
        observation.position = Some(PositionObservation::Decoded(Wgs84Position {
            latitude_deg: 40.0 + step as f64 * 0.001,
            longitude_deg: -74.0,
        }));
        observation.pressure_altitude_ft = Some(9_000);

        let outcome = engine.ingest(observation, |delta| match delta {
            TrackDelta::Created(snapshot) => {
                info!(track_id = snapshot.id.get(), "created track");
            }
            TrackDelta::Updated(snapshot) => match snapshot.position.as_ref() {
                Some(position) => info!(
                    track_id = snapshot.id.get(),
                    latitude_deg = position.value.latitude_deg,
                    longitude_deg = position.value.longitude_deg,
                    "updated track"
                ),
                None => info!(
                    track_id = snapshot.id.get(),
                    "updated position-absent track"
                ),
            },
            other => info!(delta = ?other, "track lifecycle changed"),
        });
        if let Err(error) = outcome {
            warn!(%error, "provider observation refused");
        }
    }

    // Time advances only when the caller says so. Nothing here reads a clock.
    let quiet_for_a_while = 1_000_000 + 30_000_000;
    engine.advance_time(quiet_for_a_while, |delta| {
        if let TrackDelta::Coasting(snapshot) = delta {
            info!(track_id = snapshot.id.get(), "track is coasting");
        }
    })?;

    info!(track_count = engine.track_count(), "tracking complete");
    Ok(())
}
