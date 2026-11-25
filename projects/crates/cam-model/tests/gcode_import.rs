use std::{fs, path::PathBuf};

use cam_model::{
    CamDocument, DEFAULT_STOCK, GcodeImportRequest, Hole, OpenCam, Operation, OperationKind, PathPoint, Stock, Tool,
    import_gcode, render_gcode, validate::validate,
};

fn temp_dir(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-gcode-import-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("temp directory");
    directory
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

fn points(operation: &cam_model::Operation) -> Vec<(f64, f64)> {
    operation.path.iter().map(|point| (point.x, point.y)).collect()
}

#[test]
fn imports_linear_runs_between_rapids() {
    let program = "\
%
G21 G90 G17 G94
(OpenCAM test)
G0 Z5
G0 X0 Y0 Z5
G1 X10 Y0 Z-1 F100
G1 X10 Y5 F100
G0 Z5
G0 X2 Y2 Z5
G1 X8 Y2 Z-1 F100
G1 X8 Y4 F100
G0 Z5
M30
%
";
    let import = import_gcode(program, &GcodeImportRequest::default()).expect("valid program");
    assert_eq!(import.move_count, 4);
    assert_eq!(import.arc_count, 0);
    assert_eq!(import.skipped_count, 0);
    assert!(import.losses.is_empty());
    assert_eq!(import.document.operations.len(), 2);
    assert_eq!(import.document.tools.len(), 1);
    assert_eq!(import.document.tools[0].id, "engrave");
    assert_eq!(import.document.operations[0].id, "gcode-1");
    assert_eq!(import.document.operations[0].name, "gcode-1");
    assert_eq!(import.document.operations[0].kind, OperationKind::Engrave);
    assert_eq!(points(&import.document.operations[0]), vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)]);
    assert_eq!(points(&import.document.operations[1]), vec![(2.0, 2.0), (8.0, 2.0), (8.0, 4.0)]);
    assert_eq!(import.document.stock, Stock::new(10.0, 5.0, 10.0));
    assert_eq!(import.document.revision, 0);
    assert!(validate(&import.document).ok);
}

#[test]
fn plunge_and_feed_only_lines_do_not_split_a_run() {
    let program = "\
G21 G90
G0 X0 Y0 Z5
G1 Z-1 F300
G1 X10 F100
G1 F150
G1 X20 Y0
G0 Z5
M30
";
    let import = import_gcode(program, &GcodeImportRequest::default()).expect("valid program");
    assert_eq!(import.document.operations.len(), 1);
    assert_eq!(points(&import.document.operations[0]), vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    assert_eq!(import.move_count, 3);
}

#[test]
fn bare_axis_words_continue_the_last_motion_mode() {
    let program = "\
G21 G90
G0 X0 Y0
G1 X10
X20
G0 Z5
M30
";
    let import = import_gcode(program, &GcodeImportRequest::default()).expect("valid program");
    assert_eq!(import.document.operations.len(), 1);
    assert_eq!(points(&import.document.operations[0]), vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
}

#[test]
fn flattens_arcs_within_tolerance_with_exact_endpoints() {
    let request = GcodeImportRequest::default();
    let program = "\
G21 G90
G0 Z5
G0 X10 Y10 Z5
G1 Z-1 F300
G2 X20 Y10 I5 J0 F100
G0 Z5
M30
";
    let import = import_gcode(program, &request).expect("valid program");
    assert_eq!(import.move_count, 2);
    assert_eq!(import.arc_count, 1);
    assert_eq!(import.document.operations.len(), 1);
    let path = &import.document.operations[0].path;
    // Half circle of radius 5 mm at 0.05 mm sagitta needs 12 chords: the
    // plunge start, eleven interior points and the exact end. The bounding
    // box minimum (10, 10) is translated to the origin.
    assert_eq!(path.len(), 13);
    assert_close(path[0].x, 0.0);
    assert_close(path[0].y, 0.0);
    assert_close(path[12].x, 10.0);
    assert_close(path[12].y, 0.0);
    for pair in path.windows(2) {
        let chord = ((pair[1].x - pair[0].x).powi(2) + (pair[1].y - pair[0].y).powi(2)).sqrt();
        let sagitta = 5.0 - (25.0 - (chord / 2.0).powi(2)).sqrt();
        assert!(sagitta <= request.tolerance + 1e-9, "sagitta {sagitta} exceeds the tolerance");
    }
}

#[test]
fn full_circle_arc_closes_on_the_start_point() {
    let program = "\
G21 G90
G0 X10 Y10
G1 Z-1 F300
G3 I-5 J0 F100
G0 Z5
M30
";
    let import = import_gcode(program, &GcodeImportRequest::default()).expect("valid program");
    assert_eq!(import.arc_count, 1);
    let path = &import.document.operations[0].path;
    assert!(path.len() >= 4, "a full circle needs at least three chords");
    let first = path.first().expect("start");
    let last = path.last().expect("end");
    assert_eq!((first.x, first.y), (last.x, last.y), "the circle closes on its start");
}

#[test]
fn reports_cycles_radius_arcs_and_unknown_words_as_losses() {
    let program = "\
G21 G90
G0 Z5
G81 X5 Y5 Z-4 R5 F300
G80
G2 X10 Y0 R5
M98 P1000
G54
G0 X0 Y0
G1 X10 Y0 F100
G1 X10 Y5
G0 Z5
M30
";
    let import = import_gcode(program, &GcodeImportRequest::default()).expect("valid program");
    let codes: Vec<_> = import.losses.iter().map(|loss| loss.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "import/gcode-unsupported-gcode",
            "import/gcode-unsupported-arc",
            "import/gcode-unsupported-mcode",
            "import/gcode-unsupported-gcode"
        ]
    );
    assert_eq!(import.losses[0].word, "G81");
    assert_eq!(import.losses[0].line, 3);
    assert_eq!(import.losses[2].word, "M98");
    assert_eq!(import.skipped_count, 4);
    // The cycle and the skipped lines break the modal state, the final run
    // is the only cutting geometry.
    assert_eq!(import.document.operations.len(), 1);
    assert_eq!(points(&import.document.operations[0]), vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)]);
}

#[test]
fn degenerate_run_is_reported_as_a_loss() {
    let program = "\
G21 G90
G0 X5 Y5
G1 Z-1 F300
G0 Z5
M30
";
    let import = import_gcode(program, &GcodeImportRequest::default()).expect("valid program");
    assert_eq!(import.skipped_count, 1);
    assert_eq!(import.losses[0].code, "import/gcode-degenerate-run");
    assert_eq!(import.losses[0].word, "G1");
    assert_eq!(import.losses[0].line, 3);
    assert!(import.document.operations.is_empty());
}

#[test]
fn rejects_inch_and_incremental_modes_with_a_line_number() {
    for word in ["G20", "G91", "G92"] {
        let program = format!("G21 G90\n{word}\nG0 X0 Y0\n");
        let error = import_gcode(&program, &GcodeImportRequest::default()).expect_err("mode must fail");
        assert_eq!(error.code, "import/gcode-unsupported-mode", "{word}");
        assert!(error.message.contains("line 2"), "{}", error.message);
    }
}

#[test]
fn rejects_malformed_blocks_with_a_line_number() {
    let cases = [
        ("G21 G90\nG1 X1.2.3\n", "not a number"),
        ("G21 G90\nG2 I5 J0\n", "known start"),
        ("G21 G90\nG0 X0 Y0\nG2 X10\n", "I` and `J"),
        ("G21 G90\nX10 Y10\n", "before any motion code"),
        ("G21 G90\nG0 X0 Y0\nG1 G2 X5 Y5\n", "more than one motion"),
        ("G21 G90\nG1 X10 (open\n", "closing"),
    ];
    for (program, needle) in cases {
        let error = import_gcode(program, &GcodeImportRequest::default()).expect_err("block must fail");
        assert_eq!(error.code, "import/gcode-malformed-line", "{program}");
        assert!(error.message.contains(needle), "{}", error.message);
    }
}

#[test]
fn empty_program_yields_the_default_document() {
    let import = import_gcode("%\nG21 G90 G17 G94\nM30\n%\n", &GcodeImportRequest::default()).expect("valid program");
    assert_eq!(import.document.operations.len(), 0);
    assert_eq!(import.document.tools.len(), 0);
    assert_eq!(import.document.stock, DEFAULT_STOCK);
    assert_eq!(import.document.revision, 0);
    assert!(validate(&import.document).ok);
}

#[test]
fn invalid_request_is_rejected() {
    let error = import_gcode("G1 X0 Y0\n", &GcodeImportRequest::new(0.0, 1.0, 0.05)).expect_err("depth must be positive");
    assert_eq!(error.code, "import/invalid");
    let error = import_gcode("G1 X0 Y0\n", &GcodeImportRequest::new(1.0, 1.0, -0.1)).expect_err("tolerance must be positive");
    assert_eq!(error.code, "import/invalid");
}

#[test]
fn import_is_deterministic() {
    let program = "\
%
G21 G90 G17 G94
G0 Z5
G0 X0 Y0 Z5
G1 X10 Y0 Z-1 F100
G3 X20 Y0 I5 J0 F100
G81 X5 Y5 Z-4 R5 F300
G0 Z5
M30
%
";
    let first = import_gcode(program, &GcodeImportRequest::default()).expect("first import");
    let second = import_gcode(program, &GcodeImportRequest::default()).expect("second import");
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_string_pretty(&first.document).expect("serialize"),
        serde_json::to_string_pretty(&second.document).expect("serialize")
    );
}

#[test]
fn round_trips_the_exported_move_subset() {
    let mut document = CamDocument::new(Stock::new(20.0, 10.0, 5.0));
    document.tools.push(Tool {
        id: "t2".to_string(),
        name: "2 mm flat".to_string(),
        number: 1,
        diameter: 2.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1000.0,
        plunge_feed: 300.0,
    });
    let mut engrave = Operation::new("logo", OperationKind::Engrave, "t2", 2.0, 1.0);
    engrave.path =
        vec![PathPoint::new(2.0, 7.0), PathPoint::new(12.0, 2.0), PathPoint::new(12.0, 7.0), PathPoint::new(2.0, 2.0)];
    document.operations.push(engrave);
    let mut drill = Operation::new("holes", OperationKind::Drill, "t2", 4.0, 4.0);
    drill.holes = vec![Hole::new(15.0, 3.0), Hole::new(18.0, 3.0)];
    document.operations.push(drill);

    let program = render_gcode(&document).expect("gcode");
    let import = import_gcode(&program, &GcodeImportRequest::new(2.0, 2.0, 0.05)).expect("import");

    // Two stepdown levels of the engrave rectangle come back as two runs;
    // the two drill holes come back as cycle losses, one per hole.
    assert_eq!(import.document.operations.len(), 2);
    assert_eq!(import.skipped_count, 2);
    assert!(import.losses.iter().all(|loss| loss.code == "import/gcode-unsupported-gcode" && loss.word == "G81"));
    for operation in &import.document.operations {
        assert_eq!(operation.kind, OperationKind::Engrave);
        assert_eq!(operation.depth, 2.0);
        assert_eq!(operation.stepdown, 2.0);
        assert_eq!(points(operation), vec![(0.0, 5.0), (10.0, 0.0), (10.0, 5.0), (0.0, 0.0)]);
    }
    assert_eq!(import.document.stock, Stock::new(10.0, 5.0, 10.0));
    assert!(validate(&import.document).ok);

    // Re-importing the re-rendered program is stable.
    let again = import_gcode(&render_gcode(&import.document).expect("gcode"), &GcodeImportRequest::new(2.0, 2.0, 0.05))
        .expect("second import");
    assert_eq!(again.document, import.document);
}

#[test]
fn session_writes_a_new_document_and_refuses_existing_targets() {
    let directory = temp_dir("session");
    let source = directory.join("part.nc");
    let target = directory.join("part.ocam");
    fs::write(&source, "G21 G90\nG0 Z5\nG0 X0 Y0\nG1 X10 Y0 Z-1 F100\nG1 X10 Y5\nG0 Z5\nM30\n").expect("write program");
    let mut session = OpenCam::new();
    let report = session.import_gcode(&source, &target, &GcodeImportRequest::default()).expect("import");
    assert_eq!(report.format, "open-cam-gcode-import");
    assert_eq!(report.version, 1);
    assert_eq!(report.units, "mm");
    assert_eq!(report.operation_count, 1);
    assert_eq!(report.move_count, 2);
    assert_eq!(report.skipped_count, 0);
    assert_eq!(report.document.operations, 1);
    assert!(target.exists());

    let error = session.import_gcode(&source, &target, &GcodeImportRequest::default()).expect_err("target exists");
    assert_eq!(error.code, "file/exists");

    let error = session
        .import_gcode(&directory.join("missing.nc"), &directory.join("other.ocam"), &GcodeImportRequest::default())
        .expect_err("source missing");
    assert_eq!(error.code, "io/read");
}
