//! Newline-delimited output for replayed track deltas.

use std::io::Write;

use clap::ValueEnum;
use surveillance_core::{TrackDelta, TrackId, TrackRecord};
use thiserror::Error;

#[cfg(feature = "geojson")]
use surveillance_geojson::{encode_feature_delta, map_track_delta};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    /// Versioned TrackRecord JSON.
    TrackRecord,
    /// RFC 7946 feature JSON. This needs the `geojson` feature.
    GeoJson,
}

impl OutputFormat {
    pub(crate) fn ensure_available(self) -> Result<(), OutputError> {
        #[cfg(not(feature = "geojson"))]
        if self == Self::GeoJson {
            return Err(OutputError::GeoJsonFeatureDisabled);
        }
        Ok(())
    }
}

pub(crate) struct DeltaWriter<W> {
    writer: W,
    format: OutputFormat,
    fault: Option<OutputError>,
}

impl<W: Write> DeltaWriter<W> {
    pub(crate) fn new(writer: W, format: OutputFormat) -> Result<Self, OutputError> {
        format.ensure_available()?;

        Ok(Self {
            writer,
            format,
            fault: None,
        })
    }

    pub(crate) fn write_delta(&mut self, delta: &TrackDelta) {
        if self.fault.is_some() {
            return;
        }
        if let Err(error) = self.try_write_delta(delta) {
            self.fault = Some(error);
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), OutputError> {
        if let Some(error) = self.fault.take() {
            return Err(error);
        }
        self.writer
            .flush()
            .map_err(|source| OutputError::Flush { source })
    }

    fn try_write_delta(&mut self, delta: &TrackDelta) -> Result<(), OutputError> {
        let Some(mut line) = encode_delta(self.format, delta)? else {
            return Ok(());
        };
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .map_err(|source| OutputError::Write {
                track_id: delta.id(),
                source,
            })
    }
}

fn encode_delta(format: OutputFormat, delta: &TrackDelta) -> Result<Option<Vec<u8>>, OutputError> {
    match format {
        OutputFormat::TrackRecord => serde_json::to_vec(&TrackRecord::new(delta.clone()))
            .map(Some)
            .map_err(|source| OutputError::SerializeTrackRecord {
                track_id: delta.id(),
                source,
            }),
        OutputFormat::GeoJson => encode_geojson(delta),
    }
}

#[cfg(feature = "geojson")]
fn encode_geojson(delta: &TrackDelta) -> Result<Option<Vec<u8>>, OutputError> {
    let Some(feature) = map_track_delta(delta) else {
        return Ok(None);
    };
    encode_feature_delta(&feature)
        .map(|json| Some(json.into_bytes()))
        .map_err(|source| OutputError::EncodeGeoJson {
            track_id: delta.id(),
            source,
        })
}

#[cfg(not(feature = "geojson"))]
fn encode_geojson(_: &TrackDelta) -> Result<Option<Vec<u8>>, OutputError> {
    Err(OutputError::GeoJsonFeatureDisabled)
}

#[derive(Debug, Error)]
pub(crate) enum OutputError {
    #[cfg(not(feature = "geojson"))]
    #[error("the GeoJSON output format needs the surveillance-replay geojson feature")]
    GeoJsonFeatureDisabled,
    #[error("could not serialize track {track_id:?} as a TrackRecord")]
    SerializeTrackRecord {
        track_id: TrackId,
        #[source]
        source: serde_json::Error,
    },
    #[cfg(feature = "geojson")]
    #[error("could not encode track {track_id:?} as GeoJSON")]
    EncodeGeoJson {
        track_id: TrackId,
        #[source]
        source: surveillance_geojson::GeoJsonError,
    },
    #[error("could not write the output for track {track_id:?}")]
    Write {
        track_id: TrackId,
        #[source]
        source: std::io::Error,
    },
    #[error("could not flush the replay output")]
    Flush {
        #[source]
        source: std::io::Error,
    },
}
