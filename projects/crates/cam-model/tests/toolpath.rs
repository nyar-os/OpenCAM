use cam_model::{CamDocument, Hole, Operation, OperationKind, Stock, Tool, generate_toolpaths};

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

fn face() -> Operation {
    let mut operation = Operation::new("face", OperationKind::Face, "t6", 1.0, 0.5);
    operation.stepover = Some(4.0);
    operation
}

#[test]
fn face_uses_one_pass_per_stepdown() {
    let report = generate_toolpaths(&document(face())).expect("valid face operation");
    let path = &report.operations[0];
    // Two Z levels (0.5 and 1.0); each level rasters the whole 100 x 80 top.
    assert_eq!(report.safe_z, 15.0);
    assert!(path.segments.iter().any(|segment| (segment.to.z - 9.5).abs() < 1e-9));
    assert!(path.segments.iter().any(|segment| (segment.to.z - 9.0).abs() < 1e-9));
    assert!(path.cut_length > 100.0);
    assert!(report.estimated_seconds > 0.0);
}

#[test]
fn face_is_deterministic() {
    let first = generate_toolpaths(&document(face())).expect("valid");
    let second = generate_toolpaths(&document(face())).expect("valid");
    let first_json = serde_json::to_string(&first).expect("serialise");
    let second_json = serde_json::to_string(&second).expect("serialise");
    assert_eq!(first_json, second_json);
}

#[test]
fn contour_closes_each_level() {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 3.0, 1.5);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 20.0;
    let report = generate_toolpaths(&document(operation)).expect("valid contour");
    let path = &report.operations[0];
    let cuts: Vec<_> = path.segments.iter().filter(|segment| segment.kind == cam_model::MoveKind::Cut).collect();
    // Four edges per level, two levels: the path returns to its start corner.
    assert_eq!(cuts.len(), 8);
    let first = cuts[0];
    let last = *cuts.last().expect("last cut");
    assert!((last.to.x - first.from.x).abs() < 1e-9);
    assert!((last.to.y - first.from.y).abs() < 1e-9);
}

#[test]
fn pocket_stays_inside_the_rectangle() {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.stepover = Some(3.0);
    let report = generate_toolpaths(&document(operation)).expect("valid pocket");
    for segment in &report.operations[0].segments {
        assert!(segment.to.x >= 13.0 - 1e-9 && segment.to.x <= 47.0 + 1e-9);
        assert!(segment.to.y >= 13.0 - 1e-9 && segment.to.y <= 37.0 + 1e-9);
    }
}

#[test]
fn drill_emits_one_plunge_per_hole() {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 5.0, 5.0);
    operation.holes = vec![Hole::new(20.0, 20.0), Hole::new(60.0, 40.0)];
    operation.peck = Some(2.0);
    let document = document(operation);
    let report = generate_toolpaths(&document).expect("valid drill");
    let plunges = report.operations[0].segments.iter().filter(|segment| segment.kind == cam_model::MoveKind::Plunge);
    assert_eq!(plunges.count(), 2);
    let gcode = cam_model::render_gcode(&document).expect("gcode");
    assert!(gcode.contains("G83"));
    assert!(gcode.contains("Q2"));
}
