use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Island, MoveKind, OpenCam, Operation, OperationKind, Severity, Stock, Tool, generate_toolpaths, render_gcode,
    validate::validate,
};

const RADIUS: f64 = 3.0;
const DIAMETER: f64 = 6.0;
const FEED: f64 = 1200.0;
const PLUNGE_FEED: f64 = 300.0;

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "t6".to_string(),
        number: 1,
        diameter: DIAMETER,
        flutes: 2,
        spindle: 12000.0,
        feed: FEED,
        plunge_feed: PLUNGE_FEED,
    }
}

fn pocket(width: f64, height: f64, stepover: f64) -> Operation {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = width;
    operation.height = height;
    operation.stepover = Some(stepover);
    operation.strategy = Some("spiral".to_string());
    operation
}

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-spiral-{}-{name}", process::id()));
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

#[test]
fn spiral_cut_points_stay_inside_the_pocket_and_clear_of_the_wall() {
    let operation = pocket(100.0, 70.0, 6.0);
    let report = generate_toolpaths(&document(operation.clone())).expect("valid spiral pocket");
    let path = &report.operations[0];
    assert_eq!(path.strategy.as_deref(), Some("spiral"));
    assert_eq!(path.loop_count, Some(6));

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
            let clearance = (point.x - operation.x).min(operation.x + operation.width - point.x);
            let clearance = clearance.min((point.y - operation.y).min(operation.y + operation.height - point.y));
            assert!(
                clearance >= RADIUS - 1e-9,
                "cut point ({}, {}) is {} mm from the pocket wall",
                point.x,
                point.y,
                clearance
            );
        }
    }
}

#[test]
fn spiral_rings_follow_the_stepover() {
    let operation = pocket(100.0, 70.0, 6.0);
    let report = generate_toolpaths(&document(operation.clone())).expect("valid spiral pocket");
    let path = &report.operations[0];

    let insets = [3.0, 9.0, 15.0, 21.0, 27.0, 33.0];
    for inset in insets {
        let x0 = operation.x + inset;
        let y0 = operation.y + inset;
        let x1 = operation.x + operation.width - inset;
        let y1 = operation.y + operation.height - inset;
        for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
            assert!(
                path.segments
                    .iter()
                    .any(|segment| segment.kind == MoveKind::Cut && close(segment.to.x, x) && close(segment.to.y, y)),
                "missing corner ({x}, {y}) of the ring inset {inset}"
            );
        }
    }
    let last = path.segments.iter().rev().find(|segment| segment.kind == MoveKind::Cut).expect("a cut");
    assert!(close(last.to.x, operation.x + 33.0) && close(last.to.y, operation.y + 33.0), "the innermost ring is missing");
    assert!(
        !path.segments.iter().any(|segment| segment.kind == MoveKind::Cut && close(segment.to.y, operation.y + 39.0)),
        "a ring was emitted after the core was already covered"
    );

    for inset in [3.0, 9.0, 15.0, 21.0, 27.0] {
        let from = cam_model::Point::new(operation.x + inset, operation.y + inset, 8.0);
        let to = cam_model::Point::new(operation.x + inset + 6.0, operation.y + inset + 6.0, 8.0);
        assert!(
            path.segments.iter().any(|segment| segment.kind == MoveKind::Cut
                && close(segment.from.x, from.x)
                && close(segment.from.y, from.y)
                && close(segment.to.x, to.x)
                && close(segment.to.y, to.y)
                && close(segment.feed, FEED)),
            "rings inset {inset} and {} are not joined by a cutting move",
            inset + 6.0
        );
    }
}

#[test]
fn spiral_stops_when_the_narrow_side_is_exhausted() {
    let report = generate_toolpaths(&document(pocket(100.0, 30.0, 6.0))).expect("valid spiral pocket");
    assert_eq!(report.operations[0].loop_count, Some(2));
}

#[test]
fn spiral_metrics_match_the_generated_moves() {
    let report = generate_toolpaths(&document(pocket(100.0, 70.0, 6.0))).expect("valid spiral pocket");
    let path = &report.operations[0];
    let cut: f64 = path.segments.iter().filter(|segment| segment.kind != MoveKind::Rapid).map(|segment| segment.length).sum();
    let rapid: f64 = path.segments.iter().filter(|segment| segment.kind == MoveKind::Rapid).map(|segment| segment.length).sum();
    assert!(close(cut, path.cut_length));
    assert!(close(rapid, path.rapid_length));
    assert!(path.cutting_seconds > 0.0);
}

#[test]
fn spiral_is_deterministic() {
    let first = generate_toolpaths(&document(pocket(100.0, 70.0, 6.0))).expect("valid");
    let second = generate_toolpaths(&document(pocket(100.0, 70.0, 6.0))).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));
}

#[test]
fn raster_output_is_unchanged_by_the_optional_strategy() {
    let mut legacy = pocket(40.0, 30.0, 3.0);
    legacy.strategy = None;

    let mut explicit = pocket(40.0, 30.0, 3.0);
    explicit.strategy = Some("raster".to_string());

    let legacy_report = generate_toolpaths(&document(legacy.clone())).expect("valid raster");
    let explicit_report = generate_toolpaths(&document(explicit.clone())).expect("valid raster");
    assert_eq!(legacy_report, explicit_report);
    assert_eq!(render_gcode(&document(legacy.clone())).expect("gcode"), render_gcode(&document(explicit)).expect("gcode"));

    let json = serde_json::to_string(&legacy_report).expect("serialise");
    assert!(!json.contains("\"strategy\""));
    assert!(!json.contains("loopCount"));
    assert!(!serde_json::to_string(&document(legacy)).expect("serialise").contains("strategy"));
}

#[test]
fn an_unknown_strategy_is_an_operation_error() {
    let mut operation = pocket(100.0, 70.0, 6.0);
    operation.strategy = Some("helical".to_string());
    assert_eq!(error_code(operation), "operation/invalid");
}

#[test]
fn spiral_rejects_islands() {
    let mut operation = pocket(100.0, 70.0, 6.0);
    operation.islands = vec![Island::new(40.0, 35.0, 20.0, 20.0)];
    assert_eq!(error_code(operation.clone()), "operation/strategy-island");

    let directory = workspace("island");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    let error = session.add_operation(operation).expect_err("spiral does not support islands");
    assert_eq!(error.code, "operation/strategy-island");
    assert_eq!(session.summary().expect("summary").operations, 0);
}

#[test]
fn spiral_survives_a_save_and_reopen() {
    let directory = workspace("reopen");
    let path = directory.join("spiral.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(pocket(100.0, 70.0, 6.0)).expect("spiral pocket");

    let first = session.toolpaths().expect("toolpaths");
    let text = fs::read_to_string(&path).expect("read document");
    assert!(text.contains("\"strategy\": \"spiral\""));

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let third = reopened.toolpaths().expect("reopened toolpaths");
    assert_eq!(first, third);
    assert_eq!(first.operations[0].loop_count, Some(6));
}

#[test]
fn spiral_gcode_keeps_the_cutting_feed_between_rings() {
    let document = document(pocket(100.0, 70.0, 6.0));
    let program = render_gcode(&document).expect("gcode");
    assert!(program.contains("(operation pocket - pocket)"));
    assert!(program.contains("G1 X13 Y13 Z8 F300"));
    assert!(program.contains("G1 X107 Y13 Z8 F1200"));
    assert!(program.contains("G1 X37 Y37 Z8 F1200"));
    assert!(program.contains("G1 X43 Y43 Z8 F1200"));
    assert_eq!(program.lines().filter(|line| line.starts_with("G0 X")).count(), 2);
    assert_eq!(program.matches("F300").count(), 1);
}
