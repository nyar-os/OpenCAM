use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Hole, HolePattern, MoveKind, OpenCam, Operation, OperationKind, Severity, Stock, Tool, generate_toolpaths,
    render_gcode, validate::validate,
};

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "6 mm drill".to_string(),
        number: 3,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(80.0, 60.0, 8.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

fn drill() -> Operation {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    operation.holes = vec![Hole::new(10.0, 10.0), Hole::new(30.0, 20.0)];
    operation
}

fn gridded() -> Operation {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.pattern = Some(HolePattern::grid(2, 3, 15.0, 10.0));
    operation
}

fn circular() -> Operation {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    operation.x = 40.0;
    operation.y = 30.0;
    operation.pattern = Some(HolePattern::circular(4, 10.0));
    operation
}

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

fn error_message(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.message.clone()).expect("an error")
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-pattern-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

#[test]
fn grid_pattern_drills_every_cell_in_row_major_order() {
    let project = document(gridded());
    assert_eq!(project.operations[0].resolved_holes().len(), 6);

    let report = generate_toolpaths(&project).expect("toolpaths");
    let plunges: Vec<&cam_model::Move> =
        report.operations[0].segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).collect();
    assert_eq!(plunges.len(), 6);
    assert_eq!(plunges[0].to, cam_model::Point::new(10.0, 10.0, 4.0));
    assert_eq!(plunges[5].to, cam_model::Point::new(40.0, 20.0, 4.0));

    let program = render_gcode(&project).expect("gcode");
    let cycles: Vec<&str> = program.lines().filter(|line| line.starts_with("G81 ")).collect();
    assert_eq!(
        cycles,
        [
            "G81 X10 Y10 Z4 R13 F300",
            "G81 X25 Y10 Z4 R13 F300",
            "G81 X40 Y10 Z4 R13 F300",
            "G81 X10 Y20 Z4 R13 F300",
            "G81 X25 Y20 Z4 R13 F300",
            "G81 X40 Y20 Z4 R13 F300",
        ]
    );
    assert_eq!(program.lines().filter(|line| *line == "G80").count(), 1);
}

#[test]
fn circular_pattern_starts_at_zero_degrees_and_runs_counter_clockwise() {
    let project = document(circular());
    let holes = project.operations[0].resolved_holes();
    assert_eq!(holes.len(), 4);
    let expected = [(50.0, 30.0), (40.0, 40.0), (30.0, 30.0), (40.0, 20.0)];
    for (hole, (x, y)) in holes.iter().zip(expected) {
        assert!((hole.x - x).abs() < 1e-9, "hole x {} != {x}", hole.x);
        assert!((hole.y - y).abs() < 1e-9, "hole y {} != {y}", hole.y);
    }

    let program = render_gcode(&project).expect("gcode");
    let cycles: Vec<&str> = program.lines().filter(|line| line.starts_with("G81 ")).collect();
    assert_eq!(
        cycles,
        ["G81 X50 Y30 Z4 R13 F300", "G81 X40 Y40 Z4 R13 F300", "G81 X30 Y30 Z4 R13 F300", "G81 X40 Y20 Z4 R13 F300"]
    );
}

#[test]
fn pattern_and_explicit_holes_cannot_be_combined() {
    let mut operation = gridded();
    operation.holes = vec![Hole::new(1.0, 1.0)];
    assert_eq!(error_code(operation.clone()), "operation/invalid");
    assert!(error_message(operation).contains("declares both holes and pattern"));
}

#[test]
fn pattern_is_rejected_on_every_other_kind() {
    for kind in [OperationKind::Face, OperationKind::Contour, OperationKind::Pocket, OperationKind::Engrave] {
        let mut operation = Operation::new("op", kind, "t6", 1.0, 1.0);
        if matches!(kind, OperationKind::Contour | OperationKind::Pocket) {
            operation.width = 40.0;
            operation.height = 30.0;
            operation.x = 10.0;
            operation.y = 10.0;
        }
        if kind == OperationKind::Engrave {
            operation.path = vec![cam_model::PathPoint::new(10.0, 10.0), cam_model::PathPoint::new(40.0, 10.0)];
        }
        operation.pattern = Some(HolePattern::grid(1, 1, 10.0, 10.0));
        let report = validate(&document(operation));
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.severity == Severity::Error && issue.message.contains("only `drill` supports it")),
            "kind {kind:?} must reject a pattern"
        );
    }
}

#[test]
fn invalid_pattern_geometry_is_rejected() {
    let mut no_rows = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    no_rows.pattern = Some(HolePattern::grid(0, 2, 10.0, 10.0));
    assert_eq!(error_code(no_rows), "operation/invalid");

    let mut no_columns = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    no_columns.pattern = Some(HolePattern::grid(2, 0, 10.0, 10.0));
    assert_eq!(error_code(no_columns), "operation/invalid");

    let mut zero_spacing = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    zero_spacing.pattern = Some(HolePattern::grid(2, 2, 0.0, 10.0));
    assert!(error_message(zero_spacing).contains("spacingX"));

    let mut no_count = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    no_count.pattern = Some(HolePattern::circular(0, 10.0));
    assert!(error_message(no_count).contains("count of at least one"));

    let mut zero_radius = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    zero_radius.pattern = Some(HolePattern::circular(4, -1.0));
    assert!(error_message(zero_radius).contains("positive radius"));
}

#[test]
fn unknown_pattern_kind_is_rejected() {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    let mut pattern = HolePattern::grid(1, 1, 10.0, 10.0);
    pattern.kind = "spiral".to_string();
    operation.pattern = Some(pattern);
    assert_eq!(error_code(operation.clone()), "operation/invalid");
    assert!(error_message(operation).contains("only `grid` or `circular` is supported"));
}

#[test]
fn a_generated_hole_outside_the_stock_is_rejected() {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.pattern = Some(HolePattern::grid(1, 3, 40.0, 10.0));
    assert_eq!(error_code(operation), "path/outside-stock");
}

#[test]
fn a_pattern_with_peck_emits_g83_per_generated_hole() {
    let mut operation = gridded();
    operation.peck = Some(2.0);
    let program = render_gcode(&document(operation)).expect("gcode");
    assert_eq!(program.lines().filter(|line| line.starts_with("G83 ")).count(), 6);
    assert!(program.contains("G83 X40 Y20 Z4 R13 Q2 F300"));
    assert!(!program.contains("G81"));
}

#[test]
fn explicit_holes_stay_byte_identical() {
    let program = render_gcode(&document(drill())).expect("gcode");
    let expected = [
        "%",
        concat!("(OpenCAM ", env!("CARGO_PKG_VERSION"), ")"),
        "(Units: mm)",
        "(Post: generic mill, absolute, XY plane)",
        "G21 G90 G17 G94",
        "G0 Z13",
        "(operation holes - drill)",
        "T3 M6",
        "S12000 M3",
        "(tool t6 diameter 6)",
        "G0 Z13",
        "G81 X10 Y10 Z4 R13 F300",
        "G81 X30 Y20 Z4 R13 F300",
        "G80",
        "G0 Z13",
        "M5",
        "G0 Z13",
        "M30",
        "%",
    ];
    assert_eq!(program, expected.join("\n") + "\n");
}

#[test]
fn unused_pattern_fields_are_omitted_from_the_document() {
    let text = serde_json::to_string(&circular()).expect("json");
    assert!(text.contains("\"pattern\""), "saved document: {text}");
    assert!(text.contains("\"kind\":\"circular\""), "saved document: {text}");
    assert!(text.contains("\"count\":4"), "saved document: {text}");
    assert!(!text.contains("\"rows\""), "circular pattern must omit rows: {text}");
    assert!(!text.contains("\"spacingX\""), "circular pattern must omit spacingX: {text}");

    let text = serde_json::to_string(&gridded()).expect("json");
    assert!(text.contains("\"rows\":2"), "saved document: {text}");
    assert!(text.contains("\"spacingX\":15"), "saved document: {text}");
    assert!(!text.contains("\"radius\""), "grid pattern must omit radius: {text}");
}

#[test]
fn pattern_output_is_deterministic() {
    let first = generate_toolpaths(&document(gridded())).expect("toolpaths");
    let second = generate_toolpaths(&document(gridded())).expect("toolpaths");
    assert_eq!(serde_json::to_string(&first).expect("json"), serde_json::to_string(&second).expect("json"));
    assert_eq!(render_gcode(&document(gridded())).expect("gcode"), render_gcode(&document(gridded())).expect("gcode"));
}

#[test]
fn a_pattern_round_trips_through_save_and_reopen() {
    let directory = workspace("round-trip");
    let path = directory.join("holes.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(80.0, 60.0, 8.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(gridded()).expect("operation");

    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("\"kind\": \"grid\""), "saved document: {text}");

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert!(reopened.validate().expect("validate").ok);
    let program = reopened.gcode().expect("gcode");
    assert_eq!(program.lines().filter(|line| line.starts_with("G81 ")).count(), 6);
    assert!(program.contains("G81 X40 Y20 Z4 R13 F300"));
}
