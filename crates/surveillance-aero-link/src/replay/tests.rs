//! Which refusals end a stream, and which end only one reception.

use crate::NormalizeError;
use crate::replay::refusal_ends_the_stream;

/// A band this build cannot normalize means the recording is newer than the
/// build. Every reception of that band would be dropped, so counting them
/// would report a successful replay of a source that never arrived.
#[test]
fn an_unsupported_band_refusal_ends_the_stream() {
    assert!(refusal_ends_the_stream(&NormalizeError::UnsupportedPayload));
}

/// Every other refusal describes one target. The next reception does not
/// inherit it, so the stream survives.
#[test]
fn a_refusal_that_names_one_target_does_not_end_the_stream() {
    for error in [
        NormalizeError::NoTargetIdentity { downlink_format: 4 },
        NormalizeError::UnconfirmedAddress { downlink_format: 5 },
        NormalizeError::ServiceManagement,
        NormalizeError::ReservedAddressQualifier { qualifier: 6 },
    ] {
        assert!(
            !refusal_ends_the_stream(&error),
            "{error:?} refuses one reception, not the recording"
        );
    }
}
