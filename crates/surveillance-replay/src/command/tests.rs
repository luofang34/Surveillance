#![allow(clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn a_fixed_producer_identity_is_used_without_random_input() {
    assert_eq!(
        resolve_producer_instance_id(Some(71)).expect("a fixed identity needs no random source"),
        ProducerInstanceId::new(71)
    );
}

#[test]
fn a_malformed_stream_keeps_its_line_context() {
    let mut output = Vec::new();
    let error = replay_stream(
        b"{ not json\n".as_slice(),
        &mut output,
        ProducerInstanceId::new(71),
        OutputFormat::TrackRecord,
    )
    .expect_err("malformed JSON must stop the command");

    match error {
        ReplayRunError::Replay(source) => assert_eq!(source.line, 1),
        other => panic!("expected a replay error, got {other:?}"),
    }
}

#[test]
fn an_output_path_cannot_be_the_input_path() {
    let path = Path::new("recording.ndjson");

    assert!(matches!(
        ensure_distinct_output(path, Some(path)),
        Err(ReplayCommandError::InputIsOutput { .. })
    ));
}
