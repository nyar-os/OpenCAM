use cam_model::{
    CamDocument, Hole, MoveKind, Operation, OperationKind, PathPoint, Stock, Tool, generate_toolpaths, render_gcode,
    validate::validate,
};

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "6 mm flat".to_string(),
        number: 7,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(100.0, 80.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

/// Polyline `(10, 10) -> (30, 10) -> (30, 40)`: two segments of 20 mm and 30 mm.
fn engrave() -> Operation {
    let mut operation = Operation::new("logo", OperationKind::Engrave, "t6", 2.0, 2.0);
    operation.path = vec![PathPoint::new(10.0, 10.0), PathPoint::new(30.0, 10.0), PathPoint::new(30.0, 40.0)];
    operation
}

#[test]
fn engrave_plunges_once_and_cuts_each_segment() {
    let report = generate_toolpaths(&document(engrave())).expect("valid engrave");
    assert_eq!(report.safe_z, 15.0);
    let path = &report.operations[0];

    let plunges: Vec<_> = path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).collect();
    assert_eq!(plunges.len(), 1);
    let cuts: Vec<_> = path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).collect();
    assert_eq!(cuts.len(), 2);

    let cut_xy: f64 = cuts.iter().map(|segment| segment.from.distance_to(segment.to)).sum();
    assert!((cut_xy - 50.0).abs() < 1e-9);
    let plunge_length: f64 = plunges.iter().map(|segment| segment.length).sum();
    assert!((path.cut_length - (cut_xy + plunge_length)).abs() < 1e-9);

    let last = cuts.last().expect("last cut");
    assert!((last.to.x - 30.0).abs() < 1e-9);
    assert!((last.to.y - 40.0).abs() < 1e-9);
    assert!((last.to.z - 8.0).abs() < 1e-9);
}

#[test]
fn engrave_repeats_the_polyline_at_every_level() {
    let mut operation = engrave();
    operation.depth = 6.0;
    operation.stepdown = 2.0;
    let report = generate_toolpaths(&document(operation)).expect("valid multi-level engrave");
    let path = &report.operations[0];

    let plunges: Vec<_> = path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).collect();
    assert_eq!(plunges.len(), 3);
    assert_eq!(path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count(), 6);

    for (plunge, expected_z) in plunges.iter().zip([8.0, 6.0, 4.0]) {
        assert!((plunge.to.z - expected_z).abs() < 1e-9);
    }
}

#[test]
fn engrave_needs_at_least_two_points() {
    let mut operation = Operation::new("logo", OperationKind::Engrave, "t6", 2.0, 2.0);
    operation.path = vec![PathPoint::new(10.0, 10.0)];
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("at least two path points"))
    );
}

#[test]
fn engrave_path_must_stay_on_the_stock() {
    let mut operation = engrave();
    operation.path = vec![PathPoint::new(10.0, 10.0), PathPoint::new(120.0, 10.0)];
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "path/outside-stock"));
}

#[test]
fn engrave_rejects_adjacent_duplicate_points() {
    let mut operation = engrave();
    operation.path = vec![PathPoint::new(10.0, 10.0), PathPoint::new(10.0, 10.0), PathPoint::new(30.0, 10.0)];
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "operation/invalid" && issue.message.contains("coincide")));
}

#[test]
fn engrave_rejects_holes() {
    let mut operation = engrave();
    operation.holes = vec![Hole::new(20.0, 20.0)];
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("only `drill` supports them"))
    );
}

#[test]
fn engrave_gcode_uses_linear_moves_and_selects_the_tool() {
    let program = render_gcode(&document(engrave())).expect("gcode");
    assert!(program.contains("(operation logo - engrave)"));
    assert!(program.contains("T7 M6"));
    assert!(program.contains("G1 X30 Y10 Z8 F1200"));
    assert!(program.contains("G1 X30 Y40 Z8 F1200"));
}

#[test]
fn an_empty_path_is_not_serialized() {
    let operation = Operation::new("logo", OperationKind::Engrave, "t6", 2.0, 2.0);
    let json = serde_json::to_value(&operation).expect("serialise");
    assert!(json.get("path").is_none());

    let mut with_path = Operation::new("logo", OperationKind::Engrave, "t6", 2.0, 2.0);
    with_path.path = vec![PathPoint::new(10.0, 10.0), PathPoint::new(30.0, 10.0)];
    let json = serde_json::to_value(&with_path).expect("serialise");
    assert_eq!(json["path"][0]["x"].as_f64(), Some(10.0));
    assert_eq!(json["path"][1]["y"].as_f64(), Some(10.0));

    let restored: Operation = serde_json::from_value(json).expect("deserialise");
    assert_eq!(restored.path, with_path.path);
}
