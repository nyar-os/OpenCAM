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

/// Closed square `(10, 10) -> (40, 10) -> (40, 40) -> (10, 40) -> (10, 10)`:
/// four 30 mm sides, so the closed loop cuts 120 mm per level and the added
/// closing side is 30 mm.
fn closed_square() -> Operation {
    let mut operation = Operation::new("frame", OperationKind::Engrave, "t6", 2.0, 2.0);
    operation.path =
        vec![PathPoint::new(10.0, 10.0), PathPoint::new(40.0, 10.0), PathPoint::new(40.0, 40.0), PathPoint::new(10.0, 40.0)];
    operation.closed = true;
    operation
}

#[test]
fn closed_engrave_cuts_the_square_perimeter() {
    let report = generate_toolpaths(&document(closed_square())).expect("valid closed engrave");
    let path = &report.operations[0];

    let cuts: Vec<_> = path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).collect();
    assert_eq!(cuts.len(), 4);
    let cut_xy: f64 = cuts.iter().map(|segment| segment.from.distance_to(segment.to)).sum();
    assert!((cut_xy - 120.0).abs() < 1e-9);

    let plunges: f64 =
        path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).map(|segment| segment.length).sum();
    assert!((path.cut_length - (120.0 + plunges)).abs() < 1e-9);

    let last = cuts.last().expect("last cut");
    assert!((last.to.x - 10.0).abs() < 1e-9);
    assert!((last.to.y - 10.0).abs() < 1e-9);
    assert!((last.to.z - 8.0).abs() < 1e-9);
}

#[test]
fn closed_engrave_adds_one_move_per_level() {
    let mut open = closed_square();
    open.closed = false;
    let closed = closed_square();
    let open_report = generate_toolpaths(&document(open)).expect("valid open engrave");
    let closed_report = generate_toolpaths(&document(closed)).expect("valid closed engrave");
    let open_path = &open_report.operations[0];
    let closed_path = &closed_report.operations[0];

    assert_eq!(open_path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count(), 3);
    assert_eq!(closed_path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count(), 4);
    assert_eq!(closed_path.moves - open_path.moves, 1);
    assert!((closed_path.cut_length - open_path.cut_length - 30.0).abs() < 1e-9);

    let mut open_levels = closed_square();
    open_levels.closed = false;
    open_levels.depth = 6.0;
    open_levels.stepdown = 2.0;
    let mut closed_levels = closed_square();
    closed_levels.depth = 6.0;
    closed_levels.stepdown = 2.0;
    let open_levels = generate_toolpaths(&document(open_levels)).expect("valid open engrave");
    let closed_levels = generate_toolpaths(&document(closed_levels)).expect("valid closed engrave");
    let open_path = &open_levels.operations[0];
    let closed_path = &closed_levels.operations[0];

    assert_eq!(open_path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count(), 9);
    assert_eq!(closed_path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count(), 12);
    assert!((closed_path.cut_length - open_path.cut_length - 90.0).abs() < 1e-9);
}

#[test]
fn closed_engrave_adds_one_gcode_line_per_level() {
    let mut open = closed_square();
    open.closed = false;
    let closed = closed_square();

    let open_lines = render_gcode(&document(open)).expect("gcode").lines().count();
    let closed_lines = render_gcode(&document(closed)).expect("gcode").lines().count();
    assert_eq!(closed_lines, open_lines + 1);
}

#[test]
fn closed_engrave_rejects_a_last_point_on_the_first() {
    let mut operation = closed_square();
    operation.path.push(PathPoint::new(10.0, 10.0));
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("coincides with the first"))
    );
}

#[test]
fn closed_is_engrave_only() {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.closed = true;
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("only `engrave` supports it"))
    );
}

#[test]
fn an_open_engrave_keeps_the_previous_bytes() {
    let document = document(engrave());
    let text = serde_json::to_string(&document).expect("serialise");
    let expected = concat!(
        "{\"format\":\"open-cam\",\"version\":1,\"revision\":0,\"units\":\"mm\",",
        "\"stock\":{\"width\":100.0,\"depth\":80.0,\"height\":10.0},",
        "\"tools\":[{\"id\":\"t6\",\"name\":\"6 mm flat\",\"number\":7,\"diameter\":6.0,\"flutes\":2,",
        "\"spindle\":12000.0,\"feed\":1200.0,\"plungeFeed\":300.0}],",
        "\"operations\":[{\"id\":\"logo\",\"name\":\"\",\"kind\":\"engrave\",\"tool\":\"t6\",\"depth\":2.0,",
        "\"stepdown\":2.0,\"stepover\":null,\"x\":0.0,\"y\":0.0,\"width\":0.0,\"height\":0.0,\"holes\":[],",
        "\"path\":[{\"x\":10.0,\"y\":10.0},{\"x\":30.0,\"y\":10.0},{\"x\":30.0,\"y\":40.0}],\"peck\":null}]}"
    );
    assert_eq!(text, expected);
    assert!(!text.contains("closed"));
}
