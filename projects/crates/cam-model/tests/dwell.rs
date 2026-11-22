use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Hole, MoveKind, OpenCam, Operation, OperationKind, PathPoint, Severity, Stock, Tool, generate_toolpaths,
    render_gcode, validate::validate,
};

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "6 mm flat".to_string(),
        number: 3,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(60.0, 40.0, 8.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

fn drill() -> Operation {
    let mut operation = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    operation.holes = vec![Hole::new(10.0, 10.0), Hole::new(30.0, 20.0)];
    operation
}

fn dwelling(value: f64) -> Operation {
    let mut operation = drill();
    operation.dwell = Some(value);
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
    let directory = std::env::temp_dir().join(format!("open-cam-dwell-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

#[test]
fn dwell_emits_g82_with_the_exact_p_value_and_cancels_per_hole() {
    let program = render_gcode(&document(dwelling(0.5))).expect("gcode");
    let cycles: Vec<&str> = program.lines().filter(|line| line.starts_with("G82 ")).collect();
    assert_eq!(cycles.len(), 2);
    assert_eq!(cycles[0], "G82 X10 Y10 Z4 R13 P0.5 F300");
    assert_eq!(cycles[1], "G82 X30 Y20 Z4 R13 P0.5 F300");
    assert_eq!(program.lines().filter(|line| *line == "G80").count(), 1);
    assert!(!program.contains("G81"));
    assert!(!program.contains("G83"));
    assert!(!program.contains("G4 "));
}

#[test]
fn a_drill_without_dwell_is_byte_identical_to_the_previous_output() {
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
fn dwell_changes_only_the_cycle_lines() {
    let without = render_gcode(&document(drill())).expect("gcode");
    let with = render_gcode(&document(dwelling(0.5))).expect("gcode");
    let without: Vec<&str> = without.lines().collect();
    let with: Vec<&str> = with.lines().collect();
    assert_eq!(without.len(), with.len());
    for (before, after) in without.iter().zip(&with) {
        if before.starts_with("G81 ") {
            let expected = before.replace("G81", "G82").replace(" F300", " P0.5 F300");
            assert_eq!(*after, expected);
        }
        else {
            assert_eq!(before, after);
        }
    }
}

#[test]
fn peck_with_dwell_is_rejected() {
    let mut operation = dwelling(0.5);
    operation.peck = Some(2.0);
    assert_eq!(error_code(operation.clone()), "operation/invalid");
    let message = error_message(operation);
    assert!(message.contains("combines dwell with peck"), "unexpected message: {message}");
}

#[test]
fn dwell_is_rejected_on_every_other_kind() {
    let mut face = Operation::new("face", OperationKind::Face, "t6", 1.0, 1.0);
    face.dwell = Some(0.5);

    let mut contour = Operation::new("profile", OperationKind::Contour, "t6", 3.0, 1.5);
    contour.x = 10.0;
    contour.y = 10.0;
    contour.width = 40.0;
    contour.height = 20.0;
    contour.dwell = Some(0.5);

    let mut pocket = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    pocket.x = 10.0;
    pocket.y = 10.0;
    pocket.width = 40.0;
    pocket.height = 30.0;
    pocket.dwell = Some(0.5);

    let mut engrave = Operation::new("logo", OperationKind::Engrave, "t6", 1.0, 1.0);
    engrave.path = vec![PathPoint::new(10.0, 10.0), PathPoint::new(40.0, 10.0)];
    engrave.dwell = Some(0.5);

    for operation in [face, contour, pocket, engrave] {
        let kind = operation.kind.as_str();
        assert_eq!(error_code(operation.clone()), "operation/invalid", "kind {kind}");
        let message = error_message(operation);
        assert!(message.contains("only `drill` supports it"), "kind {kind}: {message}");
    }
}

#[test]
fn zero_negative_and_nan_dwell_are_rejected() {
    for value in [0.0, -1.0, f64::NAN] {
        assert_eq!(error_code(dwelling(value)), "operation/invalid");
        let message = error_message(dwelling(value));
        assert!(message.contains("finite and greater than zero"), "value {value}: {message}");
    }
}

#[test]
fn dwell_does_not_change_toolpath_moves() {
    let without = document(drill());
    let with = document(dwelling(0.5));
    let plain = generate_toolpaths(&without).expect("toolpaths");
    let paused = generate_toolpaths(&with).expect("toolpaths");
    assert_eq!(plain, paused);
    assert_eq!(serde_json::to_string(&plain).expect("json"), serde_json::to_string(&paused).expect("json"));
    assert_eq!(plain.operations[0].segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).count(), 2);
}

#[test]
fn dwell_output_is_deterministic() {
    let first = render_gcode(&document(dwelling(0.5))).expect("gcode");
    let second = render_gcode(&document(dwelling(0.5))).expect("gcode");
    assert_eq!(first, second);
    let first_report = generate_toolpaths(&document(dwelling(0.5))).expect("toolpaths");
    let second_report = generate_toolpaths(&document(dwelling(0.5))).expect("toolpaths");
    assert_eq!(serde_json::to_string(&first_report).expect("json"), serde_json::to_string(&second_report).expect("json"));
}

#[test]
fn unset_dwell_is_omitted_from_json() {
    let text = serde_json::to_string(&drill()).expect("json");
    assert!(!text.contains("dwell"), "unset dwell must be omitted: {text}");
}

#[test]
fn dwell_round_trips_through_save_and_reopen() {
    let directory = workspace("round-trip");
    let path = directory.join("holes.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(60.0, 40.0, 8.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(dwelling(0.5)).expect("operation");

    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("\"dwell\": 0.5"), "saved document: {text}");

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert!(reopened.validate().expect("validate").ok);
    let program = reopened.gcode().expect("gcode");
    assert!(program.contains("G82 X10 Y10 Z4 R13 P0.5 F300"));
    assert!(program.contains("G80"));
}
