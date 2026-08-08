//! Mapping an `aero-link` receiver identity onto a canonical source
//! reference.

use surveillance_core::{DeliveryPath, SourceRef};

/// Carry the runtime-local receiver identity through as a local-receiver
/// delivery.
///
/// A radio is one delivery path among several the engine accepts, so the
/// identity survives but the receiver type does not. The incarnation is zero
/// because `aero-link` does not yet distinguish one receiver session from the
/// next; a host that tracks reconnects supplies its own epoch instead.
pub(crate) fn source_ref(value: aero_link::ReceiverSource) -> SourceRef {
    SourceRef::new(value.id, 0, DeliveryPath::LocalReceiver)
}
