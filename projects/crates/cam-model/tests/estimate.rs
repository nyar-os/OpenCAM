use cam_model::{
    CamDocument, EstimateRequest, Hole, Operation, OperationKind, Stock, Tool, estimate_cycle_time, generate_toolpaths,
};

fn tool(id: &str, number: u32) -> Tool {
    Tool {
        id: id.to_string(),
        name: format!("{id} flat"),
        number,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn document(tools: Vec<Tool>, operations: Vec<Operation>) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(100.0, 80.0, 10.0));
    document.tools = tools;
    document.operations = operations;
    document
}

fn face(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Face, tool, 1.0, 0.5);
    operation.stepover = Some(4.0);
    operation
}

#[test]
fn defaults_match_the_toolpath_estimate_plus_tool_changes() {
    let document = document(vec![tool("t6", 1)], vec![face("face", "t6")]);
    let report = estimate_cycle_time(&document, &EstimateRequest::default()).expect("valid document");
    let paths = generate_toolpaths(&document).expect("valid document");
    let motion = report.cutting_seconds + report.plunge_seconds + report.retract_seconds + report.traverse_seconds;
    assert!((motion - paths.estimated_seconds).abs() < 1e-6);
    assert_eq!(report.tool_changes, 1);
    assert!((report.total_seconds - motion - 3.0).abs() < 1e-6);
    assert_eq!(report.format, "open-cam-cycle-time");
    assert_eq!(report.version, 1);
}

#[test]
fn tool_change_is_charged_once_per_tool() {
    let shared = document(vec![tool("t6", 1)], vec![face("first", "t6"), face("second", "t6")]);
    let report = estimate_cycle_time(&shared, &EstimateRequest::default()).expect("valid document");
    assert_eq!(report.tool_changes, 1);
    assert_eq!(report.operations[0].toolchange_seconds, 3.0);
    assert_eq!(report.operations[1].toolchange_seconds, 0.0);

    let distinct = document(vec![tool("t6", 1), tool("t8", 2)], vec![face("first", "t6"), face("second", "t8")]);
    let report = estimate_cycle_time(&distinct, &EstimateRequest::default()).expect("valid document");
    assert_eq!(report.tool_changes, 2);
    assert!((report.toolchange_seconds - 6.0).abs() < 1e-9);
}

#[test]
fn rapid_feed_override_scales_rapid_time() {
    let document = document(vec![tool("t6", 1)], vec![face("face", "t6")]);
    let default = estimate_cycle_time(&document, &EstimateRequest::default()).expect("valid");
    let half =
        estimate_cycle_time(&document, &EstimateRequest { rapid_feed: 4000.0, ..EstimateRequest::default() }).expect("valid");
    let default_rapid = default.retract_seconds + default.traverse_seconds;
    let half_rapid = half.retract_seconds + half.traverse_seconds;
    assert!((half_rapid - 2.0 * default_rapid).abs() < 1e-6);
    assert!((half.cutting_seconds - default.cutting_seconds).abs() < 1e-9);
    assert!((half.plunge_seconds - default.plunge_seconds).abs() < 1e-9);
}

#[test]
fn drill_separates_plunge_retract_and_traverse() {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 5.0, 5.0);
    operation.holes = vec![Hole::new(20.0, 20.0), Hole::new(60.0, 40.0)];
    let document = document(vec![tool("t6", 1)], vec![operation]);
    let report = estimate_cycle_time(&document, &EstimateRequest::default()).expect("valid document");
    let path = &report.operations[0];
    // Each hole plunges 10 mm from the clearance plane at 300 mm/min and lifts
    // the same distance at the 8000 mm/min rapid feed.
    assert!((path.plunge_length - 20.0).abs() < 1e-6);
    assert!((path.retract_length - 20.0).abs() < 1e-6);
    assert!((path.plunge_seconds - 4.0).abs() < 1e-6);
    assert!((path.retract_seconds - 0.15).abs() < 1e-6);
    assert_eq!(path.cutting_seconds, 0.0);
    assert!(path.traverse_seconds > 0.0);
    assert!((path.total_seconds - path.plunge_seconds - path.retract_seconds - path.traverse_seconds - 3.0).abs() < 1e-6);
}

#[test]
fn invalid_request_is_rejected() {
    let document = document(vec![tool("t6", 1)], vec![face("face", "t6")]);
    let error = estimate_cycle_time(&document, &EstimateRequest { rapid_feed: 0.0, ..EstimateRequest::default() })
        .expect_err("zero rapid feed");
    assert_eq!(error.code, "estimate/invalid");
    let error = estimate_cycle_time(&document, &EstimateRequest { tool_change_seconds: -1.0, ..EstimateRequest::default() })
        .expect_err("negative overhead");
    assert_eq!(error.code, "estimate/invalid");
}

#[test]
fn request_defaults_deserialize_from_empty_object() {
    let request: EstimateRequest = serde_json::from_str("{}").expect("empty object");
    assert_eq!(request, EstimateRequest::default());
    let request: EstimateRequest = serde_json::from_str(r#"{"rapidFeed":4000}"#).expect("partial object");
    assert_eq!(request.rapid_feed, 4000.0);
    assert_eq!(request.tool_change_seconds, 3.0);
}

#[test]
fn empty_document_estimates_zero() {
    let document = CamDocument::new(Stock::new(100.0, 80.0, 10.0));
    let report = estimate_cycle_time(&document, &EstimateRequest::default()).expect("warning-only document");
    assert_eq!(report.operations.len(), 0);
    assert_eq!(report.tool_changes, 0);
    assert_eq!(report.total_seconds, 0.0);
}

#[test]
fn estimate_is_deterministic() {
    let document = document(vec![tool("t6", 1)], vec![face("face", "t6")]);
    let first =
        serde_json::to_string(&estimate_cycle_time(&document, &EstimateRequest::default()).expect("valid")).expect("json");
    let second =
        serde_json::to_string(&estimate_cycle_time(&document, &EstimateRequest::default()).expect("valid")).expect("json");
    assert_eq!(first, second);
}
