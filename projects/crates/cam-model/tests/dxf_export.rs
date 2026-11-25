use cam_model::{
    CamDocument, DxfImportRequest, ExportFormat, Hole, Operation, OperationKind, PathPoint, Stock, Tool, export_dxf, import_dxf,
};

fn engrave_tool() -> Tool {
    Tool {
        id: "engrave".to_string(),
        name: "engrave".to_string(),
        number: 1,
        diameter: 0.2,
        flutes: 1,
        spindle: 12000.0,
        feed: 300.0,
        plunge_feed: 100.0,
    }
}

fn engrave(id: &str, points: &[(f64, f64)], closed: bool) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, "engrave", 1.0, 1.0);
    operation.path = points.iter().map(|&(x, y)| PathPoint::new(x, y)).collect();
    operation.closed = closed;
    operation
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn round_trips_engrave_paths_and_closed_flags() {
    let mut document = CamDocument::new(Stock::new(100.0, 50.0, 10.0));
    document.tools.push(engrave_tool());
    document.operations.push(engrave("poly", &[(10.0, 10.0), (40.0, 10.0), (40.0, 25.0)], true));
    document.operations.push(engrave("line", &[(5.0, 5.0), (45.0, 5.0)], false));

    let text = export_dxf(&document);
    let import = import_dxf(&text, &DxfImportRequest::default()).expect("valid drawing");
    assert_eq!(import.source_units, "millimetres");
    assert_eq!(import.entity_count, 3, "stock rectangle plus the two paths");
    assert_eq!(import.skipped_count, 0);
    assert_eq!(
        import.losses.iter().map(|loss| (loss.code.as_str(), loss.entity.as_str(), loss.message.as_str())).collect::<Vec<_>>(),
        vec![("import/dxf-unsupported-section", "SECTION", "section `TABLES` is ignored")],
        "the exported layer table is the only section the reader skips"
    );

    let stock = &import.document.operations[0];
    assert_eq!(stock.name, "STOCK");
    assert!(stock.closed);
    assert_eq!(stock.path.len(), 4);

    let poly = &import.document.operations[1];
    assert_eq!(poly.name, "poly");
    assert!(poly.closed);
    let points: Vec<_> = poly.path.iter().map(|point| (point.x, point.y)).collect();
    assert_eq!(points.len(), 3);
    for (actual, expected) in points.iter().zip([(10.0, 10.0), (40.0, 10.0), (40.0, 25.0)]) {
        assert_close(actual.0, expected.0);
        assert_close(actual.1, expected.1);
    }

    let line = &import.document.operations[2];
    assert_eq!(line.name, "line");
    assert!(!line.closed);
    assert_eq!(line.path.len(), 2);
    assert_close(line.path[0].x, 5.0);
    assert_close(line.path[1].x, 45.0);
}

#[test]
fn writes_an_open_two_point_path_as_a_line() {
    let mut document = CamDocument::new(Stock::new(100.0, 50.0, 10.0));
    document.tools.push(engrave_tool());
    document.operations.push(engrave("line", &[(5.0, 5.0), (45.0, 5.0)], false));
    let text = export_dxf(&document);
    assert!(text.contains("0\nLINE\n8\nline\n10\n5\n20\n5\n11\n45\n21\n5\n"), "{text}");
}

#[test]
fn writes_the_stock_even_without_operations() {
    let document = CamDocument::new(Stock::new(60.0, 40.0, 10.0));
    let text = export_dxf(&document);
    let import = import_dxf(&text, &DxfImportRequest::default()).expect("valid drawing");
    assert_eq!(import.entity_count, 1);
    assert_eq!(import.document.operations.len(), 1);
    assert_eq!(import.document.operations[0].name, "STOCK");
    assert!(import.document.operations[0].closed);
}

#[test]
fn writes_circle_outlines_and_drill_holes_as_circles() {
    let mut document = CamDocument::new(Stock::new(60.0, 60.0, 10.0));
    document.tools.push(Tool {
        id: "endmill".to_string(),
        name: "endmill".to_string(),
        number: 1,
        diameter: 4.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1000.0,
        plunge_feed: 300.0,
    });
    let mut ring = Operation::new("ring", OperationKind::Contour, "endmill", 2.0, 1.0);
    ring.x = 10.0;
    ring.y = 10.0;
    ring.width = 20.0;
    ring.height = 20.0;
    ring.shape = Some("circle".to_string());
    document.operations.push(ring);
    let mut holes = Operation::new("holes", OperationKind::Drill, "endmill", 5.0, 5.0);
    holes.holes = vec![Hole::new(30.0, 30.0), Hole::new(40.0, 40.0)];
    document.operations.push(holes);

    let text = export_dxf(&document);
    assert!(text.contains("0\nCIRCLE\n8\nring\n10\n20\n20\n20\n40\n10\n"), "{text}");
    assert!(text.contains("0\nCIRCLE\n8\nholes\n10\n30\n20\n30\n40\n2\n"), "{text}");
    assert!(text.contains("0\nCIRCLE\n8\nholes\n10\n40\n20\n40\n40\n2\n"), "{text}");

    let import = import_dxf(&text, &DxfImportRequest::default()).expect("valid drawing");
    let ring = import.document.operations.iter().find(|operation| operation.name == "ring").expect("ring");
    assert!(ring.closed);
    for point in &ring.path {
        assert_close((point.x - 20.0).powi(2) + (point.y - 20.0).powi(2), 100.0);
    }
    let drilled: Vec<_> = import.document.operations.iter().filter(|operation| operation.name == "holes").collect();
    assert_eq!(drilled.len(), 2);
    for (operation, centre) in drilled.iter().zip([(30.0, 30.0), (40.0, 40.0)]) {
        for point in &operation.path {
            assert_close((point.x - centre.0).powi(2) + (point.y - centre.1).powi(2), 4.0);
        }
    }
}

#[test]
fn sanitizes_operation_layers_and_keeps_output_deterministic() {
    let mut document = CamDocument::new(Stock::new(100.0, 50.0, 10.0));
    document.tools.push(engrave_tool());
    document.operations.push(engrave("a b/c", &[(10.0, 10.0), (40.0, 10.0)], false));
    let text = export_dxf(&document);
    assert!(text.contains("8\na-b-c\n"), "{text}");
    assert_eq!(text, export_dxf(&document), "the same document always writes the same bytes");
}

#[test]
fn export_names_layers_and_extents_for_cad_tools() {
    let mut document = CamDocument::new(Stock::new(80.0, 50.0, 10.0));
    document.tools.push(engrave_tool());
    document.operations.push(engrave("a.b", &[(10.0, 10.0), (40.0, 10.0)], false));
    let text = export_dxf(&document);
    assert!(text.contains("2\nTABLES\n"), "the layer table section is written: {text}");
    assert!(text.contains("2\nLAYER\n70\n2\n"), "STOCK plus the operation layer are declared");
    assert!(text.contains("0\nLAYER\n2\nSTOCK\n70\n0\n62\n7\n6\nCONTINUOUS\n"));
    assert!(text.contains("0\nLAYER\n2\na-b\n70\n0\n62\n7\n6\nCONTINUOUS\n"));
    assert!(text.contains("9\n$EXTMIN\n10\n0\n20\n0\n30\n0\n"), "$EXTMIN spans the stock origin");
    assert!(text.contains("9\n$EXTMAX\n10\n80\n20\n50\n30\n0\n"), "$EXTMAX spans the stock corner");
}

#[test]
fn parses_dxf_as_an_export_format_and_keeps_step_unimplemented() {
    assert_eq!("dxf".parse::<ExportFormat>().expect("dxf"), ExportFormat::Dxf);
    assert_eq!(ExportFormat::Dxf.as_str(), "dxf");
    assert!(!ExportFormat::Dxf.is_drawing());
    let error = "step".parse::<ExportFormat>().expect_err("step stays unimplemented");
    assert_eq!(error.code, "capability/not-implemented");
}
