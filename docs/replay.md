# Recorded reception replay

The `surveillance-replay` application reads one recorded reception stream.
The input contains one versioned `ReceptionRecord` JSON value on each line.
The application sends traffic receptions through the complete Surveillance
pipeline. It uses recorded arrival time for track coasting and removal.

Run this command to write versioned `TrackRecord` values to standard output:

```sh
cargo run -p surveillance-replay -- replay recording.ndjson
```

The application gets a producer instance identity from the operating system.
Use a fixed identity only when the output must be reproducible:

```sh
cargo run -p surveillance-replay -- replay recording.ndjson \
  --producer-instance-id 71 \
  --output tracks.ndjson
```

Enable the `geojson` feature to write RFC 7946 feature changes:

```sh
cargo run -p surveillance-replay --features geojson -- replay recording.ndjson \
  --format geo-json \
  --output aircraft-features.ndjson
```

The GeoJSON stream is a diagnostic and integration format. It has no renderer.
Pilotage owns traffic and weather composition.

The application reports consumed, produced, skipped, refused, and rejected
field counts through `tracing`. A rejected field does not discard other valid
fields from the same observation. The application writes diagnostics to
standard error. It writes the selected data stream to standard output or to
the specified output file.

The application is a separate host crate. The default `surveillance` and
`surveillance-core` builds do not include its command or encoder dependencies.

## Live capture fixture

The `live_receptions.ndjson` test fixture contains five receptions from one
USB CDC radio session. It contains one UAT ground uplink and four ADS-B
reports for one aircraft. The fixture wraps each captured event in the
`aero-link` schema 6 envelope. It does not change the captured event payload.

The replay test compares the output with `live_track_records.ndjson`. It also
verifies that the UAT ground uplink does not enter the traffic engine.
