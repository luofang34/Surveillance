#![allow(clippy::expect_used, clippy::panic)]

use crate::cpr::{CprFormat, CprFrame, CprFrameError};

#[test]
fn accepts_seventeen_bit_coordinates() {
    let frame = CprFrame::new(CprFormat::Even, CprFrame::MAX_ENCODED, 0)
        .expect("boundary values are valid");
    assert_eq!(frame.format, CprFormat::Even);
    assert_eq!(frame.encoded_latitude, CprFrame::MAX_ENCODED);
    assert_eq!(frame.encoded_longitude, 0);
}

#[test]
fn rejects_out_of_range_coordinates() {
    let over = CprFrame::MAX_ENCODED + 1;
    assert_eq!(
        CprFrame::new(CprFormat::Odd, over, 0),
        Err(CprFrameError::EncodedLatitudeRange { value: over })
    );
    assert_eq!(
        CprFrame::new(CprFormat::Odd, 0, over),
        Err(CprFrameError::EncodedLongitudeRange { value: over })
    );
}
