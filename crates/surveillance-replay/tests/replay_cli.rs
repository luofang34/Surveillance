//! Complete command behavior for a recorded reception fixture.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

use surveillance_core::TrackRecord;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn replay(input: &str, extra: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_surveillance-replay"));
    command.arg("replay").arg(fixture(input));
    command.args(extra);
    command.output().expect("the replay command starts")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn producer_instance_id(output: &Output) -> u64 {
    let first = output
        .stdout
        .split(|byte| *byte == b'\n')
        .next()
        .expect("the stream contains one record");
    let record: TrackRecord = serde_json::from_slice(first).expect("the first record decodes");
    record
        .delta()
        .expect("the record has the current version")
        .producer_instance_id()
        .get()
}

#[test]
fn a_fixed_identity_reproduces_the_golden_track_stream() {
    let arguments = ["--producer-instance-id", "71"];
    let first = replay("receptions.ndjson", &arguments);
    let second = replay("receptions.ndjson", &arguments);
    assert_success(&first);
    assert_success(&second);

    assert_eq!(
        first.stdout,
        include_bytes!("fixtures/track_records.ndjson")
    );
    assert_eq!(first.stdout, second.stdout);
    let diagnostics = String::from_utf8_lossy(&first.stderr);
    for field in [
        "receptions_consumed=5",
        "observations_produced=4",
        "receptions_skipped_not_traffic=0",
        "receptions_refused=1",
        "reception refused line=5",
        "address qualifier 6 is reserved",
    ] {
        assert!(
            diagnostics.contains(field),
            "missing {field}: {diagnostics}"
        );
    }
}

#[test]
fn a_live_radio_capture_reproduces_the_golden_track_stream() {
    let arguments = ["--producer-instance-id", "72"];
    let first = replay("live_receptions.ndjson", &arguments);
    let second = replay("live_receptions.ndjson", &arguments);
    assert_success(&first);
    assert_success(&second);

    assert_eq!(
        first.stdout,
        include_bytes!("fixtures/live_track_records.ndjson")
    );
    assert_eq!(first.stdout, second.stdout);
    let diagnostics = String::from_utf8_lossy(&first.stderr);
    for field in [
        "receptions_consumed=5",
        "observations_produced=4",
        "receptions_skipped_not_traffic=1",
        "receptions_refused=0",
        "fields_rejected=0",
    ] {
        assert!(
            diagnostics.contains(field),
            "missing {field}: {diagnostics}"
        );
    }
}

#[test]
fn each_command_construction_gets_a_new_producer_identity() {
    let first = replay("receptions.ndjson", &[]);
    let second = replay("receptions.ndjson", &[]);
    assert_success(&first);
    assert_success(&second);

    assert_ne!(producer_instance_id(&first), producer_instance_id(&second));
}

#[test]
fn a_malformed_record_fails_with_line_and_cause_context() {
    let output = replay("malformed.ndjson", &["--producer-instance-id", "71"]);

    assert!(!output.status.success());
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostics.contains("line 1"), "{diagnostics}");
    assert!(
        diagnostics.contains("not a supported reception record"),
        "{diagnostics}"
    );
}

#[cfg(feature = "geojson")]
#[test]
fn geojson_output_is_a_golden_rfc_7946_feature_stream() {
    let output = replay(
        "receptions.ndjson",
        &["--producer-instance-id", "71", "--format", "geo-json"],
    );
    assert_success(&output);
    assert_eq!(output.stdout, include_bytes!("fixtures/features.ndjson"));

    let features: Vec<serde_json::Value> = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("each line is JSON"))
        .collect();
    assert_eq!(features.len(), 3);
    assert!(features.iter().all(|feature| feature["type"] == "Feature"));
    assert_eq!(features[0]["geometry"]["type"], "Point");
    assert_eq!(features[1]["properties"]["operation"], "stale");
    assert_eq!(features[2]["properties"]["operation"], "remove");
    assert_eq!(
        features[2]["properties"]["transfer_to"],
        serde_json::Value::Null
    );
}
