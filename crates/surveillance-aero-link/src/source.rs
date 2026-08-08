//! Mapping an `aero-link` receiver identity onto a canonical source reference.

use surveillance_core::{SourceRef, SourceTransport};

/// Carry the runtime-local receiver identity through unchanged.
///
/// A radio is one kind of delivery path among several the engine accepts, so
/// the identity survives but the receiver type does not.
pub(crate) fn source_ref(value: aero_link::ReceiverSource) -> SourceRef {
    SourceRef::new(value.id, transport(value.transport))
}

fn transport(value: aero_link::SourceTransport) -> SourceTransport {
    match value {
        aero_link::SourceTransport::Unspecified => SourceTransport::Unspecified,
        aero_link::SourceTransport::Usb => SourceTransport::Usb,
        aero_link::SourceTransport::Uart => SourceTransport::Uart,
        aero_link::SourceTransport::Spi => SourceTransport::Spi,
        aero_link::SourceTransport::Parallel => SourceTransport::Parallel,
        aero_link::SourceTransport::Network => SourceTransport::Network,
        aero_link::SourceTransport::Replay => SourceTransport::Replay,
        _ => SourceTransport::Unspecified,
    }
}
