use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, MoveKind, OpenCam, Operation, OperationKind, Point, Severity, Stock, TabSettings, Tool, generate_toolpaths,
    render_gcode, validate::validate,
};

const RADIUS: f64 = 3.0;
const FEED: f64 = 1200.0;
/// Tool-centre radius of the 80 mm circular contour: `40 + 3`.
const PATH_RADIUS: f64 = 43.0;

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

/// The same circular contour with holding tabs.
fn circle_with_tabs(count: u32, width: f64, height: f64) -> Operation {
    let mut operation = circle();
    operation.tabs = Some(TabSettings::new(count, width, height));
    operation
}

/// Angular intervals `(entry, exit)` covered by every tab, counter-clockwise.
fn tab_intervals(count: u32, width: f64) -> Vec<(f64, f64)> {
    let share = std::f64::consts::TAU / f64::from(count);
    let half = width / (2.0 * PATH_RADIUS);
    (0..count)
        .map(|index| {
            let centre = (f64::from(index) + 0.5) * share;
            (centre - half, centre + half)
        })
        .collect()
}

/// Angle in `[0, 2·pi)` of one point seen from the part centre.
fn angle_from_centre(point: &cam_model::Point) -> f64 {
    let angle = (point.y - 50.0).atan2(point.x - 50.0);
    if angle < 0.0 { angle + std::f64::consts::TAU } else { angle }
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
fn three_tabs_emit_one_lift_and_plunge_group_per_level() {
    let report = generate_toolpaths(&document(circle_with_tabs(3, 12.0, 1.0))).expect("valid tabbed circular contour");
    let path = &report.operations[0];

    for (level, raised) in [(8.5_f64, 9.5_f64), (7.0, 8.0)] {
        let lifts: Vec<_> = path
            .segments
            .iter()
            .filter(|segment| {
                segment.kind == MoveKind::Rapid
                    && close(segment.from.z, level)
                    && close(segment.to.z, raised)
                    && close(segment.from.x, segment.to.x)
                    && close(segment.from.y, segment.to.y)
            })
            .collect();
        assert_eq!(lifts.len(), 3, "one vertical lift per tab at level {level}");

        let returns = path
            .segments
            .iter()
            .filter(|segment| segment.kind == MoveKind::Plunge && close(segment.from.z, raised) && close(segment.to.z, level))
            .count();
        assert_eq!(returns, 3, "one plunge back per tab at level {level}");
    }

    // One initial plunge into the level per depth pass.
    let entries =
        path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge && close(segment.from.z, 15.0)).count();
    assert_eq!(entries, 2);
}

#[test]
fn no_cutting_move_crosses_a_tab() {
    let report = generate_toolpaths(&document(circle_with_tabs(3, 12.0, 1.0))).expect("valid tabbed circular contour");
    let path = &report.operations[0];
    let intervals = tab_intervals(3, 12.0);

    for level in [8.5_f64, 7.0] {
        for segment in path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut && close(segment.to.z, level)) {
            let mid = Point::new((segment.from.x + segment.to.x) / 2.0, (segment.from.y + segment.to.y) / 2.0, level);
            let angle = angle_from_centre(&mid);
            let inside = intervals.iter().any(|(entry, exit)| angle > entry + 1e-6 && angle < exit - 1e-6);
            assert!(!inside, "cut move at level {level} crosses the tab at angle {angle}");
        }
    }
}

#[test]
fn every_circular_tab_point_stays_on_the_tool_centre_circle() {
    let report = generate_toolpaths(&document(circle_with_tabs(3, 12.0, 1.0))).expect("valid tabbed circular contour");
    let path = &report.operations[0];

    for segment in &path.segments {
        let distance = (segment.to.x - 50.0).hypot(segment.to.y - 50.0);
        assert!(close(distance, PATH_RADIUS), "point ({}, {}) is {distance} mm from the centre", segment.to.x, segment.to.y);
    }
}

#[test]
fn a_tabbed_circular_ring_closes_on_the_start_point() {
    let report = generate_toolpaths(&document(circle_with_tabs(3, 12.0, 1.0))).expect("valid tabbed circular contour");
    let path = &report.operations[0];

    for level in [8.5_f64, 7.0] {
        let last = path
            .segments
            .iter()
            .rev()
            .find(|segment| segment.kind == MoveKind::Cut && close(segment.to.z, level))
            .expect("a cutting move at the level");
        assert!(close(last.to.x, 50.0 + PATH_RADIUS) && close(last.to.y, 50.0), "level {level} must close at the start");
        assert!(close(last.feed, FEED));
    }
}

#[test]
fn a_tab_height_above_the_safe_plane_is_capped() {
    // At the 8.5 mm level `z + 7` would be 15.5 mm, above the 15 mm clearance
    // plane, so the lift stops at `safe_z`; the 7 mm level stays at 14 mm.
    let mut operation = circle_with_tabs(2, 20.0, 7.0);
    operation.depth = 3.0;
    assert!(validate(&document(operation.clone())).ok, "a tab taller than the cut is allowed on a circular contour");

    let report = generate_toolpaths(&document(operation)).expect("valid tabbed circular contour");
    let path = &report.operations[0];
    let intervals = tab_intervals(2, 20.0);
    let tab_entry = |segment: &cam_model::Move| {
        let angle = angle_from_centre(&segment.from);
        intervals.iter().any(|(entry, exit)| angle >= entry - 1e-9 && angle <= exit + 1e-9)
    };

    let capped = path
        .segments
        .iter()
        .filter(|segment| {
            segment.kind == MoveKind::Rapid && close(segment.from.z, 8.5) && close(segment.to.z, 15.0) && tab_entry(segment)
        })
        .count();
    assert_eq!(capped, 2, "both tabs cap at the clearance plane");
    let uncapped = path
        .segments
        .iter()
        .filter(|segment| {
            segment.kind == MoveKind::Rapid && close(segment.from.z, 7.0) && close(segment.to.z, 14.0) && tab_entry(segment)
        })
        .count();
    assert_eq!(uncapped, 2, "the deeper level lifts to its own tab height");
    let highest = path.segments.iter().flat_map(|segment| [segment.from.z, segment.to.z]).fold(f64::NEG_INFINITY, f64::max);
    assert!(close(highest, 15.0));
}

#[test]
fn a_single_tab_lifts_once_per_level() {
    let report = generate_toolpaths(&document(circle_with_tabs(1, 20.0, 1.0))).expect("valid single-tab circular contour");
    let path = &report.operations[0];

    for level in [8.5_f64, 7.0] {
        let lifts = path
            .segments
            .iter()
            .filter(|segment| {
                segment.kind == MoveKind::Rapid && close(segment.from.z, level) && close(segment.to.z, level + 1.0)
            })
            .count();
        assert_eq!(lifts, 1, "one lift for the single tab at level {level}");
    }
}

#[test]
fn circular_tab_footprint_must_fit_the_tool_centre_circumference() {
    // 4 × 70 = 280 mm is wider than the 2 × pi × 43 ≈ 270.2 mm circumference.
    let report = validate(&document(circle_with_tabs(4, 70.0, 1.0)));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("tool-centre circumference"))
    );
}

#[test]
fn circular_tab_count_width_and_height_must_be_positive() {
    for tabs in [
        TabSettings::new(0, 6.0, 1.0),
        TabSettings::new(4, 0.0, 1.0),
        TabSettings::new(4, -2.0, 1.0),
        TabSettings::new(4, 6.0, 0.0),
        TabSettings::new(4, 6.0, -1.0),
        TabSettings::new(4, f64::NAN, 1.0),
        TabSettings::new(4, 6.0, f64::INFINITY),
    ] {
        let mut operation = circle();
        operation.tabs = Some(tabs);
        let report = validate(&document(operation));
        assert!(!report.ok, "tabs {tabs:?} must be rejected");
        assert!(report.issues.iter().any(|issue| issue.code == "operation/invalid"));
    }
}

#[test]
fn a_circular_contour_without_tabs_is_unchanged() {
    let report = generate_toolpaths(&document(circle())).expect("valid circular contour");
    let path = &report.operations[0];

    // Two levels: 144 cuts, one initial plunge per level, one rapid to the
    // first start and one vertical retract per level.
    let cuts = path.segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count();
    let plunges = path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).count();
    let rapids = path.segments.iter().filter(|segment| segment.kind == MoveKind::Rapid).count();
    assert_eq!((cuts, plunges, rapids), (144, 2, 3));
    assert!(path.shape.as_deref() == Some("circle"));
    assert!(path.loop_count.is_none());
}

#[test]
fn ramp_and_tabs_keep_the_ramp_target_depth() {
    let mut operation = circle_with_tabs(3, 12.0, 1.0);
    operation.ramp = Some(10.0);
    let report = generate_toolpaths(&document(operation)).expect("valid ramped tabbed circular contour");
    let path = &report.operations[0];

    assert_eq!(path.segments[0].kind, MoveKind::Rapid);
    let entry = &path.segments[1];
    assert_eq!(entry.kind, MoveKind::Cut);
    assert!(close(entry.from.z, 15.0));
    assert!(entry.to.z < entry.from.z);

    for level in [8.5_f64, 7.0] {
        let lifts = path
            .segments
            .iter()
            .filter(|segment| {
                segment.kind == MoveKind::Rapid && close(segment.from.z, level) && close(segment.to.z, level + 1.0)
            })
            .count();
        assert_eq!(lifts, 3, "the ramp must not change the tab count at level {level}");
        assert!(
            path.segments
                .iter()
                .any(|segment| segment.kind == MoveKind::Cut && segment.to.z < segment.from.z && close(segment.to.z, level)),
            "no ramp descent ends exactly at level {level}"
        );
    }

    let lowest = path.segments.iter().flat_map(|segment| [segment.from.z, segment.to.z]).fold(f64::INFINITY, f64::min);
    assert!(close(lowest, 7.0), "the ramp cut {lowest} mm deep, below the last level");
}

#[test]
fn tabbed_circle_is_deterministic_and_round_trips() {
    let operation = circle_with_tabs(4, 12.0, 1.0);
    let first = generate_toolpaths(&document(operation.clone())).expect("valid");
    let second = generate_toolpaths(&document(operation.clone())).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));

    let program = render_gcode(&document(operation.clone())).expect("gcode");
    assert!(program.contains("(tabs 4 x 12 mm wide, 1 mm high)"));
    assert!(program.contains("(operation profile - contour)"));

    let directory = workspace("reopen-tabs");
    let path = directory.join("circle-tabs.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool()).expect("tool");
    session.add_operation(operation.clone()).expect("tabbed circular contour");

    let written = session.toolpaths().expect("toolpaths");
    let text = fs::read_to_string(&path).expect("read document");
    assert!(text.contains("\"shape\": \"circle\""));
    assert!(text.contains("\"tabs\""));

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let read_back = reopened.toolpaths().expect("reopened toolpaths");
    assert_eq!(written, read_back);
    assert_eq!(read_back.operations[0].shape.as_deref(), Some("circle"));
    assert_eq!(read_back, first);
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
