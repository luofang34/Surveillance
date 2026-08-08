#![allow(clippy::expect_used, clippy::panic)]

use crate::source::{Band, DeliveryPath, SourceRef};

#[test]
fn an_unnamed_source_is_unspecified() {
    let unknown = SourceRef::default();
    assert!(unknown.is_unspecified());
    assert_eq!(unknown.id, 0);
    assert_eq!(unknown.delivery, DeliveryPath::Unspecified);
    assert_eq!(unknown.epoch, 0);
}

#[test]
fn a_named_source_is_not_unspecified() {
    assert!(!SourceRef::new(1, 0, DeliveryPath::LocalReceiver).is_unspecified());
    assert!(
        !SourceRef::new(0, 0, DeliveryPath::NetworkProvider).is_unspecified(),
        "a known transport names the path even without an identifier"
    );
    assert!(
        !SourceRef::new(7, 0, DeliveryPath::Unspecified).is_unspecified(),
        "a known identifier names the input even without a transport"
    );
}

#[test]
fn a_source_serializes_by_identity_incarnation_and_path() {
    let source = SourceRef::new(3, 7, DeliveryPath::NetworkProvider);
    let json = serde_json::to_string(&source).expect("source serializes");
    assert_eq!(json, r#"{"id":3,"epoch":7,"delivery":"network_provider"}"#);
    assert_eq!(
        serde_json::from_str::<SourceRef>(&json).expect("source decodes"),
        source
    );
}

#[test]
fn a_band_serializes_by_its_frequency_name() {
    for (band, expected) in [
        (Band::Adsb1090, "\"adsb1090\""),
        (Band::Uat978, "\"uat978\""),
    ] {
        let json = serde_json::to_string(&band).expect("band serializes");
        assert_eq!(json, expected);
        assert_eq!(
            serde_json::from_str::<Band>(&json).expect("band decodes"),
            band
        );
    }
}

#[test]
fn a_reconnected_source_is_not_its_previous_incarnation() {
    let before = SourceRef::new(4, 1, DeliveryPath::NetworkProvider);
    let after = SourceRef::new(4, 2, DeliveryPath::NetworkProvider);

    assert!(before.is_same_incarnation(before));
    assert!(
        !before.is_same_incarnation(after),
        "a reconnect must not be mistaken for the session it replaced"
    );
    assert_ne!(
        before, after,
        "provenance from a dead session never compares equal to its successor"
    );
}
