use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Island, MoveKind, OpenCam, Operation, OperationKind, Severity, Stock, Tool, generate_toolpaths, render_gcode,
    validate::validate,
};

const RADIUS: f64 = 3.0;

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "t6".to_string(),
        number: 1,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn pocket(islands: Vec<Island>) -> Operation {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 1.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 100.0;
    operation.height = 70.0;
    operation.stepover = Some(3.0);
    operation.islands = islands;
    operation
}

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

fn distance_to_island(x: f64, y: f64, island: &Island) -> f64 {
    let dx = (island.x - x).max(0.0).max(x - (island.x + island.width));
    let dy = (island.y - y).max(0.0).max(y - (island.y + island.height));
    (dx * dx + dy * dy).sqrt()
}

fn assert_clear_of_islands(operation: &Operation, document: &CamDocument) {
    let report = generate_toolpaths(document).expect("valid island pocket");
    let path = &report.operations[0];
    for segment in &path.segments {
        if segment.kind == MoveKind::Rapid {
            assert!((segment.to.z - report.safe_z).abs() < 1e-9, "rapid must stay at the clearance plane");
            continue;
        }
        let points = [
            segment.from,
            segment.to,
            cam_model::Point::new((segment.from.x + segment.to.x) / 2.0, (segment.from.y + segment.to.y) / 2.0, segment.from.z),
        ];
        for point in points {
            for island in &operation.islands {
                let distance = distance_to_island(point.x, point.y, island);
                assert!(
                    distance >= RADIUS - 1e-9,
                    "cut point ({}, {}) is {} mm from island at ({}, {}), radius {}",
                    point.x,
                    point.y,
                    distance,
                    island.x,
                    island.y,
                    RADIUS
                );
            }
        }
    }
}

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-islands-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

#[test]
fn a_single_island_is_cleared_around() {
    let operation = pocket(vec![Island::new(40.0, 35.0, 20.0, 20.0)]);
    let document = document(operation.clone());
    let report = generate_toolpaths(&document).expect("valid island pocket");
    let path = &report.operations[0];
    assert_eq!(path.island_count, 1);
    assert!(path.skipped_length > 0.0);
    assert!(path.skipped_segments > 0);
    assert_clear_of_islands(&operation, &document);

    let program = render_gcode(&document).expect("gcode");
    assert!(program.contains("G0 X"));
    assert!(
        path.segments.iter().any(|segment| segment.kind == MoveKind::Rapid
            && (segment.from.y - segment.to.y).abs() < 1e-9
            && segment.from.x.min(segment.to.x) <= 50.0
            && segment.from.x.max(segment.to.x) >= 50.0),
        "a rapid must cross the island at the clearance plane"
    );
}

#[test]
fn multiple_islands_are_cleared_around() {
    let operation = pocket(vec![Island::new(40.0, 35.0, 20.0, 20.0), Island::new(75.0, 20.0, 15.0, 15.0)]);
    let document = document(operation.clone());
    let report = generate_toolpaths(&document).expect("valid island pocket");
    assert_eq!(report.operations[0].island_count, 2);
    assert_clear_of_islands(&operation, &document);
}

#[test]
fn island_validation_reports_stable_codes() {
    assert_eq!(error_code(pocket(vec![Island::new(40.0, 35.0, 0.0, 20.0)])), "operation/island-invalid");
    assert_eq!(error_code(pocket(vec![Island::new(f64::NAN, 35.0, 10.0, 20.0)])), "operation/island-invalid");
    assert_eq!(error_code(pocket(vec![Island::new(5.0, 20.0, 10.0, 10.0)])), "operation/island-outside-pocket");
    assert_eq!(error_code(pocket(vec![Island::new(13.0, 13.0, 10.0, 10.0)])), "operation/island-too-close");
    assert_eq!(
        error_code(pocket(vec![Island::new(40.0, 30.0, 10.0, 10.0), Island::new(50.0, 35.0, 10.0, 10.0)])),
        "operation/island-overlap"
    );
}

#[test]
fn islands_are_pocket_only() {
    let mut operation = Operation::new("face", OperationKind::Face, "t6", 1.0, 1.0);
    operation.islands = vec![Island::new(10.0, 10.0, 5.0, 5.0)];
    assert_eq!(error_code(operation), "operation/invalid");
}

#[test]
fn a_rejected_island_is_not_written() {
    let directory = workspace("rejected");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");

    let error = session.add_operation(pocket(vec![Island::new(5.0, 20.0, 10.0, 10.0)])).expect_err("island outside");
    assert_eq!(error.code, "operation/island-outside-pocket");
    assert_eq!(session.summary().expect("summary").operations, 0);

    let text = fs::read_to_string(&path).expect("read document");
    let stored: CamDocument = serde_json::from_str(&text).expect("parse document");
    assert!(stored.operations.is_empty());
}

#[test]
fn a_pocket_without_islands_is_byte_identical_to_the_previous_output() {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.stepover = Some(3.0);
    let mut document = CamDocument::new(Stock::new(100.0, 80.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);

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
        "G1 X47 Y16 Z9 F1200",
        "G1 X13 Y16 Z9 F1200",
        "G1 X13 Y19 Z9 F1200",
        "G1 X47 Y19 Z9 F1200",
        "G1 X47 Y22 Z9 F1200",
        "G1 X13 Y22 Z9 F1200",
        "G1 X13 Y25 Z9 F1200",
        "G1 X47 Y25 Z9 F1200",
        "G1 X47 Y28 Z9 F1200",
        "G1 X13 Y28 Z9 F1200",
        "G1 X13 Y31 Z9 F1200",
        "G1 X47 Y31 Z9 F1200",
        "G1 X47 Y34 Z9 F1200",
        "G1 X13 Y34 Z9 F1200",
        "G1 X13 Y37 Z9 F1200",
        "G1 X47 Y37 Z9 F1200",
        "G0 X47 Y37 Z15",
        "G0 X13 Y13 Z15",
        "G1 X13 Y13 Z8 F300",
        "G1 X47 Y13 Z8 F1200",
        "G1 X47 Y16 Z8 F1200",
        "G1 X13 Y16 Z8 F1200",
        "G1 X13 Y19 Z8 F1200",
        "G1 X47 Y19 Z8 F1200",
        "G1 X47 Y22 Z8 F1200",
        "G1 X13 Y22 Z8 F1200",
        "G1 X13 Y25 Z8 F1200",
        "G1 X47 Y25 Z8 F1200",
        "G1 X47 Y28 Z8 F1200",
        "G1 X13 Y28 Z8 F1200",
        "G1 X13 Y31 Z8 F1200",
        "G1 X47 Y31 Z8 F1200",
        "G1 X47 Y34 Z8 F1200",
        "G1 X13 Y34 Z8 F1200",
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

    assert_eq!(render_gcode(&document).expect("gcode"), expected);
    assert!(!serde_json::to_string(&document).expect("serialise").contains("islands"));
    assert_eq!(generate_toolpaths(&document).expect("toolpaths").operations[0].island_count, 0);
}

#[test]
fn island_output_is_deterministic_and_survives_a_reopen() {
    let directory = workspace("reopen");
    let path = directory.join("islands.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(pocket(vec![Island::new(40.0, 35.0, 20.0, 20.0)])).expect("island pocket");

    let first = session.toolpaths().expect("toolpaths");
    let second = session.toolpaths().expect("toolpaths");
    assert_eq!(first, second);

    let text = fs::read_to_string(&path).expect("read document");
    assert!(text.contains("\"islands\""));

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let third = reopened.toolpaths().expect("reopened toolpaths");
    assert_eq!(first, third);
    assert_eq!(first.operations[0].island_count, 1);
}
