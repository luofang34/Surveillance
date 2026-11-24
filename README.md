# Surveillance

`Surveillance` is a platform-independent Rust library that converts structured
aviation traffic observations into coherent aircraft tracks. It accepts
canonical observations from local radios, installed panels, network providers,
replay, and simulation alike, and emits runtime-neutral `TrackDelta` and
`TrackSnapshotHandle` values through a versioned, serializable schema.

The library owns state that spans traffic receptions: track identity across
address namespaces, global CPR position reconstruction, per-field freshness
and provenance, conservative fusion of 1090 ES, UAT, ADS-R, and TIS-B links,
and deterministic coasting and expiration. It never acquires radio samples,
never owns weather-product lifecycle, and never contains a runtime, socket,
or renderer.

The [architecture document](docs/architecture.md) defines the design: layer
ownership, the track identity model, the cross-link association policy, the
exported schema contract, and the crate map. [AGENTS.md](AGENTS.md) states the
binding rules for anyone implementing against that design.

## Capabilities

- The core library build is `no_std + alloc` and targets MCUs, desktops, and
  `wasm32-unknown-unknown` alike.
- `SurveillanceEngine` is synchronous and single-writer: applications feed it
  observations and a monotonic clock; it emits deltas through caller-supplied
  callbacks. No channels, tasks, locks, or timers live inside the engine.
- `TrackSnapshotHandle` owns an immutable track snapshot. It carries a producer
  identity and a revision. `TrackDelta` and each handle use one versioned
  schema with golden round-trip fixtures.
- `surveillance-geojson` maps track output to typed aircraft features. Its
  GeoJSON serialization is optional.
- `surveillance-gdl90` writes unframed GDL 90 Traffic Report payloads into
  caller-owned storage. It has no transport or gateway state.
- `surveillance-replay` is a separate host application. It converts recorded
  receptions into track or GeoJSON streams.

## Workspace

- `surveillance` — umbrella crate re-exporting the portable core. Radio input
  is behind the opt-in `aero-link` feature.
- `crates/surveillance-core` — canonical observations, track engine, identity,
  CPR, fusion, freshness, and the exported schema. Depends on no link decoder.
- `crates/surveillance-aero-link` — normalizes `aero-link` radio receptions
  into canonical observations.
- `crates/surveillance-gdl90` — maps snapshots to unframed GDL 90 Traffic
  Report payloads.
- `crates/surveillance-geojson` — maps track deltas to renderer-neutral
  aircraft feature changes. GeoJSON encoding is an opt-in feature.
- `crates/surveillance-replay` — replays newline-delimited reception records.
  It is separate from default library builds. See the
  [replay guide](docs/replay.md).

The [capacity guide](docs/capacity.md) defines the bounded association work.

## Checks

```sh
scripts/ci.sh                # boundaries, size, fmt, clippy, tests, docs, release
scripts/check-no-std.sh      # embedded and Wasm target matrix
scripts/check-boundaries.sh  # the engine links no radio decoder
scripts/check-source-size.sh # Rust source file limits
scripts/check-doc-crates.sh  # documents name only crates that exist
```
