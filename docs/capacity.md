# Capacity and association cost

## Persistent bounds

The default `EngineConfig` holds at most 1,024 live tracks. The constant
`DEFAULT_MAX_TRACKS` gives this value. Set `max_tracks` to `None` only when the
application has another memory limit.

The default key index holds at most 4,096 retained identities. The constant
`DEFAULT_MAX_TRACK_KEYS` gives this value. The engine checks the complete set
of keys for an observation before it changes state. One unclassified ADS-B
observation can require two keys because emitter classification adds a
classified key. A rejected observation does not create a partial track.

Each track retains at most 64 correlation evidence records by default. The
`max_candidate_peers` setting controls this value. Thus, the default engine
retains at most 65,536 evidence records. A peer removal or merge deletes all
evidence that names that peer.

An identity activity record exists only for a retained key. Thus,
`max_track_keys` also bounds the total identity activity. A merge moves
identity data into the surviving track. Expiration releases the keys and their
activity records.

The engine uses `BTreeMap` nodes for tracks and keys. Each track also owns
small `Vec` allocations for identities, activity, and correlation evidence.
The engine does not use a fixed arena. An application for a target without a
general allocator must supply a suitable `alloc` implementation.

The engine copies the caller-supplied ownship identity list. The track and key
bounds do not limit this list. The application must set its own limit before
it calls `set_ownship_identities`.

## Processing bound

An accepted position scans the live track table once. The cost is `O(T)`,
where `T` is the live track count. The scan makes at most 1,023 peer checks at
the default bound. Identity-only observations do not start this scan.

The correlation evidence update retains no more than
`max_candidate_peers` records. It removes expired evidence before it applies
new evidence. This limit bounds persistent correlation memory. The temporary
comparison list can contain `T - 1` results.

## Reference workload

The `association_capacity` example creates 1,024 tracks. It then applies 100
positions to one track. Each position checks 1,023 eligible peers. The peers
are far apart, so the engine exercises rejection and bounded evidence churn.

Build and run the workload with these commands:

```sh
cargo build -p surveillance-core --release --example association_capacity
/usr/bin/time -l target/release/examples/association_capacity
```

The reference system is an Apple M3 Max with 48 GB of memory. It runs the
`aarch64-apple-darwin` Rust 1.95.0 toolchain. Three release runs each took
from 0.03 to 0.04 seconds. Peak resident memory was from 4,816,896 to
4,866,048 bytes. This value is process memory. It is not only engine heap
memory.

The unit test `association_at_the_default_track_bound_keeps_declared_limits`
uses the same track bound. It checks the peer count and the evidence bound in
CI. Run the reference workload again after a change to association data or
processing.
