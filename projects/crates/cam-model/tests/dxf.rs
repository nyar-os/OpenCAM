use std::{fs, path::PathBuf};

use cam_model::{CamDocument, DxfImportRequest, OpenCam, OperationKind, Stock, import_dxf, validate::validate};

fn fixture() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dxf/parts.dxf");
    fs::read_to_string(path).expect("fixture")
}

fn temp_dir(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-dxf-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("temp directory");
    directory
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn imports_line_with_layer_and_normalised_stock() {
    let import = import_dxf(&fixture(), &DxfImportRequest::default()).expect("valid drawing");
    assert_eq!(import.entity_count, 4);
    assert_eq!(import.document.stock, Stock::new(100.0, 50.0, 55.0));
    assert_eq!(import.document.tools.len(), 1);
    assert_eq!(import.document.tools[0].id, "engrave");
    assert_eq!(import.document.revision, 0);

    let line = &import.document.operations[0];
    assert_eq!(line.kind, OperationKind::Engrave);
    assert_eq!(line.id, "outline-1");
    assert_eq!(line.name, "outline");
    assert_eq!(line.tool, "engrave");
    assert_eq!(line.path.len(), 2);
    assert_close(line.path[0].x, 0.0);
    assert_close(line.path[0].y, 10.0);
    assert_close(line.path[1].x, 100.0);
    assert_close(line.path[1].y, 10.0);
    assert!(!line.closed);
}

#[test]
fn imports_closed_lwpolyline() {
    let import = import_dxf(&fixture(), &DxfImportRequest::default()).expect("valid drawing");
    let polyline = &import.document.operations[1];
    assert_eq!(polyline.id, "outline-2");
    assert!(polyline.closed);
    let points: Vec<_> = polyline.path.iter().map(|point| (point.x, point.y)).collect();
    assert_eq!(points.len(), 3);
    assert_close(points[0].0, 0.0);
    assert_close(points[0].1, 0.0);
    assert_close(points[1].0, 100.0);
    assert_close(points[1].1, 0.0);
    assert_close(points[2].0, 100.0);
    assert_close(points[2].1, 50.0);
}

#[test]
fn flattens_arc_within_tolerance_and_exact_endpoints() {
    let request = DxfImportRequest::default();
    let import = import_dxf(&fixture(), &request).expect("valid drawing");
    let arc = &import.document.operations[2];
    assert_eq!(arc.id, "curves-3");
    assert!(!arc.closed);
    assert_eq!(arc.path.len(), 17, "half circle of radius 10 mm at 0.05 mm tolerance has 16 chords");

    let first = arc.path.first().expect("start");
    assert_close(first.x, 50.0);
    assert_close(first.y, 40.0);
    let last = arc.path.last().expect("end");
    assert_close(last.x, 30.0);
    assert_close(last.y, 40.0);

    let radius: f64 = 10.0;
    for pair in arc.path.windows(2) {
        let chord = ((pair[1].x - pair[0].x).powi(2) + (pair[1].y - pair[0].y).powi(2)).sqrt();
        let sagitta = radius - (radius.powi(2) - (chord / 2.0).powi(2)).sqrt();
        assert!(sagitta <= request.tolerance + 1e-9, "sagitta {sagitta} exceeds the tolerance");
    }
}

#[test]
fn a_tighter_tolerance_adds_chords_but_keeps_the_endpoints() {
    let request = DxfImportRequest { tolerance: 0.001, ..DxfImportRequest::default() };
    let import = import_dxf(&fixture(), &request).expect("valid drawing");
    let arc = &import.document.operations[2];
    assert!(arc.path.len() > 17);
    assert_close(arc.path.first().expect("start").x, 50.0);
    assert_close(arc.path.last().expect("end").x, 30.0);
}

#[test]
fn flattens_circle_closed_and_without_a_duplicated_endpoint() {
    let request = DxfImportRequest::default();
    let import = import_dxf(&fixture(), &request).expect("valid drawing");
    let circle = &import.document.operations[3];
    assert_eq!(circle.id, "curves-4");
    assert!(circle.closed);
    assert_eq!(circle.path.len(), 23);
    let first = circle.path.first().expect("start");
    assert_close(first.x, 75.0);
    assert_close(first.y, 20.0);
    let last = circle.path.last().expect("last");
    assert!((last.x - 75.0).abs() > 1.0 || (last.y - 20.0).abs() > 1.0);
    for point in &circle.path {
        assert_close((point.x - 70.0).powi(2) + (point.y - 20.0).powi(2), 25.0);
    }
}

#[test]
fn imports_polyline_vertices_until_seqend() {
    let text = "\
0
SECTION
2
ENTITIES
0
POLYLINE
8
frame
70
1
0
VERTEX
8
frame
10
0
20
0
0
VERTEX
8
frame
10
20
20
0
0
VERTEX
8
frame
10
20
20
10
0
SEQEND
0
ENDSEC
0
EOF
";
    let import = import_dxf(text, &DxfImportRequest::default()).expect("valid polyline");
    assert_eq!(import.entity_count, 1);
    assert_eq!(import.skipped_count, 0);
    let operation = &import.document.operations[0];
    assert_eq!(operation.name, "frame");
    assert!(operation.closed);
    assert_eq!(operation.path.len(), 3);
    assert_eq!(import.document.stock, Stock::new(20.0, 10.0, 15.0));
}

#[test]
fn reports_unsupported_entities_and_sections_as_losses() {
    let import = import_dxf(&fixture(), &DxfImportRequest::default()).expect("valid drawing");
    assert_eq!(import.skipped_count, 1);
    assert_eq!(import.losses.len(), 2);

    let entity = import.losses.iter().find(|loss| loss.code == "import/dxf-unsupported-entity").expect("entity loss");
    assert_eq!(entity.entity, "SPLINE");
    assert_eq!(entity.layer, "ignored");

    let section = import.losses.iter().find(|loss| loss.code == "import/dxf-unsupported-section").expect("section loss");
    assert!(section.message.contains("OBJECTS"));
}

#[test]
fn a_bulge_is_reported_and_read_as_a_straight_segment() {
    let text = "\
0
SECTION
2
ENTITIES
0
LWPOLYLINE
8
arcs
90
2
10
0
20
0
42
0.5
10
10
20
10
0
ENDSEC
0
EOF
";
    let import = import_dxf(text, &DxfImportRequest::default()).expect("bulge loss");
    assert_eq!(import.entity_count, 1);
    assert_eq!(import.document.operations[0].path.len(), 2);
    assert!(import.losses.iter().any(|loss| loss.code == "import/dxf-bulge-ignored"));
}

#[test]
fn rejects_a_malformed_group_code_with_its_line() {
    let error = import_dxf("0\nSECTION\n2\nENTITIES\nabc\n10\n", &DxfImportRequest::default()).expect_err("bad code");
    assert_eq!(error.code, "import/dxf-malformed-group");
    assert!(error.message.contains("line 5"), "{}", error.message);
}

#[test]
fn rejects_a_truncated_group_with_its_line() {
    let error = import_dxf("0\nSECTION\n2", &DxfImportRequest::default()).expect_err("missing value");
    assert_eq!(error.code, "import/dxf-malformed-group");
    assert!(error.message.contains("line 3"), "{}", error.message);
}

#[test]
fn rejects_a_circle_without_a_radius_with_its_line() {
    let text = "\
0
SECTION
2
ENTITIES
0
CIRCLE
8
c
10
1
20
2
0
ENDSEC
0
EOF
";
    let error = import_dxf(text, &DxfImportRequest::default()).expect_err("missing radius");
    assert_eq!(error.code, "import/dxf-malformed-entity");
    assert!(error.message.contains("line 5"), "{}", error.message);
}

#[test]
fn rejects_an_arc_with_a_non_positive_radius() {
    let text = "\
0
SECTION
2
ENTITIES
0
ARC
10
0
20
0
40
0
50
0
51
90
0
ENDSEC
0
EOF
";
    let error = import_dxf(text, &DxfImportRequest::default()).expect_err("bad radius");
    assert_eq!(error.code, "import/dxf-malformed-entity");
}

#[test]
fn the_import_is_deterministic() {
    let request = DxfImportRequest::default();
    let first = import_dxf(&fixture(), &request).expect("first");
    let second = import_dxf(&fixture(), &request).expect("second");
    assert_eq!(first.losses, second.losses);
    assert_eq!(
        serde_json::to_string(&first.document).expect("serialize first"),
        serde_json::to_string(&second.document).expect("serialize second")
    );
}

#[test]
fn an_empty_drawing_keeps_the_default_stock() {
    let text = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
    let import = import_dxf(text, &DxfImportRequest::default()).expect("empty drawing");
    assert_eq!(import.entity_count, 0);
    assert!(import.document.operations.is_empty());
    assert!(import.document.tools.is_empty());
    assert_eq!(import.document.stock, cam_model::DEFAULT_STOCK);
}

#[test]
fn units_are_reported_and_unsupported_units_are_a_loss() {
    let supported = import_dxf(&fixture(), &DxfImportRequest::default()).expect("mm drawing");
    assert_eq!(supported.ins_units, Some(4));
    assert_eq!(supported.source_units, "millimetres");
    assert!(!supported.losses.iter().any(|loss| loss.code == "import/dxf-unsupported-units"));

    let text = "\
0
SECTION
2
HEADER
9
$INSUNITS
70
1
0
ENDSEC
0
SECTION
2
ENTITIES
0
LINE
10
0
20
0
11
10
21
0
0
ENDSEC
0
EOF
";
    let inches = import_dxf(text, &DxfImportRequest::default()).expect("inch drawing");
    assert_eq!(inches.ins_units, Some(1));
    assert_eq!(inches.source_units, "millimetres");
    assert!(inches.losses.iter().any(|loss| loss.code == "import/dxf-unsupported-units"));
}

#[test]
fn rejects_an_invalid_request_value() {
    let error = import_dxf(&fixture(), &DxfImportRequest::new(1.0, 1.0, 0.0)).expect_err("zero tolerance");
    assert_eq!(error.code, "import/invalid");
}

#[test]
fn the_session_writes_a_new_document_with_revision_zero() {
    let directory = temp_dir("session");
    let source = directory.join("parts.dxf");
    let target = directory.join("parts.ocam");
    fs::write(&source, fixture()).expect("write source");

    let mut session = OpenCam::new();
    let report = session.import_dxf(&source, &target, &DxfImportRequest::new(2.0, 0.5, 0.05)).expect("import");
    assert_eq!(report.format, "open-cam-dxf-import");
    assert_eq!(report.version, 1);
    assert_eq!(report.entity_count, 4);
    assert_eq!(report.operation_count, 4);
    assert_eq!(report.skipped_count, 1);
    assert_eq!(report.depth, 2.0);
    assert_eq!(report.stepdown, 0.5);
    assert_eq!(report.document.revision, 0);
    assert_eq!(report.document.operations, 4);

    let saved: CamDocument = serde_json::from_str(&fs::read_to_string(&target).expect("read target")).expect("parse");
    assert_eq!(saved.revision, 0);
    assert_eq!(saved.operations.len(), 4);
    assert_eq!(saved.operations[0].depth, 2.0);
    assert_eq!(saved.operations[0].stepdown, 0.5);
    assert!(validate(&saved).ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_session_rejects_an_existing_target_and_leaves_it_untouched() {
    let directory = temp_dir("exists");
    let source = directory.join("parts.dxf");
    let target = directory.join("parts.ocam");
    fs::write(&source, fixture()).expect("write source");
    fs::write(&target, "keep me").expect("write target");

    let mut session = OpenCam::new();
    let error = session.import_dxf(&source, &target, &DxfImportRequest::default()).expect_err("existing target");
    assert_eq!(error.code, "file/exists");
    assert_eq!(fs::read_to_string(&target).expect("target"), "keep me");

    let _ = fs::remove_dir_all(&directory);
}
