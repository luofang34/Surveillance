# Map feature boundary

## Purpose

Surveillance emits `TrackDelta` values. A map uses geographic features.
`surveillance-geojson` converts each applicable delta into one typed feature
change. The crate has no renderer dependency.

The adapter owns semantic mapping. A composition application owns the
renderer binding.

## Typed API

The adapter supplies this renderer-neutral vocabulary:

```rust,ignore
pub enum FeatureDelta {
    Upsert(AircraftFeature),
    Stale { id: FeatureId, revision: SnapshotRevision },
    Remove {
        id: FeatureId,
        revision: SnapshotRevision,
        transfer_to: Option<FeatureId>,
    },
}
```

An `Upsert` places or moves one aircraft feature. A `Stale` change marks a
coasting feature as old. A `Remove` change removes one feature.

`transfer_to` separates a merge from an expired track. A merge supplies the
`FeatureId` of the surviving track. An expiry supplies `None`.

`FeatureId` combines `ProducerInstanceId` and `TrackId`. A producer restart
cannot collide with a feature from an earlier producer. `SnapshotRevision`
orders all changes for one feature. A change to `TrackKey` does not change
the feature identity. Thus, promotion and demotion do not make a new map
feature.

## Position and properties

A create or update with a position produces an `Upsert`. A position-absent
create or update produces no feature change. The first valid position
produces the first `Upsert`.

A positioned track that starts to coast produces `Stale`. A position-absent
track that starts to coast produces no feature change. A removal always
produces `Remove`.

`AircraftFeature` retains the complete `TrackSnapshotHandle`. It also gives
direct access to the position. The position includes observation time,
quality, and provenance. The snapshot includes `ownship_shadow` and all other
exported track properties.

The adapter does not apply display policy. It does not filter ownship shadows.
It does not define icons, labels, grouping, or camera behavior.

## GeoJSON serialization

GeoJSON (RFC 7946) is an optional serialization. It is not the Rust API. The
`geojson` feature adds `encode_feature_delta`.

An upsert becomes a GeoJSON `Feature` with a Point geometry. The coordinate
order is longitude and then latitude. Stale and remove changes use a null
geometry. The feature identity contains the producer and track values. The
properties include the revision, operation, and applicable track properties.

The default crate build does not link `serde_json`. The default build and the
`geojson` build are both `no_std + alloc`.

## Application boundary

Pilotage owns the composition application. The application combines traffic
features and weather features. It also owns the renderer binding and display
policy. Surveillance does not contain a separate preview application.
