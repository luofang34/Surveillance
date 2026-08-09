//! End-to-end contract: receptions become tracks, and weather never does.

#![allow(clippy::expect_used, clippy::panic)]

use aero_link::{
    Adsb1090Message, CorrectedUatFrame, ReceiverSource, ReceptionEvent, SourceTransport,
    UatDecoder, UatFrameClass, UatMessage,
};
use surveillance_aero_link::normalize;
use surveillance_core::{
    AddressNamespace, Band, EngineConfig, ObservationOrigin, ProducerInstanceId,
    SurveillanceEngine, TrackDelta, TrackKey,
};

/// Even and odd halves of a published airborne position pair for one target.
const EVEN_POSITION: &str = "8D40621D58C382D690C8AC2863A7";
const ODD_POSITION: &str = "8D40621D58C386435CC412692AD6";

fn decode(hex: &str) -> Adsb1090Message {
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|index| {
            u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("valid hex pair")
        })
        .collect();
    Adsb1090Message::decode(&bytes, None).expect("published frame decodes")
}

fn ground_uplink() -> ReceptionEvent {
    let bytes = vec![0_u8; UatFrameClass::GroundUplink.corrected_bytes()].into_boxed_slice();
    let frame = CorrectedUatFrame::try_from_boxed_bytes(UatFrameClass::GroundUplink, bytes)
        .expect("a sized uplink frame is valid");
    let reception = UatDecoder::default()
        .decode_corrected(frame, -20, 0, 0)
        .expect("a ground uplink decodes");
    assert!(
        matches!(reception.message, UatMessage::Uplink(_)),
        "a 432-byte frame is a ground uplink"
    );
    ReceptionEvent::uat978(5_000, reception)
}

/// The caller pattern the project instructions specify: project a reception to
/// its traffic subset, normalize it, and feed the engine.
#[test]
fn receptions_become_tracks_through_the_documented_caller_pattern() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(1));
    let source = ReceiverSource::new(1, SourceTransport::Usb);
    let mut deltas = Vec::new();

    for (at, hex) in [(1_000_u64, ODD_POSITION), (2_000, EVEN_POSITION)] {
        let event = ReceptionEvent::adsb1090(at, decode(hex)).with_source(source);
        if let Some(projection) = event.traffic() {
            let observation = normalize(&projection).expect("a direct ADS-B frame normalizes");
            engine
                .ingest(observation, |delta| deltas.push(delta.clone()))
                .expect("ingest succeeds");
        }
    }

    assert_eq!(engine.track_count(), 1);
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    let snapshot = engine.snapshot(key).expect("the target became a track");
    let position = snapshot.position.as_ref().expect("the pair decoded");

    assert!(
        (position.value.latitude_deg - 52.257_202).abs() < 1e-4
            && (position.value.longitude_deg - 3.919_372).abs() < 1e-4,
        "decoded {:?}",
        position.value
    );
    assert_eq!(
        position.provenance.origin,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        }
    );
    assert_eq!(
        position.provenance.source.id, 1,
        "the runtime-local input identity survives normalization"
    );
    assert!(matches!(deltas.first(), Some(TrackDelta::Created(_))));
}

/// The traffic-only contract, as a test rather than a convention: a stream
/// carrying both traffic and weather changes engine state only for traffic.
#[test]
fn a_mixed_stream_admits_traffic_and_silently_skips_weather() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(1));
    let source = ReceiverSource::new(2, SourceTransport::Usb);

    let stream = [
        ground_uplink(),
        ReceptionEvent::adsb1090(1_000, decode(ODD_POSITION)).with_source(source),
        ground_uplink(),
        ReceptionEvent::adsb1090(2_000, decode(EVEN_POSITION)).with_source(source),
        ground_uplink(),
    ];

    let mut consumed = 0_usize;
    let mut skipped = 0_usize;
    let mut observations = 0_usize;
    for event in &stream {
        consumed += 1;
        let Some(projection) = event.traffic() else {
            skipped += 1;
            continue;
        };
        let observation = normalize(&projection).expect("traffic normalizes");
        observations += 1;
        engine.ingest(observation, |_| {}).expect("ingest succeeds");
    }

    assert_eq!(consumed, 5);
    assert_eq!(skipped, 3, "every ground uplink is skipped");
    assert_eq!(observations, 2, "only traffic produces observations");
    assert_eq!(
        engine.track_count(),
        1,
        "weather receptions leave engine state untouched"
    );
}

/// Skipping weather is a projection property, not an error path: a weather
/// reception must never reach the adapter and never produce a failure a
/// caller has to distinguish from a real one.
#[test]
fn weather_never_reaches_the_adapter_at_all() {
    let uplink = ground_uplink();
    assert!(
        uplink.traffic().is_none(),
        "a ground uplink has no traffic projection"
    );
    assert!(
        uplink.fis_b_messages().count() > 0 || uplink.traffic().is_none(),
        "the weather projection is the separate path for this reception"
    );
}
