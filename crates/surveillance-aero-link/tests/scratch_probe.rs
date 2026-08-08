//! Scratch probe for encoding layouts; not part of the suite.

#![allow(clippy::expect_used, clippy::panic)]
use aero_link::{Adsb1090Message, ModeSMessage};

fn parity(payload: &[u8]) -> u32 {
    let mut data = payload.to_vec();
    data.extend_from_slice(&[0, 0, 0]);
    let mut crc: u32 = 0;
    for index in 0..data.len() * 8 {
        let bit = (data[index / 8] >> (7 - (index % 8))) & 1;
        crc = ((crc << 1) | u32::from(bit)) & 0x01FF_FFFF;
        if crc & 0x0100_0000 != 0 {
            crc ^= 0x01FF_F409;
        }
    }
    crc & 0x00FF_FFFF
}

#[test]
#[ignore]
fn probe_df4() {
    for ac in [0x19A0_u16, 0x0C30, 0x1830, 0x0230, 0x1000, 0x0410] {
        let mut payload = [0u8; 4];
        payload[0] = 4 << 3;
        payload[2] = (ac >> 8) as u8;
        payload[3] = ac as u8;
        let overlaid = parity(&payload) ^ 0x00AB_CDEF;
        let mut frame = payload.to_vec();
        frame.extend_from_slice(&[
            (overlaid >> 16) as u8,
            (overlaid >> 8) as u8,
            overlaid as u8,
        ]);
        let m = Adsb1090Message::decode(&frame, None).expect("decodes");
        if let ModeSMessage::SurveillanceAltitudeReply(r) = &m.frame.message {
            println!("ac={ac:#06x} altitude={:?}", r.altitude);
        }
    }
}

#[test]
#[ignore]
fn probe_velocity() {
    for (ew, ns) in [(0x0065_u16, 0x0194_u16), (0x0101, 0x0101), (0x0201, 0x0201)] {
        let mut me = [0u8; 7];
        me[0] = (19 << 3) | 1;
        // ME bits: 5 tc, 3 subtype, 1 ic, 1 ifr, 3 nacv, then 1 dew + 10 vew, 1 dns + 10 vns
        me[1] = 0x00;
        me[2] = ((ew >> 8) & 0x03) as u8;
        me[3] = (ew & 0xFF) as u8;
        me[4] = ((ns >> 8) & 0x03) as u8;
        me[5] = (ns & 0xFF) as u8;
        let mut payload = [0u8; 11];
        payload[0] = 18 << 3;
        payload[1] = 0x00;
        payload[2] = 0xAB;
        payload[3] = 0xCD;
        payload[4..11].copy_from_slice(&me);
        let checksum = parity(&payload);
        let mut frame = payload.to_vec();
        frame.extend_from_slice(&[
            (checksum >> 16) as u8,
            (checksum >> 8) as u8,
            checksum as u8,
        ]);
        let m = Adsb1090Message::decode(&frame, None).expect("decodes");
        println!("ew={ew:#06x} ns={ns:#06x} -> {:?}", m.frame.message);
    }
}
