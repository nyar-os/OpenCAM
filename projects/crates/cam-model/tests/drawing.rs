//! SVG and PDF drawings.

use cam_model::{
    CamDocument, DrawingOptions, Hole, Island, Operation, OperationKind, PathPoint, Stock, TabSettings, Tool, depth_map_pdf,
    depth_map_svg, generate_toolpaths, operation_sheet_pdf, operation_sheet_svg, operation_sheet_svg_with_options, pdf,
    stock_removal_pdf, stock_removal_svg, stock_setup_pdf, stock_setup_svg, svg, svg_with_options, text_width_mm,
    tool_list_pdf, tool_list_svg,
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

fn face_document() -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    let mut face = Operation::new("face", OperationKind::Face, "t6", 1.0, 0.5);
    face.width = 120.0;
    face.height = 80.0;
    document.operations.push(face);
    let mut pocket = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    pocket.x = 10.0;
    pocket.y = 10.0;
    pocket.width = 40.0;
    pocket.height = 30.0;
    document.operations.push(pocket);
    document
}

#[test]
fn svg_sheet_has_a_frame_measured_title_block_and_legend() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = svg(&document, &report, "bracket");

    assert!(drawing.text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(drawing.text.contains("xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(drawing.text.contains(" viewBox=\"0 0 297 210\""), "A4 landscape millimetre viewBox");
    assert!(drawing.text.contains("data-frame=\"true\""));
    assert!(drawing.text.contains("data-plot=\""));
    assert!(drawing.text.contains("data-stock=\"true\""));
    assert!(drawing.text.contains("data-stock-outline=\"true\""));
    assert!(drawing.text.contains("data-operation=\"profile\""));
    assert!(drawing.text.contains("data-operation=\"logo &amp; &lt;tag&gt;\""));
    assert!(drawing.text.contains("data-legend=\"true\""));
    assert!(drawing.text.contains("data-title-block=\"true\""));
    assert!(drawing.text.contains("OpenCAM toolpath drawing (SVG)"));
    assert!(drawing.text.contains("#1  contour profile  tool t6 d6"), "the legend names operation, kind and tool");
    assert!(drawing.text.contains("stock 120 x 80 mm"));
    assert!(drawing.text.contains("REV 2"));
    assert!(drawing.text.contains("units: mm"));
    assert!(drawing.text.contains("3 operation(s)"));
    assert!(drawing.text.contains("</svg>\n"));
    assert!(drawing.text.contains("槽"), "SVG keeps Unicode");
    assert_eq!(drawing.dropped, 0);
}

#[test]
fn svg_sheet_uses_short_legend_samples_in_two_columns() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = svg(&document, &report, "bracket");
    let legend = drawing.text.split("data-legend=\"true\"").nth(1).expect("a legend");
    let legend = &legend[..legend.find("</g>").expect("the legend closes")];
    assert!(legend.contains("x1=\"9\"") && legend.contains("x2=\"12\""), "samples are 3 mm long, not full width");
    let left: Vec<&str> = legend.match_indices("x1=\"").map(|(index, _)| &legend[index..]).collect();
    assert!(left.len() >= 4, "the operation and style rows use short samples");
    assert!(legend.matches("text-anchor=\"end\"").count() == 0);
}

#[test]
fn svg_sheet_weights_fill_rapids_and_cutting_lines() {
    let document = face_document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let drawing = svg(&document, &report, "face");

    assert!(drawing.text.contains("stroke-dasharray=\"0.8 1.4\""), "rapids are short light dashes");
    assert!(drawing.text.contains("stroke=\"#cbd5e1\""), "rapids use the pale underlay colour");
    assert!(
        drawing.text.find("data-rapids=\"true\"").unwrap() < drawing.text.find("<polyline").unwrap(),
        "rapid dashes stay below every cutting chain"
    );
    assert!(drawing.text.contains("stroke-width=\"0.18\""), "facing and pocket clearing are thin");
    assert!(drawing.text.contains("stroke-opacity=\"0.45\""), "facing and pocket clearing are faded");
    assert!(drawing.text.contains("stroke-width=\"0.5\""), "cutting feed keeps the main weight");
    assert!(drawing.text.contains("data-plunge=\"true\""));
    assert!(drawing.text.contains("data-start=\"true\""));
    assert!(!drawing.text.contains("r=\"0.6\""), "plunge markers no longer use the old radius");
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
    assert!(drawing.text.contains("OpenCAM toolpath drawing"));
    assert!(drawing.text.contains("REV 2"));
    assert!(drawing.text.contains("3 operation\\(s\\)"));
    assert!(drawing.text.contains("1 J 1 j"), "cutting polylines are round-joined");
    assert!(drawing.text.contains("[0.8 1.4] 0 d"), "rapid moves are dashed");
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
    assert_eq!(group.matches("data-drill-hole=\"true\"").count(), 4, "one hollow ring per hole");
    assert!(!group.contains("data-plunge=\"true\""), "a drill does not draw the small plunge dot");
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

    assert_eq!(svg(&document, &report, "bracket").text, svg(&document, &report, "bracket").text);
    assert_eq!(pdf(&document, &report, "bracket").text, pdf(&document, &report, "bracket").text);
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
    assert!(sheet.text.contains("<polygon"), "the rect island is a closed outline");
    assert!(sheet.text.contains("pocket island (nominal outline)"), "the legend explains the island outline");

    let page = pdf(&doc, &report, "islands");
    assert!(page.text.contains("pocket island \\(nominal outline\\)"));

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
fn rapid_traverses_start_where_the_previous_operation_ended() {
    let mut document = CamDocument::new(Stock::new(120.0, 80.0, 12.0));
    document.tools.push(tool());
    document.operations.push(contour("first"));
    let mut second = contour("second");
    second.x = 70.0;
    document.operations.push(second);
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = svg(&document, &report, "traverse");

    let (stock_x, stock_y, stock_w, stock_h) = rect_after(&sheet.text, "data-stock-outline=\"true\"");
    assert!(
        sheet.text.contains(&format!("x1=\"{}\" y1=\"{}\"", number(stock_x), number(stock_y + stock_h))),
        "the sheet starts at the stock origin"
    );
    assert!(stock_w > 0.0 && stock_h > 0.0);
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
    assert!(sheet.text.contains("#1  engrave  tool t6 d6"), "the repeated kind is printed once");
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
    assert_eq!(group.matches("data-plunge=\"true\"").count(), 1, "the repeated plunge point is drawn once");
}

#[test]
fn tool_list_sheet_lists_every_declared_tool() {
    let document = document();
    let sheet = tool_list_svg(&document, "bracket");
    assert!(sheet.text.contains("data-table=\"true\""));
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
    assert!(page.text.contains("stock setup drawing"));
    assert!(page.text.ends_with("%%EOF\n"));
    assert!(page.dropped > 0, "the non-ASCII id degrades in the PDF");

    let drill = drill_document();
    let sheet = stock_setup_svg(&drill, "drill");
    assert_eq!(sheet.text.matches("data-drill-hole=\"true\"").count(), 4, "drill rings at the tool diameter");
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
    assert!(page.text.contains("depth map drawing"));
    assert!(page.text.ends_with("%%EOF\n"));
    assert_eq!(depth_map_svg(&document, &report, "bracket").text, sheet.text);
}

#[test]
fn operation_sheet_tabulates_every_operation_with_right_aligned_numbers() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheet = operation_sheet_svg(&document, &report, "bracket");
    assert!(sheet.text.contains("data-table=\"true\""));
    assert!(sheet.text.contains("operation sheet"));
    assert!(sheet.text.contains(">stepover</text>"), "the column set is explicit");
    assert!(sheet.text.contains(">time s</text>"));
    assert!(sheet.text.contains(">path mm</text>"));
    assert!(sheet.text.contains(">t6</text>"));
    assert!(sheet.text.contains(">contour profile</text>"));
    assert!(sheet.text.contains(">engrave 槽</text>"));
    assert!(sheet.text.contains("text-anchor=\"end\""), "numeric columns are right aligned");
    assert!(sheet.text.contains("#1"));
    assert!(sheet.text.ends_with("</svg>\n"));
    assert_eq!(sheet.dropped, 0);

    let page = operation_sheet_pdf(&document, &report, "bracket");
    assert!(page.text.starts_with("%PDF-1.4\n"));
    assert!(page.text.contains("operation sheet"));
    assert!(page.text.ends_with("%%EOF\n"));
    assert!(page.dropped > 0, "the non-ASCII id degrades in the PDF");

    let filtered =
        operation_sheet_svg_with_options(&document, &report, "bracket", &DrawingOptions::from_csv("1").expect("valid"));
    assert!(filtered.text.contains("1 of 3 operation(s)"));
    assert!(!filtered.text.contains("槽"), "the unchecked operation has no table row");
}

#[test]
fn stock_removal_sheet_draws_layered_steps_and_allowance() {
    let mut document = document();
    let mut deep = Operation::new("deep", OperationKind::Pocket, "t6", 6.0, 2.0);
    deep.x = 30.0;
    deep.y = 20.0;
    deep.width = 30.0;
    deep.height = 20.0;
    document.operations.push(deep);
    let sheet = stock_removal_svg(&document, "bracket");

    assert!(sheet.text.contains("data-stock-removal=\"true\""));
    assert!(sheet.text.contains("data-removal-layer=\""), "removed bands are coloured per operation");
    assert!(sheet.text.contains("data-final-profile=\"true\""), "the final part outline is drawn");
    assert!(sheet.text.contains("stock allowance"), "the allowance label is present");
    assert!(sheet.text.contains("section X-X"));
    assert!(sheet.text.contains("final part outline"), "the legend explains the profile");
    assert!(sheet.text.contains("stock removal drawing"));
    assert!(sheet.text.ends_with("</svg>\n"));

    let page = stock_removal_pdf(&document, "bracket");
    assert!(page.text.starts_with("%PDF-1.4\n"));
    assert!(page.text.contains("stock removal drawing"));
    assert!(page.text.ends_with("%%EOF\n"));

    let deep_bands = sheet.text.matches("data-removal-layer=\"4\"").count();
    assert!(deep_bands > 0, "the deepest operation owns a band");
}

#[test]
fn operation_filter_renders_only_the_selected_operations() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let options = DrawingOptions::from_csv("1,3").expect("a valid list");
    let sheet = svg_with_options(&document, &report, "subset", &options);

    assert!(sheet.text.contains("data-operation=\"profile\""));
    assert!(sheet.text.contains("data-operation=\"槽\""));
    assert!(!sheet.text.contains("data-operation=\"logo &amp; &lt;tag&gt;\""), "the unchecked operation is hidden");
    assert!(sheet.text.contains("2 of 3 operation(s)"));

    let page = cam_model::pdf_with_options(&document, &report, "subset", &options);
    assert!(page.text.contains("2 of 3 operation\\(s\\)"));

    let error = DrawingOptions::from_csv("1,x").expect_err("a bad list fails");
    assert_eq!(error.code, "drawing/invalid-ops");
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
    assert!(report.text.contains("data-table=\"true\""));
    let report = session.drawing(DrawingKind::DepthMap, ExportFormat::Pdf).expect("depth map sheet");
    assert_eq!(report.kind, "depth-map");
    assert!(report.text.starts_with("%PDF-1.4\n"));
    let report = session.drawing(DrawingKind::OperationSheet, ExportFormat::Svg).expect("operation sheet");
    assert_eq!(report.kind, "operation-sheet");
    assert!(report.text.contains("operation sheet"));
    let report = session.drawing(DrawingKind::StockRemoval, ExportFormat::Pdf).expect("stock removal sheet");
    assert_eq!(report.kind, "stock-removal");
    assert!(report.text.starts_with("%PDF-1.4\n"));
    let report = session.drawing(DrawingKind::Toolpath, ExportFormat::Svg).expect("toolpath sheet");
    assert_eq!(report.kind, "toolpath");
    assert!(report.text.contains("data-rapids=\"true\""));

    assert_eq!("toolpath".parse::<DrawingKind>().expect("default kind"), DrawingKind::Toolpath);
    assert_eq!("stock-setup".parse::<DrawingKind>().expect("setup kind"), DrawingKind::StockSetup);
    assert_eq!("operation-sheet".parse::<DrawingKind>().expect("operation sheet kind"), DrawingKind::OperationSheet);
    assert_eq!("stock-removal".parse::<DrawingKind>().expect("removal kind"), DrawingKind::StockRemoval);
    let error = "sections".parse::<DrawingKind>().expect_err("unknown kind");
    assert_eq!(error.code, "cli/unknown-kind");
    let error = session.drawing(DrawingKind::ToolList, ExportFormat::Gcode).expect_err("gcode is not a sheet");
    assert_eq!(error.code, "export/not-drawing");

    let options = DrawingOptions::from_csv("1").expect("a valid list");
    let report = session.drawing_with_options(DrawingKind::Toolpath, ExportFormat::Svg, &options).expect("filtered sheet");
    assert!(report.text.contains("0 operation(s)"));
}

#[test]
fn svg_sheets_pass_a_quality_self_check() {
    let document = document();
    let report = generate_toolpaths(&document).expect("the sample is machineable");
    let sheets = [
        ("toolpath", svg(&document, &report, "bracket").text),
        ("tool-list", tool_list_svg(&document, "bracket").text),
        ("stock-setup", stock_setup_svg(&document, "bracket").text),
        ("depth-map", depth_map_svg(&document, &report, "bracket").text),
        ("operation-sheet", operation_sheet_svg(&document, &report, "bracket").text),
        ("stock-removal", stock_removal_svg(&document, "bracket").text),
    ];
    for (name, sheet) in &sheets {
        let (width, height) = view_box(sheet);
        assert_eq!((width, height), (297.0, 210.0), "every sheet is A4 landscape");
        assert_positions(sheet, width, height);
        assert_colors(sheet);
        assert_rectangles(sheet);
        assert_text_fits(sheet);
        assert_text_positions_are_unique(name, sheet);
    }
    assert_coverage(&sheets[0].1);
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

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}

fn rect_after(sheet: &str, marker: &str) -> (f64, f64, f64, f64) {
    let start = sheet.find(marker).expect("the marked element") + marker.len();
    let element = &sheet[start..start + sheet[start..].find('>').expect("the element closes")];
    let x = attribute(element, "x");
    let y = attribute(element, "y");
    let width = attribute(element, "width");
    let height = attribute(element, "height");
    (x, y, width, height)
}

fn attribute(element: &str, name: &str) -> f64 {
    let key = format!(" {name}=\"");
    let start = element.find(&key).expect("the attribute") + key.len();
    let end = element[start..].find('"').expect("the attribute value closes");
    element[start..start + end].parse().expect("a numeric attribute")
}

fn assert_coverage(sheet: &str) {
    let plot_start = sheet.find("data-plot=\"").expect("a plot area") + "data-plot=\"".len();
    let plot_end = sheet[plot_start..].find('"').expect("the plot closes");
    let plot: Vec<f64> =
        sheet[plot_start..plot_start + plot_end].split(',').map(|value| value.parse().expect("a plot number")).collect();
    let (_, _, width, height) = rect_after(sheet, "data-stock-outline=\"true\"");
    let plot_area = plot[2] * plot[3];
    assert!(width * height / plot_area >= 0.65, "the stock fills at least 65 % of the drawable area");
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

fn assert_rectangles(sheet: &str) {
    let mut cursor = 0;
    while let Some(found) = sheet[cursor..].find("<rect ") {
        let start = cursor + found;
        let tag_end = start + sheet[start..].find('>').expect("the rect tag closes");
        let tag = &sheet[start..tag_end];
        for name in ["width", "height"] {
            let value = attribute(tag, name);
            assert!(value >= -1e-9, "rect {name} {value} is negative");
        }
        cursor = tag_end;
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

fn assert_text_fits(sheet: &str) {
    let mut cursor = 0;
    let mut count = 0;
    while let Some(found) = sheet[cursor..].find("<text ") {
        let start = cursor + found;
        let tag_end = start + sheet[start..].find('>').expect("the text tag closes");
        let tag = &sheet[start..tag_end];
        let content_start = tag_end + 1;
        let content_end = content_start + sheet[content_start..].find("</text>").expect("the text closes");
        let content = &sheet[content_start..content_end];
        let x = attribute(tag, "x");
        let y = attribute(tag, "y");
        let size = attribute(tag, "font-size");
        assert!(tag.contains("data-box=\""), "every text carries its cell: {tag}");
        let box_start = tag.find("data-box=\"").expect("a box") + "data-box=\"".len();
        let box_end = tag[box_start..].find('"').expect("the box closes");
        let bounds: Vec<f64> =
            tag[box_start..box_start + box_end].split(',').map(|value| value.parse().expect("a box number")).collect();
        let width = text_width_mm(content, size);
        let left = if tag.contains("text-anchor=\"end\"") { x - width } else { x };
        assert!(left >= bounds[0] - 0.02, "text `{content}` starts at {left}, before its cell {}", bounds[0]);
        assert!(
            left + width <= bounds[0] + bounds[2] + 0.02,
            "text `{content}` is {width} mm wide and overflows its {} mm cell",
            bounds[2]
        );
        assert!((0.0..=210.0).contains(&y), "text baseline {y} leaves the page");
        count += 1;
        cursor = content_end;
    }
    assert!(count > 0, "the sheet writes text");
}

fn assert_text_positions_are_unique(name: &str, sheet: &str) {
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
        assert!(!seen.contains(&key), "{name}: two texts share the position {x}, {y}");
        seen.push(key);
        cursor = y_start + y_end;
    }
}
