//! Optional RFC 7946 serialization.

use alloc::string::String;

use serde::Serialize;
use surveillance_core::{ProducerInstanceId, SnapshotRevision, TrackId, TrackSnapshot};
use thiserror::Error;

use crate::{AircraftFeature, FeatureDelta};

#[cfg(test)]
mod tests;

/// Failure to encode one typed feature change as GeoJSON.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum GeoJsonError {
    /// An upsert position is outside the WGS84 coordinate range.
    #[error(
        "track {track_id:?} has invalid WGS84 coordinates: latitude {latitude_deg}, longitude {longitude_deg}"
    )]
    InvalidPosition {
        /// Stable identity of the invalid feature.
        track_id: TrackId,
        /// Invalid latitude in degrees.
        latitude_deg: f64,
        /// Invalid longitude in degrees.
        longitude_deg: f64,
    },
    /// JSON serialization failed.
    #[error("could not serialize the aircraft feature as GeoJSON")]
    Serialize {
        /// JSON serialization error.
        #[source]
        source: serde_json::Error,
    },
}

/// Encode one typed feature change as an RFC 7946 Feature object.
///
/// Upserts contain a Point geometry in longitude-latitude order. Stale and
/// remove changes contain a null geometry.
pub fn encode_feature_delta(delta: &FeatureDelta) -> Result<String, GeoJsonError> {
    let feature = GeoJsonFeature::from_delta(delta)?;
    serde_json::to_string(&feature).map_err(|source| GeoJsonError::Serialize { source })
}

#[derive(Serialize)]
struct GeoJsonFeature<'a> {
    #[serde(rename = "type")]
    feature_type: &'static str,
    id: TrackId,
    geometry: Option<PointGeometry>,
    properties: FeatureProperties<'a>,
}

impl<'a> GeoJsonFeature<'a> {
    fn from_delta(delta: &'a FeatureDelta) -> Result<Self, GeoJsonError> {
        match delta {
            FeatureDelta::Upsert(feature) => Self::upsert(feature),
            FeatureDelta::Stale { id } => Ok(Self {
                feature_type: "Feature",
                id: *id,
                geometry: None,
                properties: FeatureProperties::Stale(OperationProperties { operation: "stale" }),
            }),
            FeatureDelta::Remove { id, transfer_to } => Ok(Self {
                feature_type: "Feature",
                id: *id,
                geometry: None,
                properties: FeatureProperties::Remove(RemoveProperties {
                    operation: "remove",
                    transfer_to: *transfer_to,
                }),
            }),
        }
    }

    fn upsert(feature: &'a AircraftFeature) -> Result<Self, GeoJsonError> {
        let position = feature.position().value;
        validate_position(feature.id(), position.latitude_deg, position.longitude_deg)?;

        Ok(Self {
            feature_type: "Feature",
            id: feature.id(),
            geometry: Some(PointGeometry {
                geometry_type: "Point",
                coordinates: [position.longitude_deg, position.latitude_deg],
            }),
            properties: FeatureProperties::Upsert(UpsertProperties {
                operation: "upsert",
                producer_instance_id: feature.producer_instance_id(),
                snapshot_revision: feature.snapshot_revision(),
                snapshot: feature.snapshot(),
            }),
        })
    }
}

fn validate_position(
    track_id: TrackId,
    latitude_deg: f64,
    longitude_deg: f64,
) -> Result<(), GeoJsonError> {
    if latitude_deg.is_finite()
        && longitude_deg.is_finite()
        && (-90.0..=90.0).contains(&latitude_deg)
        && (-180.0..=180.0).contains(&longitude_deg)
    {
        return Ok(());
    }

    Err(GeoJsonError::InvalidPosition {
        track_id,
        latitude_deg,
        longitude_deg,
    })
}

#[derive(Serialize)]
struct PointGeometry {
    #[serde(rename = "type")]
    geometry_type: &'static str,
    coordinates: [f64; 2],
}

#[derive(Serialize)]
#[serde(untagged)]
enum FeatureProperties<'a> {
    Upsert(UpsertProperties<'a>),
    Stale(OperationProperties),
    Remove(RemoveProperties),
}

#[derive(Serialize)]
struct UpsertProperties<'a> {
    operation: &'static str,
    producer_instance_id: ProducerInstanceId,
    snapshot_revision: SnapshotRevision,
    #[serde(flatten)]
    snapshot: &'a TrackSnapshot,
}

#[derive(Serialize)]
struct OperationProperties {
    operation: &'static str,
}

#[derive(Serialize)]
struct RemoveProperties {
    operation: &'static str,
    transfer_to: Option<TrackId>,
}
