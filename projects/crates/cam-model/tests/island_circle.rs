use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Island, MoveKind, OpenCam, Operation, OperationKind, Severity, Stock, Tool, export_dxf, generate_toolpaths,
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

fn circle_island(x: f64, y: f64, diameter: f64) -> Island {
    Island { x, y, width: diameter, height: diameter, shape: Some("circle".to_string()) }
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

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-island-circle-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

#[test]
fn a_valid_circle_island_passes_validation() {
    let report = validate(&document(pocket(vec![circle_island(40.0, 35.0, 20.0)])));
    assert!(report.ok, "issues: {:?}", report.issues);
}

#[test]
fn a_circle_island_is_cleared_around() {
    let operation = pocket(vec![circle_island(40.0, 35.0, 20.0)]);
    let document = document(operation);
    let report = generate_toolpaths(&document).expect("valid circle island pocket");
    let path = &report.operations[0];
    assert_eq!(path.island_count, 1);
    assert!(path.skipped_length > 0.0);
    assert!(path.skipped_segments > 0);

    let centre = (50.0, 45.0);
    let reach = 10.0 + RADIUS;
    for segment in &path.segments {
        if segment.kind == MoveKind::Rapid {
            continue;
        }
        let points = [
            segment.from,
            segment.to,
            cam_model::Point::new((segment.from.x + segment.to.x) / 2.0, (segment.from.y + segment.to.y) / 2.0, segment.from.z),
        ];
        for point in points {
            let distance = ((point.x - centre.0).powi(2) + (point.y - centre.1).powi(2)).sqrt();
            assert!(distance >= reach - 1e-9, "cut point ({}, {}) is {} mm from the island centre", point.x, point.y, distance);
        }
    }

    assert!(
        path.segments.iter().any(|segment| segment.kind == MoveKind::Rapid
            && (segment.from.y - segment.to.y).abs() < 1e-9
            && segment.from.x.min(segment.to.x) <= centre.0
            && segment.from.x.max(segment.to.x) >= centre.0),
        "a rapid must cross the island at the clearance plane"
    );
}

#[test]
fn circle_validation_reports_stable_codes() {
    let mut squashed = circle_island(40.0, 35.0, 20.0);
    squashed.height = 10.0;
    assert_eq!(error_code(pocket(vec![squashed])), "operation/island-invalid");

    let mut unknown = circle_island(40.0, 35.0, 20.0);
    unknown.shape = Some("hexagon".to_string());
    assert_eq!(error_code(pocket(vec![unknown])), "operation/island-invalid");

    assert_eq!(error_code(pocket(vec![circle_island(5.0, 35.0, 20.0)])), "operation/island-outside-pocket");
    assert_eq!(error_code(pocket(vec![circle_island(14.0, 35.0, 20.0)])), "operation/island-too-close");
    assert_eq!(
        error_code(pocket(vec![circle_island(40.0, 35.0, 20.0), circle_island(55.0, 35.0, 20.0)])),
        "operation/island-overlap"
    );
    assert_eq!(
        error_code(pocket(vec![Island::new(40.0, 35.0, 20.0, 20.0), circle_island(45.0, 40.0, 20.0)])),
        "operation/island-overlap"
    );
}

#[test]
fn touching_circle_islands_are_accepted() {
    let first = circle_island(40.0, 35.0, 20.0);
    let second = circle_island(66.0, 35.0, 20.0);
    let report = validate(&document(pocket(vec![first, second])));
    assert!(report.ok, "issues: {:?}", report.issues);
}

#[test]
fn circle_islands_survive_a_reopen_and_rectangles_stay_unshaped() {
    let directory = workspace("reopen");
    let path = directory.join("islands.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session
        .add_operation(pocket(vec![circle_island(40.0, 35.0, 20.0), Island::new(80.0, 55.0, 10.0, 10.0)]))
        .expect("island pocket");

    let text = fs::read_to_string(&path).expect("read document");
    assert!(text.contains("\"shape\": \"circle\""), "{text}");
    let stored: CamDocument = serde_json::from_str(&text).expect("parse document");
    let islands = &stored.operations[0].islands;
    assert_eq!(islands.len(), 2);
    assert_eq!(islands[1].shape, None, "a rectangle island serializes no shape");

    let first = session.toolpaths().expect("toolpaths");
    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let second = reopened.toolpaths().expect("reopened toolpaths");
    assert_eq!(first, second);
    assert_eq!(first.operations[0].island_count, 2);
}

#[test]
fn exports_circle_islands_as_circles() {
    let document = document(pocket(vec![circle_island(40.0, 35.0, 20.0)]));
    let text = export_dxf(&document);
    assert!(text.contains("0\nCIRCLE\n8\npocket\n10\n50\n20\n45\n40\n10\n"), "{text}");
}
