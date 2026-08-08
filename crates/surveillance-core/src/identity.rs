//! Associated over-the-air identities for one track.

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::track::TrackKey;

#[cfg(test)]
mod tests;

/// Every over-the-air identity currently associated with one track: the
/// primary key plus keys correlated from other namespaces or links.
///
/// Association is evidence-driven engine policy — the cross-link association
/// table in `docs/architecture.md` — and both promotion and demotion keep the
/// track's stable [`crate::TrackId`] and history. The set itself is plain
/// data; mutating a copy held outside the engine has no effect on tracking.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IdentitySet {
    primary: TrackKey,
    associated: Vec<TrackKey>,
}

impl IdentitySet {
    /// Create a set containing only the primary identity.
    #[must_use]
    pub fn new(primary: TrackKey) -> Self {
        Self {
            primary,
            associated: Vec::new(),
        }
    }

    /// Current primary identity.
    #[must_use]
    pub fn primary(&self) -> TrackKey {
        self.primary
    }

    /// Identities correlated with the primary, in association order.
    #[must_use]
    pub fn associated(&self) -> &[TrackKey] {
        &self.associated
    }

    /// Whether `key` is the primary or an associated identity.
    #[must_use]
    pub fn contains(&self, key: TrackKey) -> bool {
        self.primary == key || self.associated.contains(&key)
    }

    /// Record an identity correlated with this track; duplicates are ignored.
    pub fn associate(&mut self, key: TrackKey) {
        if !self.contains(key) {
            self.associated.push(key);
        }
    }

    /// Make `key` the primary identity, keeping the displaced primary as an
    /// associated identity so history survives re-keying in both directions.
    pub fn promote(&mut self, key: TrackKey) {
        if self.primary == key {
            return;
        }
        self.associated.retain(|associated| *associated != key);
        let displaced = core::mem::replace(&mut self.primary, key);
        self.associated.push(displaced);
    }
}
