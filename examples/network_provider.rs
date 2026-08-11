//! Track traffic from a network provider, with no radio anywhere in sight.
//!
//! This example imports no link decoder. It exists to demonstrate that the
//! engine's contract is genuinely source-neutral: a provider, an installed
//! panel, or a simulator supplies canonical observations directly, and gets
//! the same tracking, fusion, and lifecycle a receiver would.
//!
//! Run it with `cargo run --example network_provider`.

use surveillance::{
    AddressNamespace, DeliveryPath, EngineConfig, ObservationOrigin, ObservationTime,
    PositionObservation, ProducerInstanceId, SourceRef, SurveillanceEngine, TrackDelta, TrackKey,
    TrafficObservation, Wgs84Position,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
                println!("created  track {}", snapshot.id.get());
            }
            TrackDelta::Updated(snapshot) => match snapshot.position.as_ref() {
                Some(position) => println!(
                    "updated  track {} at {:.3}, {:.3}",
                    snapshot.id.get(),
                    position.value.latitude_deg,
                    position.value.longitude_deg
                ),
                None => println!("updated  track {} (position absent)", snapshot.id.get()),
            },
            other => println!("{other:?}"),
        });
        if let Err(error) = outcome {
            println!("provider observation refused: {error}");
        }
    }

    // Time advances only when the caller says so. Nothing here reads a clock.
    let quiet_for_a_while = 1_000_000 + 30_000_000;
    engine.advance_time(quiet_for_a_while, |delta| {
        if let TrackDelta::Coasting(snapshot) = delta {
            println!("coasting track {}", snapshot.id.get());
        }
    })?;

    println!("{} track(s) maintained", engine.track_count());
    Ok(())
}
