# Map features and the preview boundary

## The question this answers

Surveillance emits `TrackDelta` values. A map needs features. Something has
to convert one into the other, and the choice of where that conversion lives
decides whether a library consumer pays for a renderer they never asked for.

Two things get conflated under "the GeoLibre adapter", and separating them is
the whole design:

- **The semantic mapping** — a track delta becomes an aircraft feature
  upsert, a stale marker, or a removal, in WGS84, with the properties a map
  might style by. This is pure data transformation. It needs no renderer, no
  platform, and no graphics context.
- **The renderer binding** — handing those features to something that draws:
  MapLibre Native on iPad, MapLibre GL JS in a browser, or a future
  first-party renderer.

The first belongs in Rust, in this workspace, with no renderer dependency at
all. The second belongs in an application target outside the Cargo workspace.

## Where the API lives

There is no first-party GeoLibre code today — no crate, no repository, no
local directory. `AGENTS.md` names a `surveillance-geolibre` adapter as a
destination, not as an existing dependency.

A third-party `opengeos/GeoLibre` does exist, with a Rust component
(`geolibre-rust`), but it is not a renderer: it is a geospatial *analysis*
toolkit built on whitebox-wasm, compiled to WebAssembly and consumed from
JavaScript or Python for raster, projection, vector, and LiDAR processing. It
cannot draw a traffic layer on an iPad, so it is not a candidate target for
this adapter whether or not it was the name's inspiration.

Either way the conclusion is the same, and it matters: naming a crate after a
renderer that does not exist would block the adapter on building the renderer
first, or bake a speculative API into a boundary that is supposed to outlive
every renderer.

So the adapter depends on nothing. It owns a small, renderer-neutral feature
vocabulary:

```rust,ignore
pub enum FeatureDelta {
    /// Place or move an aircraft feature.
    Upsert(AircraftFeature),
    /// The track is coasting; the feature stays but should read as aging.
    Stale { id: TrackId },
    /// Remove the feature. `transfer_to` is set when the track was merged
    /// into another, so the map moves what it was showing instead of
    /// deleting it.
    Remove { id: TrackId, transfer_to: Option<TrackId> },
}
```

`transfer_to` is why the merge path carries `RemovalReason::Merged { into }`:
without it a correlated duplicate disappearing from the map is
indistinguishable from an aircraft that flew out of range.

The interchange format on the wire is GeoJSON (RFC 7946), because MapLibre
Native, MapLibre GL JS, and any plausible future renderer all consume it.
But GeoJSON is a *serialization*, not the API: the adapter's Rust surface is
typed structs, and GeoJSON encoding sits behind an opt-in feature. A Rust
consumer moves structs; a Swift or JavaScript consumer takes strings. A
hot-path map at a few hundred targets should not be allocating a JSON
document per update unless it actually needs one.

If a first-party renderer with its own feature types ever exists, it gets a
separate thin shim that maps this vocabulary onto those types. Nothing in
core or in this adapter changes.

### Naming

The crate is named for what it produces, not for a consumer it might have:
`surveillance-geojson`. This is a deliberate departure from the
`surveillance-geolibre` name in `AGENTS.md`, on the grounds that the only
GeoLibre that exists is an analysis toolkit rather than a renderer, and that
a boundary meant to outlive renderers should not carry one's name. A
`surveillance-geolibre` shim can still sit beside it later — mapping this
vocabulary onto that project's types if it ever grows rendering — without
either crate being renamed or split.

## Paying nothing when used as a library

The constraint is that a consumer using Surveillance purely as a tracking
library compiles none of this. Three mechanisms, in order of how much they
matter:

1. **The adapter is its own crate.** A consumer naming `surveillance` gets
   the core engine and nothing else. Adding the feature adapter is an
   explicit dependency, not a default.
2. **GeoJSON encoding is a feature of that crate.** Even a consumer who wants
   typed feature deltas does not pull `serde_json` unless they want strings.
3. **The preview application is not in the Cargo workspace's default
   members.** It builds only when asked for by name, and its Swift half is
   not a Cargo target at all.

The adapter itself stays `no_std + alloc` and builds for the same embedded
and wasm matrix as core, so it never becomes the reason a target drops off
the list.

## The preview

### What the platforms actually support

MapLibre Native ships an iOS SDK as a binary xcframework through Swift
Package Manager
([maplibre-gl-native-distribution](https://github.com/maplibre/maplibre-gl-native-distribution)).
`MLNShapeSource` takes GeoJSON directly and supports point clustering, which
is exactly the seam a traffic layer needs: one source, replaced or updated
per batch.

macOS is the awkward part. MapLibre Native's core renderer builds for macOS,
but its AppKit SDK is not in a state the project itself recommends — the
MapLibre documentation points macOS users at Qt or at reusing the iOS
platform through Mac Catalyst. So the honest plan for "iPad and Mac" is one
iOS target reaching the Mac through Mac Catalyst, or simply running the iPad
build on Apple Silicon. Planning a native AppKit map view would be building
on the least maintained surface MapLibre has.

### Where it lives

The workspace already has a pattern for exactly this kind of tool:
`avionics-link/app/LinkProbe` is an XcodeGen project (`project.yml`) building
a pure-SwiftUI iOS application that hosts rendered content from Swift
packages pinned by path or git revision. The traffic preview should mirror
it rather than invent a second convention — same XcodeGen layout, same
Swift 6 and SwiftUI-only style, `TARGETED_DEVICE_FAMILY: "1,2"`, MapLibre
added as a Swift package dependency alongside the local ones.

One deliberate difference: LinkProbe sets `SUPPORTS_MACCATALYST: false`,
while the preview wants `true`, because Catalyst is how the same target
reaches the Mac.

It lives in this repository, not in `avionics-link`, because it previews
Surveillance output and should move with the feature vocabulary it renders.
Keeping it out of the Cargo workspace's default members — its Swift half is
not a Cargo target at all — is what keeps it free for library consumers.

### Two stages, thinnest first

**Stage one needs no FFI at all.** The replay CLI already planned in issue #11
writes a delta stream; have it write GeoJSON. The preview app reads that file
and pushes each batch into an `MLNShapeSource`. That is a few dozen lines of
Swift, no Rust linked into the app, no bridging header, no xcframework build
step — and it is enough to actually watch recorded traffic move on a map,
which is what a preview is for.

**Stage two adds live tracking** by linking the engine: a small `staticlib`
exposing a handful of C functions (create engine, ingest a reception, drain
deltas as GeoJSON, advance time, destroy), packaged as an xcframework. This
is the only part that costs real build machinery, and it is worth deferring
until stage one has proven the feature vocabulary is right.

Both stages consume the same GeoJSON, so stage two changes how bytes arrive,
not what they mean.

### Boundaries the preview does not cross

The preview is a testing tool, not a product. Icons, colours, labels,
clustering thresholds, camera behaviour, and traffic-display policy stay in
the application. Ownship-shadow filtering is a display decision made there,
never in the adapter — the engine labels and the adapter passes the label
through, and only the application decides whether to draw it.
