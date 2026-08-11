//! Exercise position association at the default track bound.
//!
//! Build this example in release mode. Run the executable with an external
//! process timer to measure elapsed time and peak resident memory.

use std::error::Error;
use std::hint::black_box;

use surveillance_core::{
    AddressNamespace, Band, DEFAULT_MAX_TRACKS, EngineConfig, ObservationOrigin, ObservationTime,
    PositionObservation, ProducerInstanceId, SourceRef, SurveillanceEngine, TrackKey,
    TrafficObservation, VelocityObservation, Wgs84Position,
};

const ITERATIONS: u64 = 100;
const SUBJECT: TrackKey = TrackKey::new(AddressNamespace::Icao, 0x00ab_cdef);

fn positioned(at_micros: u64, key: TrackKey, latitude_deg: f64) -> TrafficObservation {
    let origin = match key.namespace {
        AddressNamespace::Icao => ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        _ => ObservationOrigin::Tisb { band: Band::Uat978 },
    };
    let mut observation = TrafficObservation::new(
        ObservationTime::local(at_micros),
        SourceRef::default(),
        origin,
        key,
    );
    observation.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg,
        longitude_deg: 0.0,
    }));
    observation.pressure_altitude_ft = Some(10_000);
    let mut velocity = VelocityObservation::default();
    velocity.ground_speed_kt = Some(100.0);
    observation.velocity = Some(velocity);
    observation
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(1));
    engine.ingest(positioned(1, SUBJECT, 0.0), |_| {})?;
    for index in 1..DEFAULT_MAX_TRACKS {
        let key = TrackKey::new(AddressNamespace::TisbTrackFile, index as u32);
        engine.ingest(positioned(1, key, 45.0), |_| {})?;
    }

    for at_micros in 2..ITERATIONS + 2 {
        engine.ingest(positioned(at_micros, SUBJECT, 0.0), |_| {})?;
    }

    if engine.track_count() != DEFAULT_MAX_TRACKS {
        return Err(std::io::Error::other("the capacity workload lost a track").into());
    }
    black_box(engine);
    Ok(())
}
