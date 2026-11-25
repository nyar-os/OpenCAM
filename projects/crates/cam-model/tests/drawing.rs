//! SVG and PDF toolpath drawings.

use cam_model::{CamDocument, Hole, Operation, OperationKind, PathPoint, Stock, Tool, generate_toolpaths, pdf, svg};

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "flat 6".to_string(),
        number: 1,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn contour(id: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Contour, "t6", 3.0, 1.5);
    operation.x = 20.0;
    operation.y = 20.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation
}

fn engrave(id: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, "t6", 1.0, 0.5);
    operation.path = vec![PathPoint { x: 12.0, y: 12.0 }, PathPoint { x: 60.0, y: 12.0 }];
    operation
}

/// One document with a contour, an ASCII id that needs XML escaping and a
/// non-ASCII engrave id.
fn document() -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    document.operations.push(contour("profile"));
    document.operations.push(engrave("logo & <tag>"));
    document.operations.push(engrave("槽"));
    document.revision = 2;
    document
}

/// One document with one drilling operation using the 6 mm tool.
fn drill_document() -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    let mut drill = Operation::new("holes", OperationKind::Drill, "t6", 12.0, 6.0);
    drill.holes =
        vec![Hole { x: 25.0, y: 25.0 }, Hole { x: 45.0, y: 25.0 }, Hole { x: 25.0, y: 45.0 }, Hole { x: 45.0, y: 45.0 }];
    document.operations.push(drill);
    document
}

#[test]
fn svg_sheet_has_stock_paths_legend_and_title_block() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = svg(&document, &report, "bracket");

    assert!(drawing.text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(drawing.text.contains("xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(drawing.text.contains(" viewBox=\"0 0 "));
    assert!(drawing.text.contains("width=\""));
    assert!(drawing.text.contains("data-stock=\"true\""));
    assert!(drawing.text.contains("data-operation=\"profile\""));
    assert!(drawing.text.contains("data-operation=\"logo &amp; &lt;tag&gt;\""));
    assert!(drawing.text.contains("data-legend=\"true\""));
    assert!(drawing.text.contains("data-title-block=\"true\""));
    assert!(drawing.text.contains("OpenCAM toolpath drawing (SVG)"));
    assert!(drawing.text.contains("tool t6 d6 r3"), "the legend names the tool");
    assert!(drawing.text.contains("stock 120 x 80 mm"));
    assert!(drawing.text.contains("REV 2"));
    assert!(drawing.text.contains("units: mm"));
    assert!(drawing.text.contains("3 operation(s)"));
    assert!(drawing.text.contains("</svg>\n"));
    assert!(drawing.text.contains("槽"), "SVG keeps Unicode");
    assert_eq!(drawing.dropped, 0);
}

#[test]
fn svg_sheet_marks_rapid_and_cutting_moves() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = svg(&document, &report, "bracket");

    assert!(drawing.text.contains("<polyline points=\""), "cutting moves are polylines");
    assert!(drawing.text.contains("stroke-dasharray=\"1.5 1\""), "rapid moves are dashed");
    assert!(drawing.text.contains("r=\"1.4\""), "the operation start point is marked");
    assert!(drawing.text.contains("r=\"0.6\""), "plunge points are marked");
}

#[test]
fn pdf_sheet_is_one_a4_landscape_page_with_a_title_block() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = pdf(&document, &report, "bracket");

    assert!(drawing.text.starts_with("%PDF-1.4\n"));
    assert!(drawing.text.contains("/MediaBox [0 0 842 595]"), "A4 landscape");
    assert!(drawing.text.contains("/Count 1"), "exactly one page");
    assert!(drawing.text.contains("/BaseFont /Helvetica"));
    assert!(drawing.text.contains("BT /F1"));
    assert!(drawing.text.contains("OpenCAM toolpath drawing \\(PDF\\)"));
    assert!(drawing.text.contains("REV 2"));
    assert!(drawing.text.contains("3 operation\\(s\\)"));
    assert!(drawing.text.contains("1 J 1 j"), "cutting polylines are round-joined");
    assert!(drawing.text.contains("[2 2] 0 d"), "rapid moves are dashed");
    assert!(drawing.text.ends_with("%%EOF\n"));
}

#[test]
fn svg_draws_drill_holes_at_tool_diameter() {
    let document = drill_document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = svg(&document, &report, "drill");

    let start = drawing.text.find("data-operation=\"holes\"").expect("the drill group is drawn");
    let group = &drawing.text[start..];
    let group = &group[..group.find("</g>").expect("the drill group closes")];
    assert_eq!(group.matches("r=\"3\" fill=\"none\"").count(), 4, "one hollow ring per hole at the 3 mm tool radius");
    assert!(!group.contains("r=\"0.6\""), "a drill does not draw the small plunge dot");
    assert!(drawing.text.contains("drill hole (tool diameter)"), "the legend explains the hole ring");
}

#[test]
fn pdf_draws_drill_holes_at_tool_diameter() {
    let document = drill_document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = pdf(&document, &report, "drill");

    assert!(drawing.text.starts_with("%PDF-1.4\n"));
    assert!(drawing.text.ends_with("%%EOF\n"));
    assert_eq!(drawing.text.matches("% drill hole r=3\n").count(), 4, "one 3 mm ring per hole");
    assert!(drawing.text.contains("drill hole \\(tool diameter\\)"), "the legend row is written");
}

#[test]
fn both_formats_are_deterministic() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");

    let first = svg(&document, &report, "bracket");
    let second = svg(&document, &report, "bracket");
    assert_eq!(first.text, second.text);
    let first = pdf(&document, &report, "bracket");
    let second = pdf(&document, &report, "bracket");
    assert_eq!(first.text, second.text);
}

#[test]
fn empty_document_draws_a_valid_empty_sheet() {
    let document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    let report = generate_toolpaths(&document).expect("an empty document has an empty toolpath");

    let sheet = svg(&document, &report, "empty");
    assert!(sheet.text.contains("data-stock=\"true\""));
    assert!(sheet.text.contains("data-title-block=\"true\""));
    assert!(sheet.text.contains("0 operation(s)"));
    assert!(sheet.text.contains("est 0 s"));
    assert!(!sheet.text.contains("<polyline"));
    assert!(sheet.text.ends_with("</svg>\n"));

    let sheet = pdf(&document, &report, "empty");
    assert!(sheet.text.contains("/Count 1"));
    assert!(sheet.text.contains("0 operation\\(s\\)"));
    assert!(sheet.text.ends_with("%%EOF\n"));
    assert_eq!(sheet.dropped, 0);
}

#[test]
fn non_ascii_text_is_a_loss_in_pdf_only() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");

    let vector = svg(&document, &report, "bracket");
    assert!(vector.text.contains("槽"));
    assert_eq!(vector.dropped, 0);

    let sheet = pdf(&document, &report, "bracket");
    assert!(sheet.dropped > 0, "the non-ASCII operation id is counted");
    assert!(!sheet.text.contains('槽'), "PDF text degrades to ASCII");
}
