//! GDL 90 traffic reports from Surveillance track snapshots.
//!
//! [`encode_traffic_report`] writes one unframed GDL 90 Traffic Report into
//! caller-owned storage. It maps unavailable values to protocol encodings.
//! It returns an error when GDL 90 has no unavailable encoding for a required
//! value. The crate owns no transport, framing, escaping, CRC, heartbeat,
//! weather data, scheduling, or backpressure.

#![no_std]

mod report;
mod value;

pub use report::{
    GDL90_TRAFFIC_REPORT_LEN, TrafficAlertStatus, TrafficReportEncoding, TrafficReportError,
    encode_traffic_report,
};
