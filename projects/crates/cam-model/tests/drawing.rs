//! SVG and PDF toolpath drawings.

use cam_model::{
    CamDocument, Hole, Island, Operation, OperationKind, PathPoint, Stock, TabSettings, Tool, depth_map_pdf, depth_map_svg,
    generate_toolpaths, pdf, stock_setup_pdf, stock_setup_svg, svg, tool_list_pdf, tool_list_svg,
};

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

#[test]
fn pocket_islands_get_nominal_outlines_and_a_legend_row() {
    let mut doc = document();
    let mut pocket = Operation::new("island-pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    pocket.x = 20.0;
    pocket.y = 30.0;
    pocket.width = 50.0;
    pocket.height = 35.0;
    pocket.islands = vec![
        Island::new(35.0, 45.0, 12.0, 10.0),
        Island { x: 54.0, y: 50.0, width: 8.0, height: 8.0, shape: Some("circle".into()) },
    ];
    doc.operations.push(pocket);
    let report = generate_toolpaths(&doc).expect("the sample is machineable");

    let sheet = svg(&doc, &report, "islands");
    assert!(sheet.text.contains("data-islands=\"true\""), "island outlines need their own group");
    assert_eq!(sheet.text.matches("#475569").count(), 3, "rect polygon, circle and one legend row");
    assert!(sheet.text.contains("<polygon"), "the rect island is a closed outline");
    assert!(sheet.text.contains("pocket island (nominal outline)"), "the legend explains the island outline");

    let page = pdf(&doc, &report, "islands");
    assert_eq!(page.text.matches("0.278 0.333 0.412").count(), 3, "island colour appears twice plus the legend row");
    assert!(page.text.contains("pocket island \\(nominal outline\\)"));

    // A document without islands must not grow the extra legend row.
    let plain = document();
    let report = generate_toolpaths(&plain).expect("the sample is machineable");
    let sheet = svg(&plain, &report, "plain");
    assert!(!sheet.text.contains("pocket island (nominal outline)"));
    assert!(!sheet.text.contains("data-islands"));
}

#[test]
fn contour_tabs_get_marker_segments_and_a_legend_row() {
    let mut document = document();
    let mut contour = Operation::new("tabbed", OperationKind::Contour, "t6", 4.0, 2.0);
    contour.x = 20.0;
    contour.y = 20.0;
    contour.width = 40.0;
    contour.height = 20.0;
    contour.tabs = Some(TabSettings::new(4, 6.0, 1.5));
    document.operations.push(contour);
    let report = generate_toolpaths(&document).expect("the sample is machineable");

    let sheet = svg(&document, &report, "tabs");
    let group_start = sheet.text.find("data-operation=\"tabbed\"").unwrap();
    let group_end = sheet.text[group_start..].find("</g>").unwrap();
    let group = &sheet.text[group_start..group_start + group_end];
    assert_eq!(group.matches("#9a3412").count(), 4, "one thick marker per tab, repeated levels merged");
    assert!(sheet.text.contains("holding tab (lifted traverse)"), "the legend explains the tab marker");

    let page = pdf(&document, &report, "tabs");
    assert!(page.text.contains("0.604 0.204 0.071"), "the tab colour is painted in the PDF too");
    assert!(page.text.contains("holding tab \\(lifted traverse\\)"));
}

#[test]
fn rapids_form_an_underlay_below_the_cutting_moves() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = svg(&document, &report, "layers");
    assert!(
        sheet.text.find("data-rapids=\"true\"").unwrap() < sheet.text.find("<polyline").unwrap(),
        "dashed rapids must be painted before the cutting polylines"
    );
}

/// The sheet's coordinate mapping for the fixed 120 x 80 stock frame.
fn round3(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}

#[test]
fn rapid_traverses_start_where_the_previous_operation_ended() {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    document.operations.push(contour("first"));
    let mut second = contour("second");
    second.x = 70.0;
    document.operations.push(second);
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = svg(&document, &report, "traverse");

    assert_eq!(sheet.text.matches("x1=\"5\" y1=\"85\"").count(), 1, "only the first operation starts at the origin");
    let last = report.operations[0].segments.last().expect("the first operation has moves");
    let x = round3(5.0 + last.to.x);
    let y = round3(85.0 - last.to.y);
    assert!(sheet.text.contains(&format!("x1=\"{x}\" y1=\"{y}\"")), "the second operation traverses from the first one's end");
}

#[test]
fn multi_level_cuts_are_shaded_per_z_level() {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    let mut deep = contour("deep");
    deep.depth = 4.0;
    deep.stepdown = 2.0;
    document.operations.push(deep);
    let report = generate_toolpaths(&document).expect("the sample is machineable");

    let sheet = svg(&document, &report, "levels");
    assert!(sheet.text.contains("stroke=\"#1f6feb\""), "the first level keeps the full colour");
    assert!(sheet.text.contains("stroke=\"#0b2752\""), "the deepest level is shaded to 35 %");
    assert!(sheet.text.contains("Z10 depth 2 mm"), "the legend names the first level");
    assert!(sheet.text.contains("Z8 depth 4 mm"), "the legend names the second level");

    let page = pdf(&document, &report, "levels");
    assert!(page.text.contains("0.122 0.435 0.922 RG"), "the first level keeps the full colour in the PDF");
    assert!(page.text.contains("0.043 0.153 0.322 RG"), "the deepest level is shaded in the PDF too");
}

#[test]
fn legend_omits_the_repeated_kind_of_a_default_operation_id() {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    let mut single = engrave("engrave");
    single.stepdown = 1.0;
    document.operations.push(single);
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = svg(&document, &report, "default-id");
    assert!(sheet.text.contains("#1 engrave  tool t6 d6 r3"), "the repeated kind is printed once");
    assert!(!sheet.text.contains("engrave engrave"), "the legend does not stutter");
}

#[test]
fn plunge_dots_are_merged_across_depth_levels() {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    let mut pocket = Operation::new("deep-pocket", OperationKind::Pocket, "t6", 4.0, 2.0);
    pocket.x = 20.0;
    pocket.y = 20.0;
    pocket.width = 40.0;
    pocket.height = 30.0;
    document.operations.push(pocket);
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = svg(&document, &report, "plunges");

    let start = sheet.text.find("data-operation=\"deep-pocket\"").expect("the pocket group is drawn");
    let group = &sheet.text[start..];
    let group = &group[..group.find("</g>").expect("the pocket group closes")];
    assert_eq!(group.matches("r=\"0.6\"").count(), 1, "the repeated plunge point is drawn once");
}

#[test]
fn tool_list_sheet_lists_every_declared_tool() {
    let document = document();
    let sheet = tool_list_svg(&document, "bracket");
    assert!(sheet.text.contains("data-tool-table=\"true\""));
    assert!(sheet.text.contains(">t6</text>"), "the tool id is a cell");
    assert!(sheet.text.contains("flat 6"), "the tool name is a cell");
    assert!(sheet.text.contains("12000"), "the spindle speed is a cell");
    assert!(sheet.text.contains("1 tool(s)"));
    assert!(sheet.text.ends_with("</svg>\n"));
    assert_eq!(sheet.dropped, 0);

    let page = tool_list_pdf(&document, "bracket");
    assert!(page.text.starts_with("%PDF-1.4\n"));
    assert!(page.text.contains("flat 6"), "the name survives the PDF writer");
    assert!(page.text.ends_with("%%EOF\n"));

    let empty = CamDocument::new(Stock::new(50.0, 50.0, 10.0));
    assert!(tool_list_svg(&empty, "none").text.contains("no tools declared"));
    assert_eq!(tool_list_svg(&document, "bracket").text, tool_list_svg(&document, "bracket").text);
}

#[test]
fn stock_setup_sheet_draws_nominal_operation_geometry() {
    let document = document();
    let sheet = stock_setup_svg(&document, "bracket");
    assert!(sheet.text.contains("data-stock=\"true\""));
    assert!(sheet.text.contains("data-operation=\"profile\""));
    assert!(sheet.text.contains("#1 contour profile"), "operations are annotated");
    assert!(sheet.text.contains("data-legend=\"true\""));
    assert!(sheet.text.contains("data-title-block=\"true\""));
    assert!(sheet.text.contains("stock setup drawing"));
    assert!(sheet.text.contains("3 operation(s)"));
    assert!(sheet.text.contains("槽"), "SVG keeps Unicode");
    assert_eq!(sheet.dropped, 0);

    let page = stock_setup_pdf(&document, "bracket");
    assert!(page.text.starts_with("%PDF-1.4\n"));
    assert!(page.text.contains("stock setup drawing \\(PDF\\)"));
    assert!(page.text.ends_with("%%EOF\n"));
    assert!(page.dropped > 0, "the non-ASCII id degrades in the PDF");

    let drill = drill_document();
    let sheet = stock_setup_svg(&drill, "drill");
    assert_eq!(sheet.text.matches("r=\"3\" fill=\"none\"").count(), 4, "drill rings at the tool diameter");
    assert_eq!(stock_setup_svg(&document, "bracket").text, stock_setup_svg(&document, "bracket").text);
}

#[test]
fn depth_map_sheet_shades_levels_and_annotates_heights() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = depth_map_svg(&document, &report, "bracket");
    assert!(sheet.text.contains("data-depth-map=\"true\""));
    assert!(sheet.text.contains("stroke=\"#1f6feb\""), "the first contour level keeps the full colour");
    assert!(sheet.text.contains("stroke=\"#0b2752\""), "the second contour level is shaded");
    assert!(sheet.text.contains(">Z10.5</text>"), "the first level is annotated");
    assert!(sheet.text.contains(">Z9</text>"), "the second level is annotated");
    assert!(!sheet.text.contains("stroke-dasharray"), "the depth map omits the rapids");
    assert!(sheet.text.contains("depth map drawing (SVG)"));

    let page = depth_map_pdf(&document, &report, "bracket");
    assert!(page.text.starts_with("%PDF-1.4\n"));
    assert!(page.text.contains("depth map drawing \\(PDF\\)"));
    assert!(page.text.ends_with("%%EOF\n"));
    assert_eq!(depth_map_svg(&document, &report, "bracket").text, sheet.text);
}

#[test]
fn session_drawing_kinds_parse_and_render() {
    use cam_model::{DrawingKind, ExportFormat, OpenCam};

    let directory = std::env::temp_dir().join(format!("open-cam-drawing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create test directory");
    let path = directory.join("sheet.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 80.0, 12.0)).expect("stock");
    session.add_tool(tool()).expect("tool");

    let report = session.drawing(DrawingKind::ToolList, ExportFormat::Svg).expect("tool list sheet");
    assert_eq!(report.kind, "tool-list");
    assert!(report.text.contains("data-tool-table=\"true\""));
    let report = session.drawing(DrawingKind::DepthMap, ExportFormat::Pdf).expect("depth map sheet");
    assert_eq!(report.kind, "depth-map");
    assert!(report.text.starts_with("%PDF-1.4\n"));
    let report = session.drawing(DrawingKind::Toolpath, ExportFormat::Svg).expect("toolpath sheet");
    assert_eq!(report.kind, "toolpath");
    assert!(report.text.contains("data-rapids=\"true\""));

    assert_eq!("toolpath".parse::<DrawingKind>().expect("default kind"), DrawingKind::Toolpath);
    assert_eq!("stock-setup".parse::<DrawingKind>().expect("setup kind"), DrawingKind::StockSetup);
    let error = "sections".parse::<DrawingKind>().expect_err("unknown kind");
    assert_eq!(error.code, "cli/unknown-kind");
    let error = session.drawing(DrawingKind::ToolList, ExportFormat::Gcode).expect_err("gcode is not a sheet");
    assert_eq!(error.code, "export/not-drawing");
}

#[test]
fn svg_sheets_pass_a_quality_self_check() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheets = [
        svg(&document, &report, "bracket").text,
        tool_list_svg(&document, "bracket").text,
        stock_setup_svg(&document, "bracket").text,
        depth_map_svg(&document, &report, "bracket").text,
    ];
    for sheet in &sheets {
        let (width, height) = view_box(sheet);
        assert_positions(sheet, width, height);
        assert_colors(sheet);
        assert_text_positions_are_unique(sheet);
    }
}

fn view_box(sheet: &str) -> (f64, f64) {
    let start = sheet.find("viewBox=\"0 0 ").expect("a viewBox") + "viewBox=\"0 0 ".len();
    let rest = &sheet[start..];
    let end = rest.find('"').expect("the viewBox closes");
    let mut parts = rest[..end].split(' ');
    let width: f64 = parts.next().expect("a width").parse().expect("a numeric width");
    let height: f64 = parts.next().expect("a height").parse().expect("a numeric height");
    (width, height)
}

fn assert_positions(sheet: &str, width: f64, height: f64) {
    for key in [" x=\"", " x1=\"", " x2=\"", " cx=\""] {
        let mut cursor = 0;
        while let Some(found) = sheet[cursor..].find(key) {
            let start = cursor + found + key.len();
            let end = sheet[start..].find('"').expect("a quoted value");
            let value: f64 = sheet[start..start + end].parse().expect("a numeric coordinate");
            assert!((-(1e-9)..=(width + 1e-9)).contains(&value), "{key}{value} exceeds the sheet width {width}");
            cursor = start + end;
        }
    }
    for key in [" y=\"", " y1=\"", " y2=\"", " cy=\""] {
        let mut cursor = 0;
        while let Some(found) = sheet[cursor..].find(key) {
            let start = cursor + found + key.len();
            let end = sheet[start..].find('"').expect("a quoted value");
            let value: f64 = sheet[start..start + end].parse().expect("a numeric coordinate");
            assert!((-(1e-9)..=(height + 1e-9)).contains(&value), "{key}{value} exceeds the sheet height {height}");
            cursor = start + end;
        }
    }
    let mut cursor = 0;
    while let Some(found) = sheet[cursor..].find("points=\"") {
        let start = cursor + found + "points=\"".len();
        let end = sheet[start..].find('"').expect("the points close");
        for pair in sheet[start..start + end].split(' ') {
            let (x, y) = pair.split_once(',').expect("an x,y pair");
            let x: f64 = x.parse().expect("a numeric x");
            let y: f64 = y.parse().expect("a numeric y");
            assert!((-(1e-9)..=(width + 1e-9)).contains(&x), "point x {x} leaves the sheet");
            assert!((-(1e-9)..=(height + 1e-9)).contains(&y), "point y {y} leaves the sheet");
        }
        cursor = start + end;
    }
}

fn assert_colors(sheet: &str) {
    for key in ["stroke=\"", "fill=\""] {
        let mut cursor = 0;
        while let Some(found) = sheet[cursor..].find(key) {
            let start = cursor + found + key.len();
            let end = sheet[start..].find('"').expect("the colour closes");
            let value = &sheet[start..start + end];
            cursor = start + end;
            if value == "none" {
                continue;
            }
            let hex = value.strip_prefix('#').expect("a hash-prefixed colour");
            assert!(
                hex.len() == 6 && hex.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "colour `{value}` is not a lowercase 6-digit hex"
            );
        }
    }
}

fn assert_text_positions_are_unique(sheet: &str) {
    let mut seen: Vec<(i64, i64)> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = sheet[cursor..].find("<text x=\"") {
        let start = cursor + found + "<text x=\"".len();
        let x_end = sheet[start..].find('"').expect("x closes");
        let x: f64 = sheet[start..start + x_end].parse().expect("a numeric x");
        let y_start = start + sheet[start..].find(" y=\"").expect("a y attribute") + " y=\"".len();
        let y_end = sheet[y_start..].find('"').expect("y closes");
        let y: f64 = sheet[y_start..y_start + y_end].parse().expect("a numeric y");
        let key = (x.round() as i64, y.round() as i64);
        assert!(!seen.contains(&key), "two texts share the position {x}, {y}");
        seen.push(key);
        cursor = y_start + y_end;
    }
}
