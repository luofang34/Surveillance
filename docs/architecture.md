# Architecture and service boundaries

## Project identity

`Surveillance` is the stateful traffic-tracking layer of a larger aviation
stack. It converts structured traffic observations into coherent aircraft
tracks and nothing else: no radio acquisition, no weather lifecycle, no
gateway transport, no rendering. It is named for the surveillance function
rather than any input link or output consumer; 1090 ES, UAT, ADS-R, and TIS-B
are supported observation sources, not the project boundary.

Surveillance must outlive every receiver, MCU, operating system, USB stack,
async runtime, EFB, map renderer, and non-traffic aviation product used with
it. Everything platform-bound is an adapter around the portable core.

## Layer ownership

```text
aero-link reception (ReceptionEvent + provenance)
        |
        |  ReceptionEvent::traffic() borrowed projection
        v
observation normalization  (surveillance-core, stateless)
        |
        v
SurveillanceEngine         (surveillance-core, single-writer state)
  - track identity across address namespaces
  - global CPR reconstruction
  - per-field freshness, fusion, provenance
  - coasting, expiration, ownship-shadow labeling
        |
        +----------------------+---------------------+
        v                      v                     v
  TrackDelta stream      TrackSnapshot query    versioned records
        |                                            |
        +-----------+----------------+               v
        v           v                v          recorders / replay /
  GDL 90 adapter  GeoLibre adapter  app logic   web viewers
```

`aero-link` owns everything that completes within one reception: framing,
correction, demodulation, protocol decoding, monotonic timestamps, and
receiver provenance. Surveillance owns everything whose meaning spans traffic
receptions:

- address namespaces and track identity
- global CPR even/odd pairing and position reconstruction
- per-field freshness, ordering, confidence, and expiration
- conservative fusion and deduplication of 1090 ES, UAT, ADS-R, and TIS-B
- identity evolution from unknown to known targets without identity churn
- track creation, update deltas, coasting, and removal

Weather-product reassembly, GDL 90 gateway composition, event fan-out,
recording storage, and map rendering belong to sibling services. The
application router feeds one `ReceptionEvent` to every consumer that needs
it; Surveillance sees only the borrowed traffic projection.

## Crate map

```text
aero-link (default-features = false)
        ^
        |
surveillance-core ---- no_std + alloc, the only load-bearing crate
        ^
        |------ surveillance (umbrella re-export)
        |------ surveillance-gdl90     (planned: TrackSnapshot -> GDL 90 payloads)
        |------ surveillance-geolibre  (planned: TrackDelta -> aircraft features)
        |------ host testing apps      (planned: replay CLI, MapLibre viewer;
                                        std/host feature-gated, never default)
```

- `surveillance-core` depends only on `aero-link` (portable API), `serde`,
  and `thiserror` in portable configurations. It contains no channels, tasks,
  locks, executors, sockets, timers, or global registries.
- `surveillance` is the umbrella package applications name in `Cargo.toml`.
  Its default features re-export exactly the portable core. Host-side testing
  binaries hang off opt-in features so that library consumers never pay for
  them.
- Adapter crates depend on core output types only. A renderer or transport
  never imports CPR, fusion, identity, or track internals.
- Application crates may enable `aero-link/std` or platform adapters; the
  core must build with `aero-link` default features only.

## Input contract

- `ReceptionEvent.received_at_micros` is runtime-monotonic microseconds and
  is the only clock. It orders observations, ages fields, and drives
  expiration. It is not UTC. The engine never reads a wall clock and never
  starts a timer; time advances only through `advance_time`.
- UAT firmware timestamps and Mode S receiver sample indexes are per-radio
  metadata. They are never compared across radios and never used as a
  cross-radio clock.
- `ReceiverSource` provenance survives normalization and fusion; every
  exported field can name the input that supplied it.
- Observations may be duplicated, delayed, or out of order, and several
  receivers may observe the same band. Arrival order is a hint, never a
  correctness assumption: a newer-timestamped value already applied is not
  overwritten by an older observation arriving late.
- Absent input stays absent. Normalization never fabricates accuracy,
  integrity, altitude, velocity, or identity values, and exported types
  represent absence explicitly.

## Observation normalization

`surveillance-core` defines a stateless, synchronous adapter from the
borrowed `ReceptionEvent::traffic()` projection into its own owned
`TrafficObservation`:

```rust,ignore
if let Some(projection) = event.traffic() {
    let observation = TrafficObservation::normalize(&projection)?;
    engine.ingest(observation, |delta| consume(delta))?;
}
```

- One `TrafficObservation` is one normalized traffic measurement: track key,
  link classification, monotonic time, receiver source, and the optional
  measured fields (position or undecoded CPR, altitudes, velocity, callsign,
  squawk, quality indicators).
- Classification uses `aero-link`'s typed fields — the UAT address qualifier
  and the classified extended-squitter address, which already encodes both
  the namespace and the service that produced the frame — never raw message
  bytes.
- Non-traffic receptions (FIS-B, ground uplinks) never reach the engine and
  must leave its state untouched — the projection returns `None` and the
  caller skips ingestion.
- `TrafficLink` records the originating link class (direct 1090 ES, Mode S
  reply, direct UAT, ADS-R, TIS-B) with its band. It is the fusion-precedence
  axis and part of every field's provenance. On 978 MHz the qualifier does
  not separate an ADS-R rebroadcast from a TIS-B report, so both normalize to
  a TIS-B link: that never claims a report came straight from the target when
  it did not.
- A reception that names no target, carries service-management data, or uses
  a reserved qualifier produces a typed error rather than a fabricated
  identity. A Mode S reply whose address was recovered from parity but never
  independently confirmed is refused for the same reason: accepting it would
  create a track for an address no receiver actually saw announced.
- Two identity spaces need care. A TIS-B Mode A track file carries a Mode A
  code and a track-file number rather than a 24-bit address; both are twelve
  bits, so they pack into one key value under their own namespace, kept
  distinct from service-generated 24-bit track files that could collide with
  them numerically. And one protocol value covers every non-ICAO ADS-B
  address — anonymous, self-assigned, vehicle, and fixed obstacle — which
  only an emitter category separates, so those normalize to an
  `AdsbNonIcao` namespace that correlation refuses to act on until a category
  arrives.

## Track identity model

Three distinct concepts, deliberately separated:

- `TrackId` — a stable, engine-assigned identity for one physical target.
  It never changes across re-keying, promotion, or demotion. Consumers use
  it to correlate deltas over a track's whole life. IDs increment with
  `wrapping_add` and are never reused while a track is alive.
- `TrackKey` — the current primary over-the-air identity: a numeric address
  plus its `AddressNamespace`. Keys can change during a track's life; the
  `TrackId` cannot.
- `IdentitySet` — every over-the-air identity currently associated with the
  track: the primary key plus correlated keys from other namespaces or
  links.

`AddressNamespace` separates identity spaces that may reuse the same numeric
address: ICAO, self-assigned/anonymous, TIS-B track file, surface vehicle,
fixed beacon. Equal numeric addresses in different namespaces are always
distinct tracks. ICAO-qualified identities form one namespace across links:
direct ADS-B, ADS-R, and TIS-B with an ICAO address associate on address
equality, and per-field quality and age rules decide which link's values win.

## Cross-link association policy

A physical aircraft can appear through several links and namespaces during
one flight. Association is explicit policy, not emergent behavior:

| Identity pair | Rule |
| --- | --- |
| ICAO ↔ ICAO across links (direct ADS-B, ADS-R, TIS-B with ICAO, 1090 and UAT) | Associate on address equality; the qualifier exists to assert this. |
| Track file ↔ ICAO | Kinematic correlation with hysteresis; never on address, proximity, or callsign alone. |
| Self-assigned or anonymous ↔ anything | Kinematic correlation with hysteresis. |
| Vehicle ↔ non-vehicle | Never. |

Kinematic correlation requires sustained agreement — positions consistent
with both tracks' integrity bounds plus velocity and altitude agreement —
over several consecutive observations inside a bounded window. Hysteresis
ensures a single coincidental alignment cannot merge tracks and a single
outlier cannot split them: association needs a run of consecutive
agreements, dissociation needs a run of consecutive disagreements, and each
resets the other's count. Callsign equality is never sufficient evidence for
any merge.

A pair that cannot be judged — one side has no fresh position — accumulates
neither agreement nor disagreement, so an unobservable interval is never
read as a mismatch. Correlation is evaluated only when a position is
applied, which keeps a stream of identity-only receptions free of
correlation cost.

When correlation joins two tracks, the track a consumer has watched longest
survives with its `TrackId`, absorbs the other's identities and fused fields
through the ordinary precedence rules, and re-keys to the most authoritative
identity available — a globally unique cooperative address outranks a
ground-assigned track file or an anonymized one. The absorbed track is
removed with `RemovalReason::Merged`, which names the survivor so a consumer
transfers what it was displaying rather than deleting it: the aircraft did
not disappear, its duplicate did. Equal creation times fall back to identity
order, so a merge never depends on which side reported first.

Identity transitions preserve history in both directions:

- Promotion: a radar-derived track file correlates with a newly acquired
  cooperative ICAO identity. Consumers see the same `TrackId` re-keyed, not
  a removal plus a creation. `IdentitySet` records both identities.
- Demotion: the cooperative source expires while a correlated
  non-cooperative source continues. The track demotes to the surviving
  identity, coasts under normal freshness rules, and keeps its history.

Both directions are evidence-driven and reversible without identity churn.

## Position pipeline

- Global airborne CPR lives in the engine because it requires even/odd
  history across receptions. A decode is accepted only after validating pair
  age, parity, latitude zone consistency, and decoded range plausibility.
  Each gate has its own typed rejection, so a refused position says why.
- Range plausibility bounds travel by elapsed time against a speed limit plus
  a fixed allowance, so a long gap widens the bound rather than forcing a
  false rejection, and a newly acquired target is not rejected for
  uncertainty alone.
- Local CPR is a separate path that requires a trustworthy reference
  position. It is not a fallback for a pair that decoded into inconsistent
  zones or failed plausibility: a disagreeing pair is evidence something is
  wrong, and resolving it against a reference would launder that away. It
  applies only where no pair exists to decode from — a track whose position
  is already established and fresh resolves a single frame against its own
  last position.
- Surface CPR is reference-dependent in every case, because its zones repeat
  once per quadrant. The reference is the track's own fresh position, else a
  caller-supplied reference position the engine holds as plain data.
- UAT positions arrive already decoded; they enter fusion directly with
  their quality indicators.
- A track may exist without any position: a Mode S target can supply only
  altitude, squawk, or identity. State, snapshots, and deltas represent
  position-absent tracks explicitly; position history begins when the first
  valid position arrives.

## Field fusion and freshness

Every fused field is a `TimedField<T>`: value, observation time, quality,
and provenance (link, namespace, receiver source). Rules:

- Each field ages independently. A callsign update cannot refresh an old
  position; freshness windows are per-field policy in `FreshnessPolicy`.
- Conflicts resolve through documented quality-then-age precedence, never
  last-writer-wins. An older observation arriving later cannot overwrite a
  newer applied value.
- Link precedence prefers direct reception over rebroadcast when both are
  fresh and of comparable quality (direct ADS-B over Mode S reply over ADS-R
  over TIS-B), but a fresh rebroadcast beats a stale direct value.
- The decision procedure evaluates, in order: reject a strictly older
  observation; accept anything over a value past its freshness window;
  between fresh values prefer the higher link rank; at equal rank prefer the
  newer observation; and separate two observations of the same instant by
  quality, where better means no indicator is worse and at least one is
  better.
- Duplicate observations (same content from multiple receivers) update
  provenance and freshness without generating spurious deltas.
- Units are explicit in field names and newtypes: WGS84 degrees, feet,
  knots, feet per minute, microseconds. No ambiguous `speed`, `heading`,
  `altitude`, or `time` fields exist anywhere in the public API.

## Track lifecycle

```text
            first observation for an unknown key
                          |
                          v
        +------------- Active <---------------+
        |                 |                   |
        |     per-field freshness lapses      | fresh observation
        |                 v                   |
        |             Coasting ---------------+
        |                 |
        |                 |  retention window lapses in advance_time
        v                 v
      Removed  <------ Removed
   (re-key is not removal: the TrackId survives key changes)
```

- Creation emits `TrackDelta::Created` with the initial snapshot, even when
  position-absent.
- Updates emit deltas only for meaningful change; duplicate-driven
  provenance refreshes stay silent.
- Coasting is entered deterministically inside `advance_time` when defining
  fields lapse; the track remains visible and labeled as coasting.
- Removal is deterministic and driven only by `advance_time`; the engine
  never expires state as a side effect of ingestion order. Removal releases
  every identity the track occupied, so a later observation for any of them
  starts a new track rather than resurrecting a dangling index entry.
- A merge also removes a track, but for the opposite reason: `RemovalReason`
  distinguishes a target that aged out from a duplicate that was absorbed,
  and only the former means the aircraft is gone.
- Demotion is evaluated in `advance_time` too: when the primary identity has
  been silent past its window while another associated identity keeps
  reporting, the track re-keys to the identity still being fed.
- All lifecycle transitions are pure functions of observation content and
  the caller-supplied monotonic clock, which makes replay bit-reproducible.

## Ownship shadows

Ground infrastructure rebroadcasts ownship: an aircraft's own transmission
returns as ADS-R or TIS-B and would otherwise display as a phantom intruder.
The engine accepts a caller-supplied set of ownship identities as plain data,
labels matching tracks as ownship shadows in state, snapshots, and deltas,
and suppresses nothing itself. Ownship selection, navigation, and display
policy stay in the application.

## Exported schema, provenance, and time

`TrackDelta` and `TrackSnapshot` cross process and machine boundaries —
native apps, web viewers, recorders — so their serialization is part of the
core contract, not an optional feature:

- A versioned envelope (`TrackRecord`, `CURRENT_TRACK_SCHEMA_VERSION`) wraps
  every exported delta. Its fields are private; accessors validate the
  version and return a typed error for unsupported versions before any
  payload interpretation.
- Golden round-trip fixtures with bit-exact float values pin the schema.
  JSON consumers require a correctly rounded parser (`serde_json` with
  `float_roundtrip`); other formats must preserve IEEE-754 values exactly to
  claim compatibility.
- Growth policy matches `aero-link`: growth-prone public enums are
  `#[non_exhaustive]`; adding a variant is a compatible Rust extension but
  requires a schema version bump when an older reader could encounter it;
  removing or renaming fields or variants, changing units or meaning, or
  changing Serde tagging always requires a version bump.
- Every exported field keeps provenance a detail view can display:
  originating link and band, address namespace, `ReceiverSource`, and the
  observation's monotonic time. A snapshot may additionally reference the
  receptions that supplied its current position, altitude, velocity, and
  identity, so an application can show the underlying frames from its own
  recording without Surveillance storing them.
- All exported times are runtime-monotonic microseconds, consistent with
  `ReceptionEvent.received_at_micros`. A service exporting deltas off the
  runtime attaches one monotonic-origin-to-UTC correspondence
  (`TimeCorrespondence`) per stream at that boundary. The engine itself
  never touches UTC.

## Output adapter boundaries

### GDL 90

The core exports tracks, not a GDL 90 service. `surveillance-gdl90` maps
`TrackSnapshot` values to GDL 90 traffic-report payloads in caller-owned
storage, using specification-defined unavailable encodings rather than
fabricated values. It owns no UDP, Bluetooth, serial, sockets, heartbeat
scheduling, FIS-B uplinks, or weather state. A complete GDL 90 gateway is a
composition-layer service that combines Surveillance traffic output with
weather, ownship navigation, and time/status inputs, and owns CRC, framing,
escaping, scheduling, transport, and backpressure.

### GeoLibre

`surveillance-geolibre` converts `TrackDelta` values into aircraft feature
upserts, stale/coasting changes, and removals in WGS84 coordinates. Icons,
colors, labels, clustering, camera state, and rendering policy belong to the
application. Position-absent tracks produce no geographic feature; they stay
visible through snapshots and are upserted when a first valid position
arrives. Weather features come from a separate weather adapter; displaying
both layers together is application composition, not a Surveillance concern.

## Host testing applications

Feature-gated, application-layer tools make the library directly exercisable
without ever burdening library consumers:

- a replay CLI that feeds recorded `ReceptionEvent` streams through the
  engine and prints or records delta streams
- a MapLibre-based viewer (macOS and web) driven by the GeoLibre adapter for
  visual inspection of live or replayed traffic

Both live behind opt-in features or separate application crates, follow the
same pattern as `aero-link`'s `host` feature, and may use Tokio, tracing,
and sockets freely — none of which ever appear in core or adapter crates.
When built as a plain library or a GeoLibre layer source, none of this code
is compiled in.

## Portability and dependencies

- `surveillance-core` defaults to `no_std + alloc`; `std` is a forwarding
  feature only.
- Core and adapters build for `wasm32-unknown-unknown` and the embedded
  matrix (`thumbv6m`, `thumbv8m.main`, `riscv32imac`); the local check
  scripts keep every target honest. Hosted CI mirroring aero-link#19 is
  tracked in repository issues.
- `serde` is unconditional on exported schema types and their components;
  purely internal types keep it off.
- Library error enums use `thiserror` with sources and context; `anyhow` is
  limited to host CLI entry points.
- No Tokio, async runtimes, sockets, USB crates, or board HALs in core or
  adapters. `embedded-hal` appears only in an application crate driving a
  real peripheral.

## Testing strategy

- Fixtures are captured `ReceptionEvent` values (aero-link's versioned
  reception records) or public protocol vectors; tests never open radios,
  USB devices, sockets, or serial ports, and they synchronize with events
  rather than sleeps.
- Unit tests live in `src/*/tests.rs` with access to internals; `tests/*`
  exercises the public API only.
- Determinism is load-bearing: identical observation sequences plus
  identical `advance_time` calls must produce identical delta streams, which
  makes golden end-to-end fixtures possible.
- The required behavioral coverage — CPR validation, namespace separation,
  false-merge rejection, promotion/demotion reversibility, per-field
  freshness, duplicate suppression, position-absent lifecycle, schema
  round-trips, adapter mappings — is enumerated in `AGENTS.md` and mapped
  issue-by-issue in the repository issue plan.
