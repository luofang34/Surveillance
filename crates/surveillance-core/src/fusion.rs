//! Cross-link field fusion policy.
//!
//! Fusion decides which observation's value a fused field keeps when the same
//! target is heard through several links, bands, or receivers. Its invariants:
//!
//! - Conflicts resolve through documented quality-then-age precedence, never
//!   last-writer-wins; an older observation arriving late cannot overwrite a
//!   newer applied value.
//! - Direct reception outranks rebroadcast at comparable quality and
//!   freshness (direct ADS-B over ADS-R over TIS-B), but a fresh rebroadcast
//!   beats a stale direct value.
//! - Duplicate observations from multiple receivers refresh provenance
//!   without generating spurious deltas.
//! - Every winning value keeps the [`crate::freshness::FieldProvenance`] of
//!   the observation that supplied it.
//!
//! The decision procedure operates per field on [`crate::TimedField`] values;
//! it never merges identities, which is the province of [`crate::identity`]
//! and the engine's association policy.
