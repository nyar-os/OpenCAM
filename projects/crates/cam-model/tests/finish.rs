use cam_model::{
    CamDocument, Island, MoveKind, Operation, OperationKind, OperationPath, Point, Severity, Stock, Tool, generate_toolpaths,
    render_gcode, validate::validate,
};

const RADIUS: f64 = 3.0;

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "6 mm flat".to_string(),
        number: 1,
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

/// Raster pocket `(10, 10) … (50, 40)` with three raster lines per level.
fn raster() -> Operation {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.stepover = Some(6.0);
    operation
}

fn finished(mut operation: Operation) -> Operation {
    operation.finish = Some(true);
    operation
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

/// True when the path cuts the given rectangle in one closed loop at `z`.
fn has_wall_loop(path: &OperationPath, z: f64, corners: [(f64, f64); 4]) -> bool {
    let expected = [corners[0], corners[1], corners[2], corners[3], corners[0]];
    let horizontal: Vec<&cam_model::Move> = path
        .segments
        .iter()
        .filter(|segment| segment.kind == MoveKind::Cut && close(segment.from.z, z) && close(segment.to.z, z))
        .collect();
    horizontal
        .windows(expected.len())
        .any(|window| window.iter().zip(expected).all(|(segment, (x, y))| close(segment.to.x, x) && close(segment.to.y, y)))
}

fn rectangle_clearance(operation: &Operation, point: Point) -> f64 {
    let clearance = (point.x - operation.x).min(operation.x + operation.width - point.x);
    clearance.min((point.y - operation.y).min(operation.y + operation.height - point.y))
}

#[test]
fn a_pocket_without_finish_is_byte_identical_to_the_previous_output() {
    let base = document(raster());
    let expected = [
        "%",
        concat!("(OpenCAM ", env!("CARGO_PKG_VERSION"), ")"),
        "(Units: mm)",
        "(Post: generic mill, absolute, XY plane)",
        "G21 G90 G17 G94",
        "G0 Z15",
        "(operation pocket - pocket)",
        "T1 M6",
        "S12000 M3",
        "(tool t6 diameter 6)",
        "G0 Z15",
        "G0 X13 Y13 Z15",
        "G1 X13 Y13 Z9 F300",
        "G1 X47 Y13 Z9 F1200",
        "G1 X47 Y19 Z9 F1200",
        "G1 X13 Y19 Z9 F1200",
        "G1 X13 Y25 Z9 F1200",
        "G1 X47 Y25 Z9 F1200",
        "G1 X47 Y31 Z9 F1200",
        "G1 X13 Y31 Z9 F1200",
        "G1 X13 Y37 Z9 F1200",
        "G1 X47 Y37 Z9 F1200",
        "G0 X47 Y37 Z15",
        "G0 X13 Y13 Z15",
        "G1 X13 Y13 Z8 F300",
        "G1 X47 Y13 Z8 F1200",
        "G1 X47 Y19 Z8 F1200",
        "G1 X13 Y19 Z8 F1200",
        "G1 X13 Y25 Z8 F1200",
        "G1 X47 Y25 Z8 F1200",
        "G1 X47 Y31 Z8 F1200",
        "G1 X13 Y31 Z8 F1200",
        "G1 X13 Y37 Z8 F1200",
        "G1 X47 Y37 Z8 F1200",
        "G0 X47 Y37 Z15",
        "G0 Z15",
        "M5",
        "G0 Z15",
        "M30",
        "%",
    ]
    .join("\n")
        + "\n";

    assert_eq!(render_gcode(&base).expect("gcode"), expected);
    assert!(!serde_json::to_string(&base).expect("serialise").contains("finish"));

    let mut explicitly_off = raster();
    explicitly_off.finish = Some(false);
    assert_eq!(
        render_gcode(&document(explicitly_off)).expect("gcode"),
        render_gcode(&base).expect("gcode"),
        "finish: false must keep the clearing output"
    );
}

#[test]
fn finish_adds_one_wall_loop_per_level_and_cutting_length() {
    let plain = generate_toolpaths(&document(raster())).expect("valid raster pocket");
    let report = generate_toolpaths(&document(finished(raster()))).expect("valid finished pocket");
    let path = &report.operations[0];

    for z in [9.0, 8.0] {
        assert!(
            has_wall_loop(path, z, [(13.0, 13.0), (47.0, 13.0), (47.0, 37.0), (13.0, 37.0)]),
            "level z {z} has no closed wall loop"
        );
    }

    let cut: f64 = path.segments.iter().filter(|segment| segment.kind != MoveKind::Rapid).map(|segment| segment.length).sum();
    assert!(close(cut, path.cut_length), "cut length must include the finish moves");
    assert!(path.cut_length > plain.operations[0].cut_length, "finish must add cutting length");
    assert!(report.estimated_seconds > plain.estimated_seconds, "finish must add cycle time");

    // Two levels, each one 116 mm loop plus a diagonal approach.
    let added = path.cut_length - plain.operations[0].cut_length;
    assert!(added > 2.0 * 116.0, "finish added only {added} mm");
}

#[test]
fn finish_keeps_the_level_depth_and_the_cutter_radius() {
    let operation = finished(raster());
    let report = generate_toolpaths(&document(operation.clone())).expect("valid finished pocket");
    let path = &report.operations[0];

    let lowest = path.segments.iter().flat_map(|segment| [segment.from.z, segment.to.z]).fold(f64::INFINITY, f64::min);
    assert!(close(lowest, 8.0), "the finish pass must not cut below the last level");

    for segment in &path.segments {
        if segment.kind == MoveKind::Rapid {
            continue;
        }
        let points = [
            segment.from,
            segment.to,
            Point::new((segment.from.x + segment.to.x) / 2.0, (segment.from.y + segment.to.y) / 2.0, segment.from.z),
        ];
        for point in points {
            let clearance = rectangle_clearance(&operation, point);
            assert!(
                clearance >= RADIUS - 1e-9,
                "cut point ({}, {}) is {} mm from the wall, radius {}",
                point.x,
                point.y,
                clearance,
                RADIUS
            );
        }
    }
}

#[test]
fn spiral_finish_adds_a_closed_wall_loop_without_changing_the_clearing_loops() {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 80.0;
    operation.height = 50.0;
    operation.stepover = Some(6.0);
    operation.strategy = Some("spiral".to_string());

    let plain = generate_toolpaths(&document(operation.clone())).expect("valid spiral pocket");
    let report = generate_toolpaths(&document(finished(operation))).expect("valid finished spiral pocket");
    let path = &report.operations[0];

    assert_eq!(path.loop_count, Some(4), "the finish loop is not a clearing loop");
    assert_eq!(path.segments.len(), plain.operations[0].segments.len() + 5, "one approach plus four wall edges are added");
    assert!(has_wall_loop(path, 8.0, [(13.0, 13.0), (87.0, 13.0), (87.0, 57.0), (13.0, 57.0)]));
}

#[test]
fn circular_finish_adds_a_ring_at_the_nominal_wall_radius() {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 60.0;
    operation.height = 60.0;
    operation.stepover = Some(6.0);
    operation.shape = Some("circle".to_string());

    let plain = generate_toolpaths(&document(operation.clone())).expect("valid circular pocket");
    let report = generate_toolpaths(&document(finished(operation))).expect("valid finished circular pocket");
    let path = &report.operations[0];

    assert_eq!(path.loop_count, plain.operations[0].loop_count);
    assert_eq!(path.segments.len(), plain.operations[0].segments.len() + 73, "one approach plus 72 ring segments");

    let centre = Point::new(40.0, 40.0, 0.0);
    let finish: Vec<&cam_model::Move> =
        path.segments.iter().rev().filter(|segment| segment.kind != MoveKind::Rapid).take(73).collect();
    for segment in &finish {
        let distance = ((segment.to.x - centre.x).powi(2) + (segment.to.y - centre.y).powi(2)).sqrt();
        assert!(distance <= 27.0 + 1e-9, "finish point is {} mm from the centre", distance);
        assert!(close(segment.to.z, 8.0), "the finish ring must stay at the level depth");
    }
    let last = finish[0];
    assert!(close(last.to.x, 67.0) && close(last.to.y, 40.0), "the finish ring must close on the wall start");
}

#[test]
fn finish_is_pocket_only_and_rejects_islands() {
    let mut face = Operation::new("face", OperationKind::Face, "t6", 1.0, 1.0);
    face.finish = Some(true);
    let report = validate(&document(face));
    assert!(report.issues.iter().any(|issue| { issue.code == "operation/invalid" && issue.message.contains("only `pocket`") }));

    let mut drill = Operation::new("holes", OperationKind::Drill, "t6", 2.0, 2.0);
    drill.holes = vec![cam_model::Hole::new(20.0, 20.0)];
    drill.finish = Some(true);
    assert_eq!(error_code(drill), "operation/invalid");

    let mut islanded = raster();
    islanded.finish = Some(true);
    islanded.islands = vec![Island::new(20.0, 18.0, 10.0, 10.0)];
    assert_eq!(error_code(islanded.clone()), "operation/invalid");

    islanded.finish = Some(false);
    assert!(validate(&document(islanded)).ok, "finish: false must leave an island pocket valid");
}

#[test]
fn spiral_and_circular_pockets_accept_finish() {
    let mut spiral = raster();
    spiral.strategy = Some("spiral".to_string());
    spiral.finish = Some(true);
    assert!(validate(&document(spiral)).ok);

    let mut circle = raster();
    circle.shape = Some("circle".to_string());
    circle.width = 40.0;
    circle.height = 40.0;
    circle.finish = Some(true);
    assert!(validate(&document(circle)).ok);
}

#[test]
fn finish_output_is_deterministic_and_round_trips() {
    let first = generate_toolpaths(&document(finished(raster()))).expect("valid");
    let second = generate_toolpaths(&document(finished(raster()))).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));

    let document = document(finished(raster()));
    let text = serde_json::to_string(&document).expect("serialise document");
    assert!(text.contains("\"finish\":true"));
    let restored: CamDocument = serde_json::from_str(&text).expect("deserialise");
    assert_eq!(restored.operations[0].finish, Some(true));
    assert_eq!(serde_json::to_string(&restored).expect("reserialise"), text);
}
