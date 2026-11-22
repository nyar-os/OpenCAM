use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, MoveKind, OpenCam, Operation, OperationKind, Severity, Stock, TabSettings, Tool, generate_toolpaths,
    render_gcode, validate::validate,
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

/// A circular contour centred at (50, 50) with a 40 mm part radius and 3 mm
/// levels (1.5 and 3.0 deep).
fn circle() -> Operation {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 3.0, 1.5);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 80.0;
    operation.height = 80.0;
    operation.shape = Some("circle".to_string());
    operation
}

fn rectangle() -> Operation {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 3.0, 1.5);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 20.0;
    operation
}

fn document(operation: Operation) -> CamDocument {
    document_with_stock(operation, Stock::new(120.0, 100.0, 10.0))
}

fn document_with_stock(operation: Operation, stock: Stock) -> CamDocument {
    let mut document = CamDocument::new(stock);
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-circle-contour-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

#[test]
fn circle_cut_points_follow_the_offset_circle() {
    let report = generate_toolpaths(&document(circle())).expect("valid circular contour");
    let path = &report.operations[0];
    let offset = 40.0 + RADIUS;

    for segment in &path.segments {
        if segment.kind == MoveKind::Rapid {
            continue;
        }
        let distance = (segment.to.x - 50.0).hypot(segment.to.y - 50.0);
        assert!(
            close(distance, offset),
            "cut point ({}, {}) is {distance} mm from the centre instead of {offset}",
            segment.to.x,
            segment.to.y
        );
        assert!(distance >= 40.0, "a cutting point entered the part at radius {distance}");
    }
}

#[test]
fn every_level_cuts_one_closed_seventy_two_segment_ring() {
    let report = generate_toolpaths(&document(circle())).expect("valid circular contour");
    let path = &report.operations[0];
    let offset = 40.0 + RADIUS;

    for level in [8.5_f64, 7.0] {
        let cuts: Vec<_> =
            path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut && close(segment.to.z, level)).collect();
        assert_eq!(cuts.len(), 72, "level {level} must cut 72 segments");

        let first = cuts[0];
        assert!(close(first.from.x, 50.0 + offset) && close(first.from.y, 50.0));
        assert!(first.to.y > 50.0 && first.to.x < 50.0 + offset, "the ring must run counter-clockwise from +X");

        let last = cuts[cuts.len() - 1];
        assert!(close(last.to.x, 50.0 + offset) && close(last.to.y, 50.0), "the ring must close at the cutting feed");
        assert!(close(last.feed, FEED));
    }

    assert!(path.segments.iter().any(|segment| segment.kind == MoveKind::Plunge && close(segment.to.z, 8.5)));
    assert!(path.segments.iter().any(|segment| segment.kind == MoveKind::Rapid && close(segment.to.z, 15.0)));
    assert_eq!(path.shape.as_deref(), Some("circle"));
    assert!(path.strategy.is_none());
    assert!(path.loop_count.is_none());
}

#[test]
fn circle_contour_gcode_uses_linear_moves_only() {
    let program = render_gcode(&document(circle())).expect("gcode");
    assert!(program.contains("(operation profile - contour)"));
    assert!(program.contains("G1 X93 Y50 Z8.5 F300"));
    assert!(program.contains("G1 X93 Y50 Z8.5 F1200"));
    assert!(!program.contains("G2 "), "a circular contour must not emit arcs");
    assert!(!program.contains("G3 "), "a circular contour must not emit arcs");
    assert!(program.lines().all(|line| !line.starts_with("G2 ") && !line.starts_with("G3 ")));
}

#[test]
fn rectangular_contour_is_byte_identical_without_shape() {
    let implicit = document(rectangle());
    let mut explicit_operation = rectangle();
    explicit_operation.shape = Some("rect".to_string());
    let explicit = document(explicit_operation);

    let implicit_report = generate_toolpaths(&implicit).expect("valid contour");
    let explicit_report = generate_toolpaths(&explicit).expect("valid contour");
    assert_eq!(implicit_report, explicit_report);
    assert_eq!(render_gcode(&implicit).expect("gcode"), render_gcode(&explicit).expect("gcode"));
    assert!(implicit_report.operations[0].shape.is_none());
    assert!(!serde_json::to_string(&implicit_report).expect("serialise").contains("\"shape\""));
}

#[test]
fn a_circular_contour_rejects_tabs() {
    let mut operation = circle();
    operation.tabs = Some(TabSettings::new(4, 6.0, 1.0));
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| {
        issue.code == "operation/invalid" && issue.message.contains("circular contour does not support them")
    }));
}

#[test]
fn a_circle_size_mismatch_reports_circle_size() {
    let mut operation = circle();
    operation.height = 60.0;
    assert_eq!(error_code(operation), "operation/circle-size");

    let mut zero = circle();
    zero.width = 0.0;
    zero.height = 0.0;
    assert_eq!(error_code(zero), "operation/invalid");

    let mut unknown = circle();
    unknown.shape = Some("hex".to_string());
    assert_eq!(error_code(unknown), "operation/invalid");
}

#[test]
fn ramp_entry_ends_at_the_level_depth_without_overcut() {
    let mut operation = circle();
    operation.ramp = Some(10.0);
    let report = generate_toolpaths(&document(operation)).expect("valid ramped circular contour");
    let path = &report.operations[0];

    assert_eq!(path.segments[0].kind, MoveKind::Rapid);
    let entry = &path.segments[1];
    assert_eq!(entry.kind, MoveKind::Cut);
    assert!(close(entry.from.z, 15.0));
    assert!(entry.to.z < entry.from.z);

    for level in [8.5_f64, 7.0] {
        assert!(
            path.segments
                .iter()
                .any(|segment| segment.kind == MoveKind::Cut && segment.to.z < segment.from.z && close(segment.to.z, level)),
            "no ramp descent ends exactly at level {level}"
        );
    }

    let lowest = path.segments.iter().flat_map(|segment| [segment.from.z, segment.to.z]).fold(f64::INFINITY, f64::min);
    assert!(close(lowest, 7.0), "the ramp cut {lowest} mm deep, below the last level");

    let ring = path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut && close(segment.to.z, 7.0)).count();
    assert!(ring >= 72, "the last level must still cut the full ring, got {ring} moves");
}

#[test]
fn a_circular_path_leaving_the_stock_is_reported() {
    let mut part = circle();
    part.x = 50.0;
    let report = validate(&document(part));
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "path/outside-stock" && issue.severity == Severity::Error));

    let mut offset = circle();
    offset.x = 0.0;
    offset.y = 0.0;
    let report = validate(&document_with_stock(offset, Stock::new(80.0, 80.0, 10.0)));
    assert!(report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "path/outside-stock" && issue.severity == Severity::Warning));
}

#[test]
fn circle_contour_is_deterministic_and_round_trips() {
    let first = generate_toolpaths(&document(circle())).expect("valid");
    let second = generate_toolpaths(&document(circle())).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));

    let directory = workspace("reopen");
    let path = directory.join("circle-contour.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(circle()).expect("circular contour");

    let written = session.toolpaths().expect("toolpaths");
    let text = fs::read_to_string(&path).expect("read document");
    assert!(text.contains("\"shape\": \"circle\""));

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let read_back = reopened.toolpaths().expect("reopened toolpaths");
    assert_eq!(written, read_back);
    assert_eq!(read_back.operations[0].shape.as_deref(), Some("circle"));
}
