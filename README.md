# Surveillance

`Surveillance` is a platform-independent Rust library that converts structured
aviation traffic observations into coherent aircraft tracks. It accepts
canonical observations from local radios, installed panels, network providers,
replay, and simulation alike, and emits runtime-neutral `TrackDelta` and
`TrackSnapshot` values through a versioned, serializable schema.

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
- `TrackDelta` and `TrackSnapshot` serialize through a versioned envelope with
  golden round-trip fixtures, so recordings, native apps, and web viewers share
  one schema.
- Optional adapters map track output to GDL 90 traffic reports and GeoLibre
  aircraft layers. Host-side testing applications (a replay CLI, a MapLibre
  viewer) are feature-gated and never load-bearing for library consumers.

## Workspace

- `surveillance` — umbrella crate re-exporting the portable core. Radio input
  is behind the opt-in `aero-link` feature.
- `crates/surveillance-core` — canonical observations, track engine, identity,
  CPR, fusion, freshness, and the exported schema. Depends on no link decoder.
- `crates/surveillance-aero-link` — normalizes `aero-link` radio receptions
  into canonical observations.

Planned crates for output adapters and host testing tools are tracked in the
repository issues.

## Checks

```sh
scripts/ci.sh              # boundaries, fmt, clippy, tests, rustdoc, release build
scripts/check-no-std.sh    # embedded and wasm target matrix
scripts/check-boundaries.sh  # the engine links no radio decoder
```
