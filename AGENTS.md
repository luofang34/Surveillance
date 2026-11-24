# Surveillance Project Instructions

## Primary goal

Build a platform-independent surveillance library that converts structured
aviation traffic observations into coherent aircraft tracks. It accepts
canonical observations from local radios, installed panels, network
providers, replay, and simulation alike, and emits runtime-neutral
`TrackDelta` and `TrackSnapshot` values.

Surveillance must outlive every receiver, MCU, operating system, USB stack,
async runtime, EFB, map renderer, and non-traffic aviation product used with it.

## Documentation language

All project-authored documentation MUST use ASD-STE100 Simplified Technical
English.

- Write in English.
- Use an approved word when it has the required meaning.
- Use one word for one meaning.
- Use short, direct sentences.
- Use active voice when you can identify the actor.
- Give one instruction or one main statement in each sentence.
- Define a necessary technical name before you use it.
- Use the same technical name in all related documents and diagrams.
- Keep code identifiers, protocol fields, quotations, and product names exact.
- Apply these rules to all new text and to all text that you change.

Do not copy noncompliant legacy text into a new document. Record a broad legacy
rewrite as separate work.

## Hard boundary

Surveillance starts after aviation-link reception and traffic classification.
It never acquires radio samples and never owns general UAT products.

The core must not depend on or contain:

- `nusb`, libusb, serial discovery, USB descriptors, FTDI, CDC, RTL-SDR, tuner
  configuration, RF demodulation, UAT FEC, or UAT/FIS-B framing.
- ESP32, RP2350, iPadOS, macOS, Linux, Tokio, Embassy, DriverKit, or
  `embedded-hal` device objects.
- Device paths, USB VID/PID values, firmware names, or hardware buffer sizes.
- FIS-B product reassembly, weather-product lifecycle, or weather map layers.
- General event fan-out, recording, replay storage, or application routing.

Those concerns belong to `../aero-link`, a weather service, or platform
application crates.

`surveillance-core` depends on no link decoder at all. Translating one radio
reception into canonical terms is an adapter's job, so the engine's public
model never names a receiver contract and a non-radio source never has to
imitate one:

```toml
[dependencies]
surveillance-core = { path = "crates/surveillance-core" }
# Only where RF is actually received:
surveillance-aero-link = { path = "crates/surveillance-aero-link" }
```

`scripts/check-boundaries.sh` gates this: it fails if the engine gains a
dependency edge to a decoder, mentions one in its sources, or if the umbrella
pulls the adapter in by default.

## Input adapter

Do not make the stateful tracking engine ingest every aviation product.
`aero-link` already exposes the traffic subset of one event as a borrowed
projection through `ReceptionEvent::traffic()`; FIS-B and other non-traffic
uplink content never appear in it. The `surveillance-aero-link` crate is a
small, synchronous adapter that normalizes one projection into the engine's
canonical `TrafficObservation`:

```rust,ignore
if let Some(projection) = event.traffic() {
    let observation = surveillance_aero_link::normalize(&projection)?;
    engine.ingest(observation, |delta| consume_track_delta(delta))?;
}
```

Other sources have their own adapters and reach the same `ingest` call. The
engine cannot tell them apart except through the provenance they declare.

The adapter may understand Mode S/ADS-B, UAT traffic, ADS-R, and TIS-B message
types, and it classifies through `aero-link`'s typed fields — address
qualifiers, downlink formats, and the classified extended-squitter address —
rather than parsing raw message bytes. The application router remains
responsible for delivering the same reception to a weather service, recorder,
diagnostics service, or multiple consumers.

The canonical measurement type `TrafficObservation` belongs to Surveillance
and names no decoder vocabulary; `aero-link`'s borrowed projection is
`TrafficProjection`.

## Ownership of behavior

`aero-link` owns one-reception and radio-stream work:

- Mode S and UAT uplink/downlink framing, correction, demodulation, and parsing.
- Band-specific fields, unknown payloads, and raw-frame representation.
- Monotonic reception timestamps, receiver source identity, and radio quality.

Surveillance owns state across traffic receptions:

- Address namespaces and track identity.
- Global CPR even/odd pairing and position reconstruction.
- Per-field freshness, ordering, confidence, and expiration.
- Conservative fusion and deduplication of 1090 ES, UAT, ADS-R, and TIS-B.
- Identity evolution from an unknown target to a known target without changing
  track identity unnecessarily.
- Track creation, update deltas, coasting, and removal.

Do not move stateful tracking into `aero-link`. Do not move weather lifecycle or
GDL 90 gateway composition into Surveillance.

## Input semantics

- Use `ReceptionEvent.received_at_micros` for ordering, age, and expiration. It
  is monotonic runtime time, not UTC.
- Never compare UAT firmware timestamps with Mode S sample indexes or use either
  as a cross-radio clock.
- Preserve `ReceiverSource` through `TrafficObservation` provenance.
- Treat observations as potentially duplicated, delayed, or out of order.
- Do not assume one receiver per band.
- Do not identify or merge an aircraft by callsign alone.
- Track keys preserve address namespace and qualifier. Identities in different
  namespaces (track file, self-assigned, surface vehicle) are never merged
  merely because their numeric addresses match. ICAO-qualified identities are
  one namespace across links: direct ADS-B, ADS-R rebroadcast, and TIS-B with
  an ICAO address associate on address equality, and quality/age rules decide
  which link's fields win.
- Never invent unavailable accuracy, integrity, altitude, velocity, or identity
  fields. Represent absence explicitly.

## Core model

Keep a single-writer `SurveillanceEngine` as the authoritative mutable state.
Its public API is synchronous and runtime-neutral:

```rust,ignore
engine.ingest(observation, |delta| consume_track_delta(delta))?;
engine.advance_time(now_micros, |delta| consume_expiration(delta));
```

Prefer callbacks or caller-owned output buffers on hot paths. Do not put
channels, tasks, locks, executors, sockets, or global registries inside the
engine. Platform applications feed it from their own routing and queue policy.

Separate at least these concepts:

- `TrafficObservation`: one normalized traffic measurement with provenance.
- `TrackKey`: address plus address namespace/qualifier.
- `IdentitySet`: compatible known and candidate identities for one track.
- `TrackState`: latest fused surveillance state.
- `TimedField<T>`: value, observation time, quality, and provenance.
- `TrackDelta`: created, updated, coasted, or removed output.
- `TrackSnapshot`: immutable consumer-facing state.

Use WGS84 latitude/longitude and explicit units in field names or newtypes. Do
not store ambiguous `speed`, `heading`, `altitude`, or `time` fields.

A track may exist without any position: a Mode S target can supply only
altitude, squawk, or identity. State, snapshots, and deltas represent
position-absent tracks explicitly, and position history begins whenever the
first valid position arrives.

## CPR, identity, and fusion rules

- Implement global airborne CPR here because it requires even/odd history
  across receptions.
- Validate CPR pair age, parity, latitude zone, and decoded range before updating
  a track.
- Keep local CPR separate and require a trustworthy reference position.
- Give each field its own freshness window; a callsign update cannot make an old
  position fresh.
- Resolve conflicts using documented quality and age rules. Do not use
  last-writer-wins when an older observation arrives later.
- Keep enough provenance to explain which reception supplied each exported
  position, altitude, velocity, and identity.
- Make expiration deterministic through `advance_time`; never read wall-clock
  time or start a background timer inside the engine.
- Promote unknown identities conservatively. Cross-namespace numeric address
  equality, proximity, or callsign equality alone is insufficient evidence for
  merging; same-namespace ICAO equality associates per the table in the next
  section.

## Cross-link association and identity transitions

A physical aircraft can appear through several links and namespaces during one
flight: direct 1090 ES, direct UAT, ADS-R rebroadcast, TIS-B with an ICAO
address, a radar-derived TIS-B track file, or an anonymized self-assigned
address. Association policy is explicit:

| Identity pair | Rule |
| --- | --- |
| ICAO ↔ ICAO across links (direct ADS-B, ADS-R, TIS-B with ICAO, 1090 and UAT) | Associate on address equality; the qualifier exists to assert this. |
| Track file ↔ ICAO | Kinematic correlation with hysteresis; never on address, proximity, or callsign alone. |
| Self-assigned or anonymous ↔ anything | Kinematic correlation with hysteresis. |
| Vehicle ↔ non-vehicle | Never. |

Kinematic correlation requires sustained agreement — positions consistent with
both tracks' integrity bounds plus velocity and altitude agreement — over
several consecutive observations inside a bounded window, with hysteresis so a
single coincidental alignment cannot merge tracks and a single outlier cannot
split them.

Transitions preserve track history. A track keeps one stable internal identity
while its primary key changes; `IdentitySet` records the associated identities.
When a radar-derived track file is correlated with a newly acquired cooperative
ICAO identity, consumers see the same track re-keyed, not a removal plus a
creation. When the cooperative source expires while a correlated
non-cooperative source continues, the track demotes to the surviving identity,
coasts under the normal freshness rules, and keeps its history. Both directions
are deliberate, evidence-driven decisions, and both must be reversible without
identity churn.

Ground infrastructure rebroadcasts ownship: an aircraft's own ADS-B
transmission returns as ADS-R or TIS-B and would otherwise display as a phantom
intruder. The engine accepts a caller-supplied set of ownship identities as
plain data, labels matching tracks as ownship shadows in state and deltas, and
suppresses nothing itself. Ownship selection, navigation, and display policy
stay in the application.

## GDL 90 boundary

The Surveillance core exports tracks, not a complete GDL 90 service.

An optional `surveillance-gdl90` adapter may map `TrackSnapshot` values to GDL 90
traffic reports. It can encode payload fields into caller-owned storage, but it
must not own UDP, Bluetooth, serial, sockets, heartbeat scheduling, FIS-B uplink
messages, or weather-product state.

A complete GDL 90 gateway is a composition-layer service. It receives:

- traffic reports from Surveillance output
- weather or raw UAT uplink data from a weather/link consumer
- ownship navigation and geometric altitude from a navigation service
- UTC and status inputs from a time/system service

The gateway owns CRC, framing, escaping, heartbeat scheduling, output transport,
and backpressure. It must use specification-defined unavailable encodings rather
than fabricating missing surveillance values.

## Exported schema, provenance, and time

`TrackDelta` and `TrackSnapshot` cross process and machine boundaries — native
apps, a web viewer, recorders — so their serialization is part of the core
contract, not an optional feature. Use a versioned envelope with private
fields, validation before access, and golden round-trip fixtures with
bit-exact float values. Additive growth and version-bump rules are documented
alongside the schema.

Every exported field keeps provenance that a detail view can display:
originating link and band, address qualifier, `ReceiverSource`, and the
observation's monotonic time. A snapshot may additionally reference the
receptions that supplied its current position, altitude, velocity, and
identity, so an application can show the underlying frames from its own
recording without Surveillance storing them.

All exported times are runtime-monotonic microseconds, consistent with
`ReceptionEvent.received_at_micros`. A service that exports deltas off the
runtime attaches one monotonic-origin-to-UTC correspondence per stream at that
boundary. The engine itself never reads a wall clock.

## GeoLibre boundary

The optional `surveillance-geojson` adapter converts each applicable
`TrackDelta` into one aircraft feature change: an upsert, a stale change, or a
removal. The adapter has no renderer dependency. Do not import a renderer into
the CPR, fusion, identity, or track modules.

Weather features are produced by a separate weather-to-GeoLibre adapter. The
application may display aircraft and weather layers together, but that UI
composition does not make weather part of Surveillance.

Geometry uses WGS84 coordinates. Icons, colors, labels, clustering, camera
state, and rendering policy belong to the GeoLibre application adapter.

Position-absent tracks produce no geographic feature; they remain visible
through snapshots and are upserted onto the map when a first valid position
arrives.

## Portability and dependencies

- The core defaults to `no_std + alloc`; gate `std` integrations behind
  features.
- The core and its adapters must build for `wasm32-unknown-unknown` so a web
  viewer can embed the engine or decode exported streams; keep the target in
  CI.
- `TrackDelta` and `TrackSnapshot` require `serde` because the exported schema
  is a core contract; purely internal types keep it optional.
- Library error enums use `thiserror`; `anyhow` is limited to CLI entry points.
- Do not add Tokio, async runtimes, sockets, USB crates, or board HALs to core.
- Use `embedded-hal` only in a hardware/application crate for an actual GPIO,
  SPI, I2C, delay, reset, or power-control peripheral. It is not a surveillance
  or GDL 90 abstraction.

## Suggested workspace boundaries

Use domain names and the `foo.rs` plus `foo/` layout; do not use `mod.rs`.

```text
crates/
  surveillance-aero-link/
  surveillance-core/
    src/
      lib.rs
      observation.rs
      engine.rs
      track.rs
      identity.rs
      cpr.rs
      fusion.rs
      freshness.rs
      projection.rs
  surveillance-gdl90/
  surveillance-geojson/
  surveillance-replay/
```

`lib.rs` contains only crate documentation, module declarations, and public
re-exports. Weather, USB, radio, and platform runtime modules do not belong in
this workspace unless they are application-only examples with explicit
dependencies away from core.

## Implementation order

1. Define `TrafficObservation`, `TrackKey`, timed fields, snapshots, deltas, and
   typed errors.
2. Add the borrowed `ReceptionEvent` traffic projection and prove non-traffic
   events leave engine state untouched.
3. Ingest identification, altitude, and velocity without position history.
4. Implement and test global airborne CPR pairing.
5. Add UAT position ingestion and ICAO-equality cross-link association.
6. Add identity promotion, kinematic track-file correlation with hysteresis,
   ownship-shadow labeling, and deterministic freshness, coasting, and
   expiration.
7. Add optional GDL 90 traffic and GeoLibre aircraft adapters after core output
   behavior is stable.

Each step leaves a usable public API and regression tests. Avoid building a
general service framework inside the library.

## Required tests

- Known global CPR even/odd pairs, including stale and invalid pairs.
- Interleaved receptions from multiple aircraft and receiver sources.
- Out-of-order and duplicate observations.
- Equal numeric addresses in different namespaces remaining distinct.
- Unknown-to-known identity promotion and rejected false merges.
- Per-field freshness and deterministic track expiration.
- UAT/1090 fusion without callsign-only merging.
- Unsupported or unavailable fields remaining absent.
- FIS-B and other non-traffic receptions leaving Surveillance state untouched.
- Track-file-to-ICAO association and demotion in both directions, including
  rejected single-sample correlations and preserved history across re-keying.
- Ownship-shadow labeling from a caller-supplied identity set.
- Position-absent track lifecycle: creation, field updates, first-position
  upsert, and expiration.
- Versioned delta/snapshot round-trip with bit-exact floats, including
  rejection of unsupported schema versions.
- GDL 90 unavailable-value mappings and buffer-capacity failures in its adapter.
- GeoLibre aircraft upsert, stale, and remove deltas in its adapter.

Tests use captured `ReceptionEvent` fixtures or public protocol vectors. They
must not open radios, USB devices, sockets, or serial ports, and they synchronize
with events rather than sleeps.

## Rust quality rules

- No `println!` or `eprintln!` diagnostics; application adapters use `tracing`.
- No `unwrap`, `expect`, `panic`, or discarded `Result` in library code.
- Monotonic IDs, generations, and epochs use `wrapping_add(1)`.
- Propagate typed errors with context and preserve sources.
- Keep `.rs` files under 500 lines, functions under 80 lines, `lib.rs` under 100
  lines, structs under 30 fields, and enums under 30 variants.
- Keep the engine single-writer. Never hold synchronous locks across `await`.
- Comments explain invariants and reasons, not implementation history.

Before claiming completion, run and read:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
RUSTDOCFLAGS="-D missing_docs -D rustdoc::broken_intra_doc_links" cargo doc --no-deps
cargo build --release
```
