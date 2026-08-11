//! Source-neutral aircraft tracking over canonical traffic observations.
//!
//! `surveillance-core` converts canonical traffic observations into coherent
//! aircraft tracks, whatever produced them: a local radio, an installed
//! panel, a network provider, a replay, or a simulator. Translating one radio
//! reception into canonical terms belongs to a link adapter, so no decoder
//! vocabulary reaches a consumer of the engine.
//!
//! It owns state that spans receptions — track identity across address
//! namespaces, global CPR reconstruction, per-field freshness
//! and provenance, cross-link fusion, coasting, and expiration — and emits
//! [`TrackDelta`] and [`TrackSnapshotHandle`] values through a versioned schema.
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
pub mod source;
pub mod track;

pub use association::{CorrelationPolicy, agrees, may_correlate, namespace_rank};
pub use cpr::{CprDecodeError, CprFormat, CprFrame, CprFrameError, CprPolicy};
pub use engine::{
    EngineConfig, FieldDisposition, FieldOutcomes, IngestError, IngestOutcome, PositionDisposition,
    PositionRejection, SnapshotPublicationError, SurveillanceEngine,
};
pub use freshness::{FieldProvenance, FieldQuality, FreshnessPolicy, TimedField};
pub use fusion::{FieldUpdate, fuse, origin_rank};
pub use identity::IdentitySet;
pub use observation::{
    AirGroundState, AirspeedKind, Callsign, CallsignKind, EmergencyState, HeadingReference,
    InvalidSquawk, ObservationOrigin, ObservationTime, PositionObservation, Squawk, TimeBasis,
    TrafficObservation, VelocityObservation, VerticalRateSource, Wgs84Position,
};
pub use schema::{
    CURRENT_TRACK_SCHEMA_VERSION, TimeCorrespondence, TrackRecord, TrackRecordHeader,
    TrackSchemaError,
};
pub use source::{Band, DeliveryPath, SourceRef};
pub use track::{
    AddressNamespace, ProducerInstanceId, RemovalReason, SnapshotRevision, TrackDelta, TrackId,
    TrackKey, TrackPhase, TrackSnapshot, TrackSnapshotHandle, TrackState,
};
