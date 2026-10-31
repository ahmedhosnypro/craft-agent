//! The stream-contract workflow test, split out of `main` for the
//! file-length gate.

use super::*;

#[tokio::test]
async fn the_stream_contract_gains_the_two_workflow_events() {
    let harness = rig("e2e-events", TWO_STEP_PLAN).expect("rig");
    let provider = two_step_provider();
    let step_runner = executor(&harness, provider).expect("executor");
    let mut log = journal(&harness).expect("journal");
    let report = run_or_resume(&harness.plan, &harness.args, &step_runner, &mut log)
        .await
        .expect("workflow runs");

    let (started, finished) = workflow_events(&harness, &report);
    assert_started_event_json(&harness, &started);
    assert_finished_event_json(&finished);
    assert_workflow_plain_lines(&started, &finished);
}
