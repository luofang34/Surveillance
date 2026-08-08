//! Portable aircraft tracking over `aero-link` traffic receptions.
//!
//! `surveillance-core` converts normalized traffic observations into coherent
//! aircraft tracks. It owns state that spans receptions — track identity
//! across address namespaces, global CPR reconstruction, per-field freshness
//! and provenance, cross-link fusion, coasting, and expiration — and emits
//! [`TrackDelta`] and [`TrackSnapshot`] values through a versioned schema.
//!
//! The crate is `no_std + alloc` by default and contains no channels, tasks,
//! locks, executors, sockets, timers, or global registries. Applications feed
//! [`SurveillanceEngine`] synchronously and receive output through
//! caller-supplied callbacks. Design boundaries and invariants are defined in
//! `docs/architecture.md`; binding implementation rules live in `AGENTS.md`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod association;
pub mod cpr;
pub mod engine;
pub mod freshness;
pub mod fusion;
pub mod geodesy;
pub mod identity;
pub mod observation;
pub mod schema;
pub mod track;

pub use association::{CorrelationPolicy, agrees, may_correlate, namespace_rank};
pub use cpr::{CprDecodeError, CprFormat, CprFrame, CprFrameError, CprPolicy};
pub use engine::{EngineConfig, IngestError, SurveillanceEngine};
pub use freshness::{FieldProvenance, FieldQuality, FreshnessPolicy, TimedField};
pub use fusion::{FieldUpdate, fuse, link_rank};
pub use identity::IdentitySet;
pub use observation::{
    Callsign, InvalidSquawk, PositionObservation, Squawk, TrafficLink, TrafficObservation,
    VelocityObservation, Wgs84Position,
};
pub use schema::{
    CURRENT_TRACK_SCHEMA_VERSION, TimeCorrespondence, TrackRecord, TrackRecordHeader,
    TrackSchemaError,
};
pub use track::{
    AddressNamespace, RemovalReason, TrackDelta, TrackId, TrackKey, TrackPhase, TrackSnapshot,
    TrackState,
};
