use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Island, MoveKind, OpenCam, Operation, OperationKind, Severity, Stock, Tool, generate_toolpaths, render_gcode,
    validate::validate,
};

const RADIUS: f64 = 3.0;
const FEED: f64 = 1200.0;

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "t6".to_string(),
        number: 1,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: FEED,
        plunge_feed: 300.0,
    }
}

/// A circular pocket centred at (50, 50) with a 40 mm radius.
fn circle(stepover: f64) -> Operation {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 80.0;
    operation.height = 80.0;
    operation.stepover = Some(stepover);
    operation.shape = Some("circle".to_string());
    operation
}

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

fn circle_with_height(height: f64) -> Operation {
    let mut operation = circle(6.0);
    operation.height = height;
    operation
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-circle-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn has_cut_to(operation: &cam_model::OperationPath, x: f64, y: f64) -> bool {
    operation.segments.iter().any(|segment| segment.kind == MoveKind::Cut && close(segment.to.x, x) && close(segment.to.y, y))
}

#[test]
fn circle_cut_points_stay_inside_the_pocket_and_clear_of_the_wall() {
    let operation = circle(6.0);
    let report = generate_toolpaths(&document(operation.clone())).expect("valid circular pocket");
    let centre = operation.x + operation.width / 2.0;
    let wall_clearance = operation.width / 2.0 - RADIUS;

    for segment in &report.operations[0].segments {
        if segment.kind == MoveKind::Rapid {
            continue;
        }
        let points = [
            segment.from,
            segment.to,
            cam_model::Point::new((segment.from.x + segment.to.x) / 2.0, (segment.from.y + segment.to.y) / 2.0, segment.from.z),
        ];
        for point in points {
            let distance = ((point.x - centre).powi(2) + (point.y - centre).powi(2)).sqrt();
            assert!(
                distance <= wall_clearance + 1e-9,
                "cut point ({}, {}) is {} mm from the centre, beyond the {} mm ring",
                point.x,
                point.y,
                distance,
                wall_clearance
            );
        }
    }
}

#[test]
fn circle_rings_follow_the_stepover() {
    let operation = circle(6.0);
    let path = &generate_toolpaths(&document(operation.clone())).expect("valid circular pocket").operations[0];
    assert_eq!(path.shape.as_deref(), Some("circle"));
    assert!(path.strategy.is_none());
    assert_eq!(path.loop_count, Some(7));

    let centre = operation.x + operation.width / 2.0;
    let radii = [37.0, 31.0, 25.0, 19.0, 13.0, 7.0, 1.0];
    for radius in radii {
        assert!(has_cut_to(path, centre + radius, centre), "missing the ring start at radius {radius}");
    }
    for pair in radii.windows(2) {
        let (outer, inner) = (pair[0], pair[1]);
        assert!(close(outer - inner, 6.0), "ring radii {outer} and {inner} are not one stepover apart");
        assert!(
            path.segments.iter().any(|segment| segment.kind == MoveKind::Cut
                && close(segment.from.x, centre + outer)
                && close(segment.from.y, centre)
                && close(segment.to.x, centre + inner)
                && close(segment.to.y, centre)
                && close(segment.feed, FEED)),
            "rings {outer} and {inner} are not joined by a cutting move"
        );
    }
    assert!(
        !path.segments.iter().any(|segment| segment.kind == MoveKind::Cut && segment.to.x > centre + 37.0 + 1e-9),
        "a ring was emitted outside the first ring"
    );

    let last = path.segments.iter().rev().find(|segment| segment.kind == MoveKind::Cut).expect("a cut");
    assert!(close(last.to.x, centre) && close(last.to.y, centre), "the centre is not cleared");
}

#[test]
fn circle_metrics_match_the_generated_moves() {
    let report = generate_toolpaths(&document(circle(6.0))).expect("valid circular pocket");
    let path = &report.operations[0];
    let cut: f64 = path.segments.iter().filter(|segment| segment.kind != MoveKind::Rapid).map(|segment| segment.length).sum();
    let rapid: f64 = path.segments.iter().filter(|segment| segment.kind == MoveKind::Rapid).map(|segment| segment.length).sum();
    assert!(close(cut, path.cut_length));
    assert!(close(rapid, path.rapid_length));
    assert!(path.cut_length > 0.0);
    assert!(path.cutting_seconds > 0.0);
}

#[test]
fn circle_output_is_deterministic() {
    let first = generate_toolpaths(&document(circle(6.0))).expect("valid");
    let second = generate_toolpaths(&document(circle(6.0))).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));
}

#[test]
fn rect_output_is_unchanged_by_an_explicit_shape() {
    let mut legacy = circle(6.0);
    legacy.shape = None;
    legacy.width = 40.0;
    legacy.height = 30.0;
    legacy.x = 10.0;
    legacy.y = 10.0;

    let mut explicit = legacy.clone();
    explicit.shape = Some("rect".to_string());

    let legacy_document = document(legacy.clone());
    let explicit_document = document(explicit.clone());
    let legacy_report = generate_toolpaths(&legacy_document).expect("valid raster");
    let explicit_report = generate_toolpaths(&explicit_document).expect("valid raster");
    assert_eq!(legacy_report, explicit_report);
    assert_eq!(render_gcode(&legacy_document).expect("gcode"), render_gcode(&explicit_document).expect("gcode"));
    assert!(legacy_report.operations[0].shape.is_none());

    let json = serde_json::to_string(&legacy_report).expect("serialise");
    assert!(!json.contains("\"shape\""));
    assert!(!json.contains("loopCount"));
    assert!(!serde_json::to_string(&legacy).expect("serialise").contains("shape"));
}

#[test]
fn circle_validation_reports_stable_codes() {
    let mut unknown = circle(6.0);
    unknown.shape = Some("hex".to_string());
    assert_eq!(error_code(unknown), "operation/invalid");

    let mut sized = circle(6.0);
    sized.height = 60.0;
    assert_eq!(error_code(sized), "operation/circle-size");

    let mut strategy = circle(6.0);
    strategy.strategy = Some("raster".to_string());
    assert_eq!(error_code(strategy), "operation/strategy-circle");

    let mut spiral = circle(6.0);
    spiral.strategy = Some("spiral".to_string());
    assert_eq!(error_code(spiral), "operation/strategy-circle");

    let mut islands = circle(6.0);
    islands.islands = vec![Island::new(40.0, 35.0, 20.0, 20.0)];
    assert_eq!(error_code(islands), "operation/strategy-island");
}

#[test]
fn a_rejected_circle_is_not_written() {
    let directory = workspace("rejected");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");

    let error = session.add_operation(circle_with_height(60.0)).expect_err("circle size mismatch");
    assert_eq!(error.code, "operation/circle-size");
    assert_eq!(session.summary().expect("summary").operations, 0);

    let text = fs::read_to_string(&path).expect("read document");
    let stored: CamDocument = serde_json::from_str(&text).expect("parse document");
    assert!(stored.operations.is_empty());
}

#[test]
fn circle_survives_a_save_and_reopen() {
    let directory = workspace("reopen");
    let path = directory.join("circle.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(circle(6.0)).expect("circle pocket");

    let first = session.toolpaths().expect("toolpaths");
    let text = fs::read_to_string(&path).expect("read document");
    assert!(text.contains("\"shape\": \"circle\""));

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let third = reopened.toolpaths().expect("reopened toolpaths");
    assert_eq!(first, third);
    assert_eq!(first.operations[0].shape.as_deref(), Some("circle"));
    assert_eq!(first.operations[0].loop_count, Some(7));
}

#[test]
fn circle_gcode_keeps_the_cutting_feed_between_rings() {
    let document = document(circle(6.0));
    let program = render_gcode(&document).expect("gcode");
    assert!(program.contains("(operation pocket - pocket)"));
    assert!(program.contains("G0 X87 Y50 Z15"));
    assert!(program.contains("G1 X87 Y50 Z8 F300"));
    assert!(program.contains("G1 X81 Y50 Z8 F1200"));
    assert!(program.contains("G1 X50 Y50 Z8 F1200"));
    assert_eq!(program.lines().filter(|line| line.starts_with("G0 X")).count(), 2);
    assert_eq!(program.matches("F300").count(), 1);
}
