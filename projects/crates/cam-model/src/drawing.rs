//! Vector drawing export.
//!
//! Every sheet is drawn on an A4 landscape page with an outer frame, a measured
//! title block and a two-column legend. The stock outline and the toolpath share
//! one uniform fit into the drawable area, so the sheet stays readable when five
//! operations overlap: facing and pocket clearing are light thin lines, cutting
//! feed keeps the operation colour, and rapids are a short pale dashed underlay.
//!
//! [`svg`] and [`pdf`] draw the stock outline and the moves [`crate::toolpath`]
//! already generated. Pocket islands get nominal outlines and contour holding
//! tabs become thick markers. [`operation_sheet_svg`] and [`operation_sheet_pdf`]
//! tabulate every operation, [`stock_removal_svg`] and [`stock_removal_pdf`] draw
//! the stepped stock-removal section along the X centre line, and the tool list,
//! stock setup and depth map sheets share the same frame and title block.
//!
//! Nothing here regenerates a toolpath, and every sheet is deterministic:
//! coordinates are rounded to three decimals, operations keep document order and
//! no timestamp is written.

use std::collections::HashSet;

use crate::{
    CamError,
    model::{CamDocument, Island, Operation, OperationKind, Tool},
    toolpath::{Move, MoveKind, OperationPath, ToolpathReport},
};

/// A rendered drawing and the characters the target format could not encode.
///
/// [`svg`] is XML and keeps Unicode, so it reports `0`. [`pdf`] writes base-14
/// Helvetica, so every non-ASCII character becomes `?` and is counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawing {
    /// Rendered document text.
    pub text: String,
    /// Non-ASCII characters replaced by `?`; `0` for SVG.
    pub dropped: usize,
}

/// Rendering options shared by every sheet.
///
/// [`DrawingOptions::ops`] is a list of one-based operation numbers in document
/// order. `None` draws every operation; a filtered sheet keeps the operation
/// colours, legend rows and title count of the selected subset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawingOptions {
    /// One-based operation numbers to draw, in document order.
    pub ops: Option<Vec<usize>>,
}

impl DrawingOptions {
    /// Draws every operation.
    pub fn all() -> Self {
        Self::default()
    }

    /// Parses a comma separated operation list such as `1,3,5`.
    ///
    /// Empty entries are ignored. A non-numeric or zero entry reports
    /// `drawing/invalid-ops`.
    pub fn from_csv(value: &str) -> Result<Self, CamError> {
        let mut ops = Vec::new();
        for entry in value.split(',').map(str::trim).filter(|entry| !entry.is_empty()) {
            let number: usize = entry
                .parse()
                .map_err(|_| CamError::new("drawing/invalid-ops", format!("`{entry}` is not an operation number")))?;
            if number == 0 {
                return Err(CamError::new("drawing/invalid-ops", "operation numbers start at 1"));
            }
            if !ops.contains(&number) {
                ops.push(number);
            }
        }
        Ok(Self { ops: Some(ops) })
    }

    /// Zero-based indexes of the operations this sheet draws.
    fn selected(&self, count: usize) -> Vec<usize> {
        match &self.ops {
            None => (0..count).collect(),
            Some(ops) => (0..count).filter(|index| ops.contains(&(index + 1))).collect(),
        }
    }

    fn filtered(&self, count: usize) -> bool {
        self.ops.as_ref().is_some_and(|ops| ops.len() < count)
    }
}

/// Estimated width of `text` at `font_size` millimetres in the monospace face.
///
/// ASCII characters take 0.6 em and every other character one em, matching the
/// conservative estimate the renderer uses to measure title and table cells.
pub fn text_width_mm(text: &str, font_size: f64) -> f64 {
    text.chars().map(|character| if character.is_ascii() { 0.6 } else { 1.0 }).sum::<f64>() * font_size
}

/// Stroke colours per operation index; operation `i` always uses entry `i % 8`.
const PALETTE: [(&str, [u8; 3]); 8] = [
    ("#1f6feb", [31, 111, 235]),
    ("#d97706", [217, 119, 6]),
    ("#15803d", [21, 128, 61]),
    ("#b91c1c", [185, 28, 28]),
    ("#7c3aed", [124, 58, 237]),
    ("#0e7490", [14, 116, 144]),
    ("#a21caf", [162, 28, 175]),
    ("#4d7c0f", [77, 124, 15]),
];

const STOCK_COLOR: &str = "#94a3b8";
const STOCK_RGB: [u8; 3] = [148, 163, 184];
const INK: &str = "#1f2933";
const INK_RGB: [u8; 3] = [31, 41, 51];
const MUTED: &str = "#64748b";
const MUTED_RGB: [u8; 3] = [100, 116, 139];
const RAPID_COLOR: &str = "#cbd5e1";
const RAPID_RGB: [u8; 3] = [203, 213, 225];
const ISLAND_COLOR: &str = "#475569";
const ISLAND_RGB: [u8; 3] = [71, 85, 105];
const TAB_COLOR: &str = "#9a3412";
const TAB_RGB: [u8; 3] = [154, 52, 18];
const PAPER_COLOR: &str = "#fbfcfe";

const PATH_WIDTH_MM: f64 = 0.5;
const FILL_WIDTH_MM: f64 = 0.18;
const RAPID_WIDTH_MM: f64 = 0.18;
const ISLAND_WIDTH_MM: f64 = 0.25;
const TAB_WIDTH_MM: f64 = 1.1;
const DRILL_RING_WIDTH_MM: f64 = 0.45;
const SVG_WIDTH_PX: f64 = 1040.0;
const SVG_HEIGHT_PX: f64 = 735.0;
const ARC_CHORDS_PER_TURN: usize = 72;

const PAGE_WIDTH_MM: f64 = 297.0;
const PAGE_HEIGHT_MM: f64 = 210.0;
const FRAME_MARGIN_MM: f64 = 5.0;
const TITLE_HEIGHT_MM: f64 = 22.0;
const LEGEND_ROW_MM: f64 = 3.8;
const LEGEND_GAP_MM: f64 = 3.0;
const LEGEND_PAD_MM: f64 = 2.0;
const LEGEND_SAMPLE_MM: f64 = 3.0;
const FIT_MARGIN_FRACTION: f64 = 0.08;
const TABLE_ROW_MM: f64 = 5.0;
const TABLE_FONT_MM: f64 = 2.3;
const TABLE_INSET_MM: f64 = 2.0;
const LEGEND_FONT_MM: f64 = 2.2;
const SECTION_Z_EXAGGERATION: f64 = 4.0;
const TITLE_PAD_MM: f64 = 2.0;
const TITLE_MIN_FONT_MM: f64 = 1.5;
const TITLE_BOTTOM_GAP_MM: f64 = 2.0;
/// Fraction of a cell's text width the renderer fills, leaving room for wider
/// glyph advances than the conservative `0.6 em` estimate.
const TITLE_FIT_FRACTION: f64 = 0.96;

const PDF_PAGE_WIDTH: f64 = 842.0;
const PDF_PAGE_HEIGHT: f64 = 595.0;
const MM_TO_PT: f64 = 2.834_646;

/// Renders the toolpath sheet as SVG with every operation.
pub fn svg(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    svg_with_options(document, report, id, &DrawingOptions::all())
}

/// Renders the toolpath sheet as SVG, optionally drawing a subset of operations.
pub fn svg_with_options(document: &CamDocument, report: &ToolpathReport, id: &str, options: &DrawingOptions) -> Drawing {
    let drawings = project_all(document, report);
    let selected = options.selected(report.operations.len());
    let legend = toolpath_legend(document, report, &drawings, &selected);
    let layout = SheetLayout::new(legend.len());
    let frame = toolpath_frame(document, &drawings, &selected);
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut svg = svg_open(&layout, "OpenCAM toolpath drawing", id);
    push_svg_frame(&mut svg, &layout);

    push_svg_stock(&mut svg, document, &fit);
    push_svg_islands(&mut svg, document, &fit, &selected);

    svg.push_str("  <g data-rapids=\"true\">\n");
    for &index in &selected {
        for (from, to) in &drawings[index].rapids {
            push_svg_line(&mut svg, &fit, *from, *to, RAPID_COLOR, RAPID_WIDTH_MM, Some("0.8 1.4"), None);
        }
    }
    svg.push_str("  </g>\n");

    for &index in &selected {
        push_svg_operation(&mut svg, document, report, &drawings[index], index, &fit);
    }

    push_svg_legend(&mut svg, &layout, &legend);
    let note = operation_count_note(options, report.operations.len());
    push_svg_title_block(&mut svg, &layout, toolpath_title_cells(document, report, id, &note, "SVG"));
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the toolpath sheet as a one-page A4 landscape PDF 1.4 document.
pub fn pdf(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    pdf_with_options(document, report, id, &DrawingOptions::all())
}

/// Renders the toolpath sheet as PDF, optionally drawing a subset of operations.
pub fn pdf_with_options(document: &CamDocument, report: &ToolpathReport, id: &str, options: &DrawingOptions) -> Drawing {
    let drawings = project_all(document, report);
    let selected = options.selected(report.operations.len());
    let legend = toolpath_legend(document, report, &drawings, &selected);
    let layout = SheetLayout::new(legend.len());
    let frame = toolpath_frame(document, &drawings, &selected);
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut dropped = 0usize;
    let mut content = pdf_open(&layout, "OpenCAM toolpath drawing", id, &mut dropped);
    pdf_frame(&mut content, &layout);

    pdf_stock(&mut content, document, &fit);
    pdf_islands(&mut content, document, &fit);

    for &index in &selected {
        for (from, to) in &drawings[index].rapids {
            pdf_line(&mut content, fit.map(*from), fit.map(*to), RAPID_RGB, RAPID_WIDTH_MM, Some("0.8 1.4"));
        }
    }
    for &index in &selected {
        pdf_operation(&mut content, document, &drawings[index], index, &fit, &mut dropped);
    }

    pdf_legend(&mut content, &layout, &legend, &mut dropped);
    let note = operation_count_note(options, report.operations.len());
    pdf_title_block(&mut content, &layout, toolpath_title_cells(document, report, id, &note, "PDF"), &mut dropped);
    finish_pdf_page(&content, dropped)
}

/// Renders the declared tools as an SVG table sheet.
pub fn tool_list_svg(document: &CamDocument, id: &str) -> Drawing {
    let headers = ["id", "name", "T", "d mm", "r mm", "flutes", "rpm", "feed", "plunge"];
    let rows: Vec<Vec<String>> = document
        .tools
        .iter()
        .map(|tool| {
            vec![
                tool.id.clone(),
                tool.name.clone(),
                number(f64::from(tool.number)),
                number(tool.diameter),
                number(tool.radius()),
                number(f64::from(tool.flutes)),
                number(tool.spindle),
                number(tool.feed),
                number(tool.plunge_feed),
            ]
        })
        .collect();
    let mut drawing = svg_table_sheet(
        id,
        "OpenCAM tool list drawing (SVG)",
        format!("{} tool(s)", document.tools.len()),
        &headers,
        &rows,
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        "no tools declared",
    );
    drawing.text = drawing.text.replacen("<g data-table=\"true\">", "<g data-table=\"true\" data-tool-table=\"true\">", 1);
    drawing
}

/// Renders the declared tools as one PDF page.
pub fn tool_list_pdf(document: &CamDocument, id: &str) -> Drawing {
    let headers = ["id", "name", "T", "d mm", "r mm", "flutes", "rpm", "feed", "plunge"];
    let rows: Vec<Vec<String>> = document
        .tools
        .iter()
        .map(|tool| {
            vec![
                tool.id.clone(),
                tool.name.clone(),
                number(f64::from(tool.number)),
                number(tool.diameter),
                number(tool.radius()),
                number(f64::from(tool.flutes)),
                number(tool.spindle),
                number(tool.feed),
                number(tool.plunge_feed),
            ]
        })
        .collect();
    pdf_table_sheet(
        id,
        "OpenCAM tool list drawing (PDF)",
        format!("{} tool(s)", document.tools.len()),
        &headers,
        &rows,
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        "no tools declared",
    )
}

/// Renders the nominal operation geometry on the stock as an SVG setup sheet.
pub fn stock_setup_svg(document: &CamDocument, id: &str) -> Drawing {
    let frame = setup_frame(document);
    let legend = setup_legend(document);
    let layout = SheetLayout::new(legend.len());
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut svg = svg_open(&layout, "OpenCAM stock setup drawing", id);
    push_svg_frame(&mut svg, &layout);
    push_svg_stock(&mut svg, document, &fit);

    for (index, operation) in document.operations.iter().enumerate() {
        let (hex, _) = palette(index);
        svg.push_str(&format!("  <g data-operation=\"{}\" data-index=\"{index}\">\n", escape_xml(&operation.id)));
        push_svg_setup_geometry(&mut svg, document, operation, hex, &fit);
        let (x, y) = operation_label_point(operation);
        push_svg_text(
            &mut svg,
            fit.map_x(x) + 0.8,
            fit.map_y(y) + 3.0 + f64::from(index as u32) * 2.6,
            2.2,
            INK,
            &format!("#{} {}", index + 1, operation_name(operation.kind, &operation.id)),
            layout.plot,
            None,
        );
        svg.push_str("  </g>\n");
    }

    push_svg_legend(&mut svg, &layout, &legend);
    let note = format!("{} operation(s)", document.operations.len());
    push_svg_title_block(&mut svg, &layout, stock_setup_title_cells(document, id, &note, "SVG"));
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the nominal operation geometry on the stock as one PDF page.
pub fn stock_setup_pdf(document: &CamDocument, id: &str) -> Drawing {
    let frame = setup_frame(document);
    let legend = setup_legend(document);
    let layout = SheetLayout::new(legend.len());
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut dropped = 0usize;
    let mut content = pdf_open(&layout, "OpenCAM stock setup drawing", id, &mut dropped);
    pdf_frame(&mut content, &layout);
    pdf_stock(&mut content, document, &fit);

    for (index, operation) in document.operations.iter().enumerate() {
        pdf_setup_geometry(&mut content, document, operation, palette(index).1, &fit);
        let (x, y) = operation_label_point(operation);
        pdf_text_at(
            &mut content,
            fit.map_x(x) + 1.0,
            fit.map_y(y) + 2.0,
            2.2,
            INK_RGB,
            &format!("#{} {}", index + 1, operation_name(operation.kind, &operation.id)),
            &mut dropped,
        );
    }

    pdf_legend(&mut content, &layout, &legend, &mut dropped);
    let note = format!("{} operation(s)", document.operations.len());
    pdf_title_block(&mut content, &layout, stock_setup_title_cells(document, id, &note, "PDF"), &mut dropped);
    finish_pdf_page(&content, dropped)
}

/// Renders every cutting chain coloured by its Z level as an SVG depth map.
pub fn depth_map_svg(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    depth_map_svg_with_options(document, report, id, &DrawingOptions::all())
}

/// Renders the cutting chains of the selected operations as SVG.
pub fn depth_map_svg_with_options(
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    options: &DrawingOptions,
) -> Drawing {
    let drawings = project_all(document, report);
    let selected = options.selected(report.operations.len());
    let legend = operation_rows(document, report, &drawings, &selected);
    let layout = SheetLayout::new(legend.len());
    let frame = toolpath_frame(document, &drawings, &selected);
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut svg = svg_open(&layout, "OpenCAM depth map drawing", id);
    push_svg_frame(&mut svg, &layout);
    push_svg_stock(&mut svg, document, &fit);

    let mut labels = LabelPlacer::new(layout.plot);
    for &index in &selected {
        let levels = chain_levels(&drawings[index]);
        svg.push_str(&format!("  <g data-depth-map=\"true\" data-index=\"{index}\">\n"));
        for (level, z) in levels.iter().enumerate() {
            let hex = shade(palette(index).1, shade_factor(level, levels.len())).0;
            for chain in drawings[index].cuts.iter().filter(|chain| level_index(&levels, chain.z) == level) {
                if chain.points.len() < 2 {
                    continue;
                }
                let points = chain
                    .points
                    .iter()
                    .map(|point| format!("{},{}", number(fit.map_x(point.0)), number(fit.map_y(point.1))))
                    .collect::<Vec<_>>()
                    .join(" ");
                svg.push_str(&format!(
                    "    <polyline points=\"{points}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n"
                ));
            }
            if let Some(chain) = drawings[index].cuts.iter().find(|chain| level_index(&levels, chain.z) == level) {
                let (x, y) = chain.points.first().copied().unwrap_or((0.0, 0.0));
                let text = format!("Z{}", number(*z));
                if let Some((x, baseline)) = labels.place(&text, 2.2, (fit.map_x(x) + 0.8, fit.map_y(y) + 2.0)) {
                    push_svg_text(&mut svg, x, baseline, 2.2, INK, &text, layout.plot, None);
                }
            }
        }
        svg.push_str("  </g>\n");
    }

    push_svg_legend(&mut svg, &layout, &legend);
    let note = operation_count_note(options, report.operations.len());
    push_svg_title_block(&mut svg, &layout, depth_map_title_cells(document, report, id, &note, "SVG"));
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the cutting chains as one PDF page.
pub fn depth_map_pdf(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    depth_map_pdf_with_options(document, report, id, &DrawingOptions::all())
}

/// Renders the cutting chains of the selected operations as PDF.
pub fn depth_map_pdf_with_options(
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    options: &DrawingOptions,
) -> Drawing {
    let drawings = project_all(document, report);
    let selected = options.selected(report.operations.len());
    let legend = operation_rows(document, report, &drawings, &selected);
    let layout = SheetLayout::new(legend.len());
    let frame = toolpath_frame(document, &drawings, &selected);
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut dropped = 0usize;
    let mut content = pdf_open(&layout, "OpenCAM depth map drawing", id, &mut dropped);
    pdf_frame(&mut content, &layout);
    pdf_stock(&mut content, document, &fit);

    let mut labels = LabelPlacer::new(layout.plot);
    for &index in &selected {
        let levels = chain_levels(&drawings[index]);
        for (level, z) in levels.iter().enumerate() {
            let colour = shade(palette(index).1, shade_factor(level, levels.len())).1;
            for chain in drawings[index].cuts.iter().filter(|chain| level_index(&levels, chain.z) == level) {
                if chain.points.len() < 2 {
                    continue;
                }
                let points: Vec<(f64, f64)> = chain.points.iter().map(|point| fit.map(*point)).collect();
                pdf_polyline(&mut content, &points, colour, PATH_WIDTH_MM);
            }
            if let Some(chain) = drawings[index].cuts.iter().find(|chain| level_index(&levels, chain.z) == level) {
                let (x, y) = chain.points.first().copied().unwrap_or((0.0, 0.0));
                let text = format!("Z{}", number(*z));
                if let Some((x, baseline)) = labels.place(&text, 2.2, (fit.map_x(x) + 1.0, fit.map_y(y) + 2.0)) {
                    pdf_text_at(&mut content, x, baseline, 2.2, INK_RGB, &text, &mut dropped);
                }
            }
        }
    }

    pdf_legend(&mut content, &layout, &legend, &mut dropped);
    let note = operation_count_note(options, report.operations.len());
    pdf_title_block(&mut content, &layout, depth_map_title_cells(document, report, id, &note, "PDF"), &mut dropped);
    finish_pdf_page(&content, dropped)
}

/// Renders every operation as a measured SVG table.
pub fn operation_sheet_svg(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    operation_sheet_svg_with_options(document, report, id, &DrawingOptions::all())
}

/// Renders the selected operations as a measured SVG table.
pub fn operation_sheet_svg_with_options(
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    options: &DrawingOptions,
) -> Drawing {
    let headers = ["op", "type", "tool", "d mm", "depth", "stepover", "feed", "rpm", "time s", "path mm"];
    let rows = operation_table_rows(document, report, options);
    let note = operation_count_note(options, document.operations.len());
    let mut drawing = svg_table_sheet(
        id,
        "OpenCAM operation sheet (SVG)",
        note,
        &headers,
        &rows,
        &[
            Align::Right,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        "no operations declared",
    );
    drawing.text = drawing.text.replacen("<g data-table=\"true\">", "<g data-table=\"true\" data-operation-table=\"true\">", 1);
    drawing
}

/// Renders every operation as a measured PDF table.
pub fn operation_sheet_pdf(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    operation_sheet_pdf_with_options(document, report, id, &DrawingOptions::all())
}

/// Renders the selected operations as a measured PDF table.
pub fn operation_sheet_pdf_with_options(
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    options: &DrawingOptions,
) -> Drawing {
    let headers = ["op", "type", "tool", "d mm", "depth", "stepover", "feed", "rpm", "time s", "path mm"];
    let rows = operation_table_rows(document, report, options);
    let note = operation_count_note(options, document.operations.len());
    pdf_table_sheet(
        id,
        "OpenCAM operation sheet (PDF)",
        note,
        &headers,
        &rows,
        &[
            Align::Right,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        "no operations declared",
    )
}

/// Renders the stepped stock-removal section along the X centre line as SVG.
pub fn stock_removal_svg(document: &CamDocument, id: &str) -> Drawing {
    stock_removal_svg_with_options(document, id, &DrawingOptions::all())
}

/// Renders the stock-removal section of the selected operations as SVG.
pub fn stock_removal_svg_with_options(document: &CamDocument, id: &str, options: &DrawingOptions) -> Drawing {
    let selected = options.selected(document.operations.len());
    let section = Section::new(document, &selected);
    let legend = section.legend(document, &selected);
    let layout = SheetLayout::new(legend.len());
    let frame = Frame {
        min_x: 0.0,
        min_y: 0.0,
        max_x: document.stock.width.max(1.0),
        max_y: (document.stock.height * SECTION_Z_EXAGGERATION).max(1.0),
    };
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut svg = svg_open(&layout, "OpenCAM stock removal drawing", id);
    push_svg_frame(&mut svg, &layout);
    let x = fit.map_x(0.0);
    let y = fit.map_y(document.stock.height * SECTION_Z_EXAGGERATION);
    let width = document.stock.width * fit.scale;
    let height = document.stock.height * SECTION_Z_EXAGGERATION * fit.scale;
    svg.push_str("  <g data-stock-removal=\"true\">\n");
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#f1f5f9\" stroke=\"{STOCK_COLOR}\" stroke-width=\"0.45\"/>\n",
        number(x),
        number(y),
        number(width),
        number(height)
    ));
    for band in &section.bands {
        let (hex, _) = palette(band.operation);
        svg.push_str(&format!(
            "    <rect data-removal-layer=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{hex}\" fill-opacity=\"0.65\" stroke=\"#ffffff\" stroke-width=\"0.12\"/>\n",
            band.operation + 1,
            number(fit.map_x(band.x0)),
            number(fit.map_y((document.stock.height - band.top_depth) * SECTION_Z_EXAGGERATION)),
            number((band.x1 - band.x0) * fit.scale),
            number((band.bottom_depth - band.top_depth) * SECTION_Z_EXAGGERATION * fit.scale)
        ));
    }
    let profile = section.profile(document.stock.height);
    let points = profile
        .iter()
        .map(|(x, z)| format!("{},{}", number(fit.map_x(*x)), number(fit.map_y(*z * SECTION_Z_EXAGGERATION))))
        .collect::<Vec<_>>()
        .join(" ");
    svg.push_str(&format!(
        "    <polyline data-final-profile=\"true\" points=\"{points}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.7\" stroke-linejoin=\"round\"/>\n"
    ));
    let (allowance, deepest) = section.allowance(document.stock.height);
    push_svg_text(
        &mut svg,
        fit.map_x(0.0) + 2.0,
        fit.map_y(document.stock.height * SECTION_Z_EXAGGERATION) - 4.0,
        2.4,
        INK,
        &format!(
            "section X-X at Y {} mm, stock allowance {} mm, Z x{}",
            number(document.stock.depth / 2.0),
            number(allowance),
            number(SECTION_Z_EXAGGERATION)
        ),
        layout.plot,
        None,
    );
    if deepest > 0.0 {
        push_svg_line(
            &mut svg,
            &fit,
            (document.stock.width, 0.0),
            (document.stock.width, (document.stock.height - deepest) * SECTION_Z_EXAGGERATION),
            INK,
            0.25,
            None,
            None,
        );
    }
    svg.push_str("  </g>\n");

    push_svg_legend(&mut svg, &layout, &legend);
    let note = operation_count_note(options, document.operations.len());
    push_svg_title_block(&mut svg, &layout, stock_removal_title_cells(document, id, &note, "SVG"));
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the stock-removal section as one PDF page.
pub fn stock_removal_pdf(document: &CamDocument, id: &str) -> Drawing {
    stock_removal_pdf_with_options(document, id, &DrawingOptions::all())
}

/// Renders the stock-removal section of the selected operations as PDF.
pub fn stock_removal_pdf_with_options(document: &CamDocument, id: &str, options: &DrawingOptions) -> Drawing {
    let selected = options.selected(document.operations.len());
    let section = Section::new(document, &selected);
    let legend = section.legend(document, &selected);
    let layout = SheetLayout::new(legend.len());
    let frame = Frame {
        min_x: 0.0,
        min_y: 0.0,
        max_x: document.stock.width.max(1.0),
        max_y: (document.stock.height * SECTION_Z_EXAGGERATION).max(1.0),
    };
    let fit = Fit::new(&frame, layout.plot, FIT_MARGIN_FRACTION);

    let mut dropped = 0usize;
    let mut content = pdf_open(&layout, "OpenCAM stock removal drawing", id, &mut dropped);
    pdf_frame(&mut content, &layout);
    let stock_from = (fit.map_x(0.0), fit.map_y(document.stock.height * SECTION_Z_EXAGGERATION));
    let stock_to = (fit.map_x(document.stock.width), fit.map_y(0.0));
    pdf_fill_rect(&mut content, stock_from, stock_to, [241, 245, 249]);
    pdf_rect(&mut content, stock_from, stock_to, STOCK_RGB, 0.45, true);
    for band in &section.bands {
        let colour = palette(band.operation).1;
        pdf_fill_rect(
            &mut content,
            (fit.map_x(band.x0), fit.map_y((document.stock.height - band.top_depth) * SECTION_Z_EXAGGERATION)),
            (fit.map_x(band.x1), fit.map_y((document.stock.height - band.bottom_depth) * SECTION_Z_EXAGGERATION)),
            colour,
        );
    }
    let profile: Vec<(f64, f64)> = section
        .profile(document.stock.height)
        .iter()
        .map(|(x, z)| (fit.map_x(*x), fit.map_y(*z * SECTION_Z_EXAGGERATION)))
        .collect();
    pdf_polyline(&mut content, &profile, INK_RGB, 0.7);
    let (allowance, _) = section.allowance(document.stock.height);
    pdf_text_at(
        &mut content,
        fit.map_x(0.0) + 2.0,
        fit.map_y(document.stock.height * SECTION_Z_EXAGGERATION) - 4.0,
        2.4,
        INK_RGB,
        &format!(
            "section X-X at Y {} mm, stock allowance {} mm, Z x{}",
            number(document.stock.depth / 2.0),
            number(allowance),
            number(SECTION_Z_EXAGGERATION)
        ),
        &mut dropped,
    );

    pdf_legend(&mut content, &layout, &legend, &mut dropped);
    let note = operation_count_note(options, document.operations.len());
    pdf_title_block(&mut content, &layout, stock_removal_title_cells(document, id, &note, "PDF"), &mut dropped);
    finish_pdf_page(&content, dropped)
}

// ---------------------------------------------------------------------------
// Layout primitives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn right(self) -> f64 {
        self.x + self.width
    }

    fn bottom(self) -> f64 {
        self.y + self.height
    }
}

/// Deterministic label placement that keeps annotations apart and inside the
/// plot.
struct LabelPlacer {
    plot: Rect,
    placed: Vec<Rect>,
}

impl LabelPlacer {
    fn new(plot: Rect) -> Self {
        Self { plot, placed: Vec::new() }
    }

    /// Places `text` near `at`, shifting vertically until it neither overlaps a
    /// placed label nor leaves the plot. Reports `None` when every slot is full.
    fn place(&mut self, text: &str, size: f64, at: (f64, f64)) -> Option<(f64, f64)> {
        let width = text_width_mm(text, size);
        let height = size * 1.25;
        let x = at.0.min(self.plot.right() - width).max(self.plot.x);
        for shift in [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0, 5.0, -5.0, 6.0, -6.0] {
            let baseline = at.1 + shift * (height + 0.4);
            let bounds = Rect { x, y: baseline - size, width, height };
            if bounds.x < self.plot.x
                || bounds.right() > self.plot.right() + 1e-9
                || bounds.y < self.plot.y
                || bounds.bottom() > self.plot.bottom() + 1e-9
            {
                continue;
            }
            let overlaps = self.placed.iter().any(|other| {
                bounds.x < other.right() && other.x < bounds.right() && bounds.y < other.bottom() && other.y < bounds.bottom()
            });
            if overlaps {
                continue;
            }
            self.placed.push(bounds);
            return Some((x, baseline));
        }
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct SheetLayout {
    frame: Rect,
    plot: Rect,
    legend: Rect,
    title: Rect,
}

impl SheetLayout {
    fn new(legend_rows: usize) -> Self {
        let frame = Rect {
            x: FRAME_MARGIN_MM,
            y: FRAME_MARGIN_MM,
            width: PAGE_WIDTH_MM - 2.0 * FRAME_MARGIN_MM,
            height: PAGE_HEIGHT_MM - 2.0 * FRAME_MARGIN_MM,
        };
        let title = Rect {
            x: frame.x + 2.0,
            y: frame.bottom() - 1.0 - TITLE_BOTTOM_GAP_MM - TITLE_HEIGHT_MM,
            width: frame.width - 4.0,
            height: TITLE_HEIGHT_MM,
        };
        let legend_height =
            if legend_rows == 0 { 0.0 } else { LEGEND_ROW_MM * (legend_rows.div_ceil(2) as f64) + 2.0 * LEGEND_PAD_MM };
        let legend_y = title.y - legend_height - if legend_height > 0.0 { LEGEND_GAP_MM } else { 0.0 };
        let legend = Rect { x: frame.x + 2.0, y: legend_y, width: frame.width - 4.0, height: legend_height };
        let plot =
            Rect { x: frame.x + 2.0, y: frame.y + 2.0, width: frame.width - 4.0, height: (legend_y - frame.y - 2.0).max(10.0) };
        Self { frame, plot, legend, title }
    }
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Frame {
    fn width(self) -> f64 {
        (self.max_x - self.min_x).max(0.001)
    }

    fn height(self) -> f64 {
        (self.max_y - self.min_y).max(0.001)
    }

    fn include(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }
}

/// Uniform mapping from document millimetres to sheet millimetres.
#[derive(Debug, Clone, Copy)]
struct Fit {
    scale: f64,
    min_x: f64,
    max_y: f64,
    left: f64,
    top: f64,
}

impl Fit {
    fn new(frame: &Frame, area: Rect, margin_fraction: f64) -> Self {
        let factor = 1.0 - margin_fraction;
        let scale = (area.width * factor / frame.width()).min(area.height * factor / frame.height()).max(0.001);
        let left = area.x + (area.width - frame.width() * scale) / 2.0;
        let top = area.y + (area.height - frame.height() * scale) / 2.0;
        Self { scale, min_x: frame.min_x, max_y: frame.max_y, left, top }
    }

    fn map_x(&self, x: f64) -> f64 {
        self.left + (x - self.min_x) * self.scale
    }

    fn map_y(&self, y: f64) -> f64 {
        self.top + (self.max_y - y) * self.scale
    }

    fn map(&self, point: (f64, f64)) -> (f64, f64) {
        (self.map_x(point.0), self.map_y(point.1))
    }
}

// ---------------------------------------------------------------------------
// SVG primitives
// ---------------------------------------------------------------------------

fn svg_open(layout: &SheetLayout, title: &str, id: &str) -> String {
    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {0} {1}\" width=\"{2}\" height=\"{3}\">\n",
        number(PAGE_WIDTH_MM),
        number(PAGE_HEIGHT_MM),
        number(SVG_WIDTH_PX),
        number(SVG_HEIGHT_PX)
    ));
    svg.push_str(&format!("  <title>{title} {}</title>\n", escape_xml(id)));
    svg.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        number(PAGE_WIDTH_MM),
        number(PAGE_HEIGHT_MM)
    ));
    let _ = layout;
    svg
}

fn push_svg_frame(svg: &mut String, layout: &SheetLayout) {
    let frame = layout.frame;
    svg.push_str(&format!(
        "  <g data-frame=\"true\" data-plot=\"{},{},{},{}\">\n",
        number(layout.plot.x),
        number(layout.plot.y),
        number(layout.plot.width),
        number(layout.plot.height)
    ));
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.5\"/>\n",
        number(frame.x),
        number(frame.y),
        number(frame.width),
        number(frame.height)
    ));
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.15\"/>\n",
        number(frame.x + 1.0),
        number(frame.y + 1.0),
        number(frame.width - 2.0),
        number(frame.height - 2.0)
    ));
    svg.push_str("  </g>\n");
}

#[allow(clippy::too_many_arguments)]
fn push_svg_text(svg: &mut String, x: f64, y: f64, size: f64, colour: &str, text: &str, bounds: Rect, anchor: Option<&str>) {
    let anchor = anchor.map_or(String::new(), |value| format!(" text-anchor=\"{value}\""));
    svg.push_str(&format!(
        "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"{}\" fill=\"{colour}\"{anchor} data-box=\"{},{},{},{}\">{}</text>\n",
        number(x),
        number(y),
        number(size),
        number(bounds.x),
        number(bounds.y),
        number(bounds.width),
        number(bounds.height),
        escape_xml(text)
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_svg_line(
    svg: &mut String,
    fit: &Fit,
    from: (f64, f64),
    to: (f64, f64),
    colour: &str,
    width: f64,
    dash: Option<&str>,
    opacity: Option<f64>,
) {
    let dash = dash.map_or(String::new(), |value| format!(" stroke-dasharray=\"{value}\""));
    let opacity = opacity.map_or(String::new(), |value| format!(" stroke-opacity=\"{}\"", number(value)));
    svg.push_str(&format!(
        "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{colour}\" stroke-width=\"{}\"{dash}{opacity} stroke-linecap=\"round\"/>\n",
        number(fit.map_x(from.0)),
        number(fit.map_y(from.1)),
        number(fit.map_x(to.0)),
        number(fit.map_y(to.1)),
        number(width)
    ));
}

fn push_svg_stock(svg: &mut String, document: &CamDocument, fit: &Fit) {
    svg.push_str("  <g data-stock=\"true\">\n");
    svg.push_str(&format!(
        "    <rect data-stock-outline=\"true\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{PAPER_COLOR}\" stroke=\"{STOCK_COLOR}\" stroke-width=\"0.45\"/>\n",
        number(fit.map_x(0.0)),
        number(fit.map_y(document.stock.depth)),
        number(document.stock.width * fit.scale),
        number(document.stock.depth * fit.scale)
    ));
    svg.push_str("  </g>\n");
}

fn push_svg_islands(svg: &mut String, document: &CamDocument, fit: &Fit, selected: &[usize]) {
    let islands: Vec<&Island> = document
        .operations
        .iter()
        .enumerate()
        .filter(|(index, operation)| selected.contains(index) && operation.kind == OperationKind::Pocket)
        .flat_map(|(_, operation)| &operation.islands)
        .collect();
    if islands.is_empty() {
        return;
    }
    svg.push_str("  <g data-islands=\"true\">\n");
    for island in islands {
        if island.shape.as_deref() == Some("circle") {
            svg.push_str(&format!(
                "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{ISLAND_COLOR}\" stroke-width=\"{ISLAND_WIDTH_MM}\"/>\n",
                number(fit.map_x(island.x + island.width / 2.0)),
                number(fit.map_y(island.y + island.height / 2.0)),
                number(island.width / 2.0 * fit.scale)
            ));
        }
        else {
            let points = [
                (island.x, island.y),
                (island.x + island.width, island.y),
                (island.x + island.width, island.y + island.height),
                (island.x, island.y + island.height),
            ]
            .into_iter()
            .map(|(x, y)| format!("{},{}", number(fit.map_x(x)), number(fit.map_y(y))))
            .collect::<Vec<_>>()
            .join(" ");
            svg.push_str(&format!(
                "    <polygon points=\"{points}\" fill=\"none\" stroke=\"{ISLAND_COLOR}\" stroke-width=\"{ISLAND_WIDTH_MM}\" stroke-linejoin=\"round\"/>\n"
            ));
        }
    }
    svg.push_str("  </g>\n");
}

fn push_svg_operation(
    svg: &mut String,
    document: &CamDocument,
    report: &ToolpathReport,
    drawing: &OpDrawing,
    index: usize,
    fit: &Fit,
) {
    let operation = &report.operations[index];
    let (hex, colour) = palette(index);
    let is_fill = matches!(operation.kind, OperationKind::Face | OperationKind::Pocket);
    svg.push_str(&format!(
        "  <g data-operation=\"{}\" data-index=\"{index}\" data-tool=\"{}\">\n",
        escape_xml(&operation.operation),
        escape_xml(&operation.tool)
    ));
    let levels = chain_levels(drawing);
    let shades: Vec<String> = (0..levels.len())
        .map(|level| {
            let factor =
                if is_fill { 0.5 + 0.3 * shade_factor(level, levels.len()) } else { shade_factor(level, levels.len()) };
            shade(colour, factor).0
        })
        .collect();
    for chain in &drawing.cuts {
        if chain.points.len() < 2 {
            continue;
        }
        let stroke = &shades[level_index(&levels, chain.z)];
        let points = chain
            .points
            .iter()
            .map(|(x, y)| format!("{},{}", number(fit.map_x(*x)), number(fit.map_y(*y))))
            .collect::<Vec<_>>()
            .join(" ");
        let width = if is_fill { FILL_WIDTH_MM } else { PATH_WIDTH_MM };
        let opacity = if is_fill { " stroke-opacity=\"0.45\"" } else { "" };
        svg.push_str(&format!(
            "    <polyline points=\"{points}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{width}\"{opacity} stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n"
        ));
    }
    for (from, to) in &drawing.tabs {
        push_svg_line(svg, fit, *from, *to, TAB_COLOR, TAB_WIDTH_MM, None, None);
    }
    if let Some(radius) = drawing.hole_radius {
        for (x, y) in &drawing.holes {
            svg.push_str(&format!(
                "    <circle data-drill-hole=\"true\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{DRILL_RING_WIDTH_MM}\"/>\n",
                number(fit.map_x(*x)),
                number(fit.map_y(*y)),
                number(radius * fit.scale)
            ));
            svg.push_str(&format!(
                "    <circle cx=\"{}\" cy=\"{}\" r=\"0.22\" fill=\"{hex}\"/>\n",
                number(fit.map_x(*x)),
                number(fit.map_y(*y))
            ));
        }
    }
    for (x, y) in &drawing.plunges {
        svg.push_str(&format!(
            "    <circle data-plunge=\"true\" cx=\"{}\" cy=\"{}\" r=\"0.42\" fill=\"{hex}\" stroke=\"#ffffff\" stroke-width=\"0.12\"/>\n",
            number(fit.map_x(*x)),
            number(fit.map_y(*y))
        ));
    }
    if let Some((x, y)) = drawing.start {
        svg.push_str(&format!(
            "    <circle data-start=\"true\" cx=\"{}\" cy=\"{}\" r=\"1.15\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"0.3\"/>\n",
            number(fit.map_x(x)),
            number(fit.map_y(y))
        ));
    }
    let _ = document;
    svg.push_str("  </g>\n");
}

fn push_svg_setup_geometry(svg: &mut String, document: &CamDocument, operation: &Operation, hex: &str, fit: &Fit) {
    match operation.kind {
        OperationKind::Face => svg.push_str(&format!(
            "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"0.3\" stroke-dasharray=\"1.5 1\"/>\n",
            number(fit.map_x(0.0)),
            number(fit.map_y(document.stock.depth)),
            number(document.stock.width * fit.scale),
            number(document.stock.depth * fit.scale)
        )),
        OperationKind::Contour | OperationKind::Pocket => {
            if operation.shape.as_deref() == Some("circle") {
                svg.push_str(&format!(
                    "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\"/>\n",
                    number(fit.map_x(operation.x + operation.width / 2.0)),
                    number(fit.map_y(operation.y + operation.height / 2.0)),
                    number(operation.width / 2.0 * fit.scale)
                ));
            }
            else {
                svg.push_str(&format!(
                    "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\"/>\n",
                    number(fit.map_x(operation.x)),
                    number(fit.map_y(operation.y + operation.height)),
                    number(operation.width * fit.scale),
                    number(operation.height * fit.scale)
                ));
            }
            for island in &operation.islands {
                svg.push_str(&format!(
                    "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{ISLAND_COLOR}\" stroke-width=\"{ISLAND_WIDTH_MM}\"/>\n",
                    number(fit.map_x(island.x)),
                    number(fit.map_y(island.y + island.height)),
                    number(island.width * fit.scale),
                    number(island.height * fit.scale)
                ));
            }
        }
        OperationKind::Engrave => {
            if operation.path.len() >= 2 {
                let mut points: Vec<String> =
                    operation.path.iter().map(|point| format!("{},{}", number(fit.map_x(point.x)), number(fit.map_y(point.y)))).collect();
                if operation.closed && points.len() > 2 {
                    let first = points[0].clone();
                    points.push(first);
                }
                svg.push_str(&format!(
                    "    <polyline points=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n",
                    points.join(" ")
                ));
            }
        }
        OperationKind::Drill => {
            let radius = document.tools.iter().find(|tool| tool.id == operation.tool).map(Tool::radius);
            for hole in operation.resolved_holes() {
                if let Some(radius) = radius {
                    svg.push_str(&format!(
                        "    <circle data-drill-hole=\"true\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{DRILL_RING_WIDTH_MM}\"/>\n",
                        number(fit.map_x(hole.x)),
                        number(fit.map_y(hole.y)),
                        number(radius * fit.scale)
                    ));
                }
                else {
                    svg.push_str(&format!(
                        "    <circle cx=\"{}\" cy=\"{}\" r=\"0.5\" fill=\"{hex}\"/>\n",
                        number(fit.map_x(hole.x)),
                        number(fit.map_y(hole.y))
                    ));
                }
            }
        }
    }
}

fn push_svg_legend(svg: &mut String, layout: &SheetLayout, rows: &[LegendRow]) {
    svg.push_str("  <g data-legend=\"true\">\n");
    if rows.is_empty() {
        svg.push_str("  </g>\n");
        return;
    }
    svg.push_str(&format!(
        "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{MUTED}\" stroke-width=\"0.2\"/>\n",
        number(layout.legend.x),
        number(layout.legend.y - 1.0),
        number(layout.legend.right()),
        number(layout.legend.y - 1.0)
    ));
    let per_column = rows.len().div_ceil(2);
    let column_width = layout.legend.width / 2.0;
    for (index, entry) in rows.iter().enumerate() {
        let column = index / per_column;
        let row = index % per_column;
        let left = layout.legend.x + column as f64 * column_width;
        let baseline = layout.legend.y + LEGEND_PAD_MM + 2.4 + row as f64 * LEGEND_ROW_MM;
        push_svg_legend_sample(svg, left + 2.0, baseline, entry, "0.6");
        let bounds = Rect { x: left + 6.0, y: baseline - 2.0, width: column_width - 8.0, height: LEGEND_ROW_MM };
        let size = fitted_font(&entry.label, LEGEND_FONT_MM, bounds.width);
        push_svg_text(svg, bounds.x, baseline, size, INK, &entry.label, bounds, None);
    }
    svg.push_str("  </g>\n");
}

fn push_svg_legend_sample(svg: &mut String, x: f64, baseline: f64, entry: &LegendRow, width: &str) {
    let y = baseline - 0.8;
    match entry.style {
        Style::Solid => svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{width}\"/>\n",
            number(x),
            number(y),
            number(x + LEGEND_SAMPLE_MM),
            number(y),
            entry.colour.hex
        )),
        Style::Dashed => svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.45\" stroke-dasharray=\"0.8 1.2\"/>\n",
            number(x),
            number(y),
            number(x + LEGEND_SAMPLE_MM),
            number(y),
            entry.colour.hex
        )),
        Style::Dot => svg.push_str(&format!(
            "    <circle cx=\"{}\" cy=\"{}\" r=\"0.45\" fill=\"{}\"/>\n",
            number(x + LEGEND_SAMPLE_MM / 2.0),
            number(y),
            entry.colour.hex
        )),
        Style::Ring => svg.push_str(&format!(
            "    <circle cx=\"{}\" cy=\"{}\" r=\"1.2\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.4\"/>\n",
            number(x + LEGEND_SAMPLE_MM / 2.0),
            number(y),
            entry.colour.hex
        )),
    }
}

fn push_svg_title_block(svg: &mut String, layout: &SheetLayout, cells: Vec<TitleCell>) {
    let block = title_layout(layout, &cells);
    svg.push_str("  <g data-title-block=\"true\">\n");
    svg.push_str(&format!(
        "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
        number(layout.title.x),
        number(layout.title.y),
        number(layout.title.right()),
        number(layout.title.y)
    ));
    for right in &block.dividers {
        svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
            number(*right),
            number(layout.title.y),
            number(*right),
            number(layout.title.bottom())
        ));
    }
    for line in &block.lines {
        if line.text.is_empty() {
            continue;
        }
        push_svg_text(svg, line.x, line.baseline, line.size, line.colour, &line.text, line.bounds, None);
    }
    svg.push_str("  </g>\n");
}

fn svg_table_sheet(
    id: &str,
    subtitle: &str,
    note: String,
    headers: &[&str],
    rows: &[Vec<String>],
    aligns: &[Align],
    empty: &str,
) -> Drawing {
    let layout = SheetLayout::new(0);
    let mut svg = svg_open(&layout, subtitle, id);
    push_svg_frame(&mut svg, &layout);
    let table_rows: Vec<Vec<String>> = if rows.is_empty() {
        let mut row = vec![String::new(); headers.len()];
        row[0] = empty.to_string();
        vec![row]
    }
    else {
        rows.to_vec()
    };
    let measured = measure_table(headers, &table_rows, layout.plot.width - 2.0 * TABLE_INSET_MM, TABLE_FONT_MM, 2.0);
    let left = layout.plot.x + TABLE_INSET_MM;
    let top = layout.plot.y + TABLE_INSET_MM + 3.4;
    let width: f64 = measured.widths.iter().sum();
    let header_cells: Vec<Vec<String>> = vec![headers.iter().map(|header| (*header).to_string()).collect()];
    push_svg_table(&mut svg, &table_rows, &header_cells, aligns, left, top, &measured.widths, measured.scale);
    let height = TABLE_ROW_MM * (table_rows.len() as f64 + 1.0);
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.35\"/>\n",
        number(left),
        number(top - 3.4),
        number(width),
        number(height + 3.4)
    ));
    push_svg_title_block(&mut svg, &layout, table_title_cells(id, subtitle, &note));
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

#[allow(clippy::too_many_arguments)]
fn push_svg_table(
    svg: &mut String,
    rows: &[Vec<String>],
    headers: &[Vec<String>],
    aligns: &[Align],
    left: f64,
    top: f64,
    widths: &[f64],
    font_scale: f64,
) {
    let mut column_x = Vec::with_capacity(widths.len());
    let mut x = left;
    for width in widths {
        column_x.push(x);
        x += width;
    }
    let total: f64 = widths.iter().sum();
    svg.push_str("  <g data-table=\"true\">\n");
    for (row_index, cells) in
        headers.iter().enumerate().chain(rows.iter().enumerate().map(|(index, cells)| (index + headers.len(), cells)))
    {
        let baseline = top + row_index as f64 * TABLE_ROW_MM;
        for (column, text) in cells.iter().enumerate() {
            if column >= widths.len() {
                break;
            }
            let bounds = Rect { x: column_x[column], y: baseline - 3.2, width: widths[column], height: TABLE_ROW_MM };
            let (x, anchor) = match aligns.get(column).copied().unwrap_or(Align::Left) {
                Align::Left => (bounds.x + 1.2, None),
                Align::Right => (bounds.right() - 1.2, Some("end")),
            };
            push_svg_text(svg, x, baseline, TABLE_FONT_MM * font_scale, INK, text, bounds, anchor);
        }
        let y = baseline + 1.6;
        svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{MUTED}\" stroke-width=\"0.2\"/>\n",
            number(left),
            number(y),
            number(left + total),
            number(y)
        ));
    }
    svg.push_str("  </g>\n");
}

// ---------------------------------------------------------------------------
// PDF primitives
// ---------------------------------------------------------------------------

fn pdf_open(layout: &SheetLayout, title: &str, id: &str, dropped: &mut usize) -> String {
    let _ = (layout, title, id, dropped);
    let mut content = String::new();
    content.push_str(&format!("1 1 1 rg 0 0 {PDF_PAGE_WIDTH:.2} {PDF_PAGE_HEIGHT:.2} re f\n"));
    content
}

fn pdf_frame(content: &mut String, layout: &SheetLayout) {
    let frame = layout.frame;
    pdf_rect(content, (frame.x, frame.bottom()), (frame.right(), frame.y), INK_RGB, 0.5, true);
    pdf_rect(content, (frame.x + 1.0, frame.bottom() - 1.0), (frame.right() - 1.0, frame.y + 1.0), INK_RGB, 0.15, true);
}

fn pdf_stock(content: &mut String, document: &CamDocument, fit: &Fit) {
    let from = (fit.map_x(0.0), fit.map_y(document.stock.depth));
    let to = (fit.map_x(document.stock.width), fit.map_y(0.0));
    pdf_fill_rect(content, from, to, [251, 252, 254]);
    pdf_rect(content, from, to, STOCK_RGB, 0.45, true);
}

fn pdf_islands(content: &mut String, document: &CamDocument, fit: &Fit) {
    for operation in &document.operations {
        if operation.kind != OperationKind::Pocket {
            continue;
        }
        for island in &operation.islands {
            if island.shape.as_deref() == Some("circle") {
                pdf_circle(
                    content,
                    fit.map_x(island.x + island.width / 2.0),
                    fit.map_y(island.y + island.height / 2.0),
                    island.width / 2.0 * fit.scale,
                    ISLAND_RGB,
                    true,
                );
            }
            else {
                pdf_rect(
                    content,
                    (fit.map_x(island.x), fit.map_y(island.y + island.height)),
                    (fit.map_x(island.x + island.width), fit.map_y(island.y)),
                    ISLAND_RGB,
                    0.35,
                    true,
                );
            }
        }
    }
}

fn pdf_operation(
    content: &mut String,
    document: &CamDocument,
    drawing: &OpDrawing,
    index: usize,
    fit: &Fit,
    dropped: &mut usize,
) {
    let operation = &document.operations[index];
    let colour = palette(index).1;
    let is_fill = matches!(operation.kind, OperationKind::Face | OperationKind::Pocket);
    let levels = chain_levels(drawing);
    let shades: Vec<[u8; 3]> = (0..levels.len())
        .map(|level| {
            let factor =
                if is_fill { 0.5 + 0.3 * shade_factor(level, levels.len()) } else { shade_factor(level, levels.len()) };
            shade(colour, factor).1
        })
        .collect();
    for chain in &drawing.cuts {
        if chain.points.len() < 2 {
            continue;
        }
        let width = if is_fill { FILL_WIDTH_MM } else { PATH_WIDTH_MM };
        let points: Vec<(f64, f64)> = chain.points.iter().map(|point| fit.map(*point)).collect();
        pdf_polyline(content, &points, shades[level_index(&levels, chain.z)], width);
    }
    for (from, to) in &drawing.tabs {
        pdf_line(content, fit.map(*from), fit.map(*to), TAB_RGB, TAB_WIDTH_MM, None);
    }
    if let Some(radius) = drawing.hole_radius {
        for (x, y) in &drawing.holes {
            content.push_str(&format!("% drill hole r={}\n", number(radius)));
            pdf_circle(content, fit.map_x(*x), fit.map_y(*y), radius * fit.scale, colour, true);
        }
    }
    for (x, y) in &drawing.plunges {
        pdf_circle(content, fit.map_x(*x), fit.map_y(*y), 0.42, colour, false);
    }
    if let Some((x, y)) = drawing.start {
        pdf_circle(content, fit.map_x(x), fit.map_y(y), 1.15, colour, true);
        pdf_text_at(content, fit.map_x(x) + 1.6, fit.map_y(y) + 1.0, 2.0, colour, &format!("#{}", index + 1), dropped);
    }
}

fn pdf_setup_geometry(content: &mut String, document: &CamDocument, operation: &Operation, colour: [u8; 3], fit: &Fit) {
    match operation.kind {
        OperationKind::Face => pdf_rect(
            content,
            (fit.map_x(0.0), fit.map_y(document.stock.depth)),
            (fit.map_x(document.stock.width), fit.map_y(0.0)),
            colour,
            0.35,
            true,
        ),
        OperationKind::Contour | OperationKind::Pocket => {
            if operation.shape.as_deref() == Some("circle") {
                pdf_circle(
                    content,
                    fit.map_x(operation.x + operation.width / 2.0),
                    fit.map_y(operation.y + operation.height / 2.0),
                    operation.width / 2.0 * fit.scale,
                    colour,
                    true,
                );
            }
            else {
                pdf_rect(
                    content,
                    (fit.map_x(operation.x), fit.map_y(operation.y + operation.height)),
                    (fit.map_x(operation.x + operation.width), fit.map_y(operation.y)),
                    colour,
                    0.5,
                    true,
                );
            }
            for island in &operation.islands {
                pdf_rect(
                    content,
                    (fit.map_x(island.x), fit.map_y(island.y + island.height)),
                    (fit.map_x(island.x + island.width), fit.map_y(island.y)),
                    ISLAND_RGB,
                    0.35,
                    true,
                );
            }
        }
        OperationKind::Engrave => {
            if operation.path.len() >= 2 {
                let points: Vec<(f64, f64)> = operation.path.iter().map(|point| fit.map((point.x, point.y))).collect();
                pdf_polyline(content, &points, colour, PATH_WIDTH_MM);
            }
        }
        OperationKind::Drill => {
            let radius = document.tools.iter().find(|tool| tool.id == operation.tool).map(Tool::radius);
            for hole in operation.resolved_holes() {
                pdf_circle(
                    content,
                    fit.map_x(hole.x),
                    fit.map_y(hole.y),
                    radius.map_or(0.5, |value| value * fit.scale),
                    colour,
                    radius.is_some(),
                );
            }
        }
    }
}

fn pdf_legend(content: &mut String, layout: &SheetLayout, rows: &[LegendRow], dropped: &mut usize) {
    if rows.is_empty() {
        return;
    }
    pdf_line(
        content,
        (layout.legend.x, layout.legend.y - 1.0),
        (layout.legend.right(), layout.legend.y - 1.0),
        MUTED_RGB,
        0.2,
        None,
    );
    let per_column = rows.len().div_ceil(2);
    let column_width = layout.legend.width / 2.0;
    for (index, entry) in rows.iter().enumerate() {
        let column = index / per_column;
        let row = index % per_column;
        let left = layout.legend.x + column as f64 * column_width;
        let baseline = layout.legend.y + LEGEND_PAD_MM + 2.4 + row as f64 * LEGEND_ROW_MM;
        match entry.style {
            Style::Solid => pdf_line(
                content,
                (left + 2.0, baseline - 0.8),
                (left + 2.0 + LEGEND_SAMPLE_MM, baseline - 0.8),
                entry.colour.rgb,
                0.5,
                None,
            ),
            Style::Dashed => pdf_line(
                content,
                (left + 2.0, baseline - 0.8),
                (left + 2.0 + LEGEND_SAMPLE_MM, baseline - 0.8),
                entry.colour.rgb,
                0.4,
                Some("0.8 1.2"),
            ),
            Style::Dot => {
                pdf_circle(content, left + 2.0 + LEGEND_SAMPLE_MM / 2.0, baseline - 0.8, 0.45, entry.colour.rgb, false)
            }
            Style::Ring => {
                pdf_circle(content, left + 2.0 + LEGEND_SAMPLE_MM / 2.0, baseline - 0.8, 1.2, entry.colour.rgb, true)
            }
        }
        let size = fitted_font(&entry.label, LEGEND_FONT_MM, column_width - 8.0);
        pdf_text_at(content, left + 6.0, baseline, size, INK_RGB, &entry.label, dropped);
    }
}

fn pdf_title_block(content: &mut String, layout: &SheetLayout, cells: Vec<TitleCell>, dropped: &mut usize) {
    let block = title_layout(layout, &cells);
    pdf_line(content, (layout.title.x, layout.title.y), (layout.title.right(), layout.title.y), INK_RGB, 0.4, None);
    for right in &block.dividers {
        pdf_line(content, (*right, layout.title.y), (*right, layout.title.bottom()), INK_RGB, 0.4, None);
    }
    for line in &block.lines {
        pdf_text_at(content, line.x, line.baseline, line.size, line.colour_rgb, &line.text, dropped);
    }
}

fn pdf_line(content: &mut String, from: (f64, f64), to: (f64, f64), colour: [u8; 3], width: f64, dash: Option<&str>) {
    let (x1, y1) = pdf_point(from.0, from.1);
    let (x2, y2) = pdf_point(to.0, to.1);
    match dash {
        Some(pattern) => content.push_str(&format!(
            "{} RG {:.2} w [{pattern}] 0 d {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S [] 0 d\n",
            rgb(colour),
            width * MM_TO_PT
        )),
        None => content.push_str(&format!(
            "{} RG {:.2} w 1 J {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S\n",
            rgb(colour),
            width * MM_TO_PT
        )),
    }
}

fn pdf_polyline(content: &mut String, points: &[(f64, f64)], colour: [u8; 3], width: f64) {
    if points.len() < 2 {
        return;
    }
    content.push_str(&format!("{} RG {:.2} w 1 J 1 j ", rgb(colour), width * MM_TO_PT));
    for (position, point) in points.iter().enumerate() {
        let (x, y) = pdf_point(point.0, point.1);
        let operator = if position == 0 { "m" } else { "l" };
        content.push_str(&format!("{x:.2} {y:.2} {operator} "));
    }
    content.push_str("S\n");
}

fn pdf_rect(content: &mut String, from: (f64, f64), to: (f64, f64), colour: [u8; 3], width: f64, stroke: bool) {
    let (x1, y1) = pdf_point(from.0, from.1);
    let (x2, y2) = pdf_point(to.0, to.1);
    let left = x1.min(x2);
    let bottom = y1.min(y2);
    let right = x1.max(x2);
    let top = y1.max(y2);
    if stroke {
        content.push_str(&format!(
            "{} RG {:.2} w {left:.2} {bottom:.2} m {right:.2} {bottom:.2} l {right:.2} {top:.2} l {left:.2} {top:.2} l h S\n",
            rgb(colour),
            width * MM_TO_PT
        ));
    }
    else {
        content.push_str(&format!("{} rg {left:.2} {bottom:.2} {:.2} {:.2} re f\n", rgb(colour), right - left, top - bottom));
    }
}

fn pdf_fill_rect(content: &mut String, from: (f64, f64), to: (f64, f64), colour: [u8; 3]) {
    let (x1, y1) = pdf_point(from.0, from.1);
    let (x2, y2) = pdf_point(to.0, to.1);
    let left = x1.min(x2);
    let bottom = y1.min(y2);
    content.push_str(&format!("{} rg {left:.2} {bottom:.2} {:.2} {:.2} re f\n", rgb(colour), (x1 - x2).abs(), (y1 - y2).abs()));
}

fn pdf_circle(content: &mut String, x: f64, y: f64, radius: f64, colour: [u8; 3], stroke_only: bool) {
    let (cx, cy) = pdf_point(x, y);
    let radius = radius * MM_TO_PT;
    let k = radius * 0.552_284_749_8;
    let mut path = String::new();
    path.push_str(&format!("{:.2} {:.2} m ", cx - radius, cy));
    path.push_str(&format!(
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c ",
        cx - radius,
        cy + k,
        cx - k,
        cy + radius,
        cx,
        cy + radius
    ));
    path.push_str(&format!(
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c ",
        cx + k,
        cy + radius,
        cx + radius,
        cy + k,
        cx + radius,
        cy
    ));
    path.push_str(&format!(
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c ",
        cx + radius,
        cy - k,
        cx + k,
        cy - radius,
        cx,
        cy - radius
    ));
    path.push_str(&format!(
        "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h",
        cx - k,
        cy - radius,
        cx - radius,
        cy - k,
        cx - radius,
        cy
    ));
    if stroke_only {
        content.push_str(&format!("{} RG 0.9 w {path} S\n", rgb(colour)));
    }
    else {
        content.push_str(&format!("{} rg {path} f\n", rgb(colour)));
    }
}

fn pdf_text_at(content: &mut String, x: f64, y: f64, size: f64, colour: [u8; 3], text: &str, dropped: &mut usize) {
    let (literal, count) = pdf_text(text);
    *dropped += count;
    let (x, y) = pdf_point(x, y);
    content.push_str(&format!("{} rg BT /F1 {:.2} Tf {x:.2} {y:.2} Td {literal} Tj ET\n", rgb(colour), size * MM_TO_PT));
}

fn pdf_point(x: f64, y: f64) -> (f64, f64) {
    (x * MM_TO_PT, (PAGE_HEIGHT_MM - y) * MM_TO_PT)
}

fn finish_pdf_page(content: &str, dropped: usize) -> Drawing {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PDF_PAGE_WIDTH:.0} {PDF_PAGE_HEIGHT:.0}] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    pdf.push_str("0000000000 65535 f \n");
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
    Drawing { text: pdf, dropped }
}

fn pdf_table_sheet(
    id: &str,
    subtitle: &str,
    note: String,
    headers: &[&str],
    rows: &[Vec<String>],
    aligns: &[Align],
    empty: &str,
) -> Drawing {
    let layout = SheetLayout::new(0);
    let table_rows: Vec<Vec<String>> = if rows.is_empty() {
        let mut row = vec![String::new(); headers.len()];
        row[0] = empty.to_string();
        vec![row]
    }
    else {
        rows.to_vec()
    };
    let measured = measure_table(headers, &table_rows, layout.plot.width - 2.0 * TABLE_INSET_MM, TABLE_FONT_MM, 2.0);
    let mut dropped = 0usize;
    let mut content = pdf_open(&layout, subtitle, id, &mut dropped);
    pdf_frame(&mut content, &layout);
    let left = layout.plot.x + TABLE_INSET_MM;
    let top = layout.plot.y + TABLE_INSET_MM + 3.4;
    let width: f64 = measured.widths.iter().sum();
    let all_rows: Vec<Vec<String>> = std::iter::once(headers.iter().map(|header| (*header).to_string()).collect())
        .chain(table_rows.iter().cloned())
        .collect();
    let mut column_x = Vec::with_capacity(measured.widths.len());
    let mut x = left;
    for cell_width in &measured.widths {
        column_x.push(x);
        x += cell_width;
    }
    for (row_index, cells) in all_rows.iter().enumerate() {
        let baseline = top + row_index as f64 * TABLE_ROW_MM;
        for (column, text) in cells.iter().enumerate() {
            if column >= measured.widths.len() {
                break;
            }
            let text_width = text_width_mm(text, TABLE_FONT_MM * measured.scale);
            let x = match aligns.get(column).copied().unwrap_or(Align::Left) {
                Align::Left => column_x[column] + 1.2,
                Align::Right => column_x[column] + measured.widths[column] - 1.2 - text_width,
            };
            pdf_text_at(&mut content, x, baseline, TABLE_FONT_MM * measured.scale, INK_RGB, text, &mut dropped);
        }
        pdf_line(&mut content, (left, baseline + 1.6), (left + width, baseline + 1.6), MUTED_RGB, 0.2, None);
    }
    let height = TABLE_ROW_MM * (all_rows.len() as f64) + 3.4;
    pdf_rect(&mut content, (left, top - 3.4 + height), (left + width, top - 3.4), INK_RGB, 0.35, true);
    pdf_title_block(&mut content, &layout, table_title_cells(id, subtitle, &note), &mut dropped);
    finish_pdf_page(&content, dropped)
}

// ---------------------------------------------------------------------------
// Legend and title content
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy)]
enum Style {
    Solid,
    Dashed,
    Dot,
    Ring,
}

#[derive(Clone)]
struct RowColour {
    hex: String,
    rgb: [u8; 3],
}

struct LegendRow {
    colour: RowColour,
    style: Style,
    label: String,
}

struct CellLine {
    text: String,
    size: f64,
    colour: &'static str,
    colour_rgb: [u8; 3],
}

impl CellLine {
    fn ink(text: impl Into<String>, size: f64) -> Self {
        Self { text: text.into(), size, colour: INK, colour_rgb: INK_RGB }
    }

    fn muted(text: impl Into<String>, size: f64) -> Self {
        Self { text: text.into(), size, colour: MUTED, colour_rgb: MUTED_RGB }
    }
}

struct TitleCell {
    lines: Vec<CellLine>,
}

struct MeasuredTable {
    widths: Vec<f64>,
    scale: f64,
}

struct TitleLine {
    text: String,
    size: f64,
    colour: &'static str,
    colour_rgb: [u8; 3],
    x: f64,
    baseline: f64,
    bounds: Rect,
}

struct TitleLayout {
    dividers: Vec<f64>,
    lines: Vec<TitleLine>,
}

/// Distributes the title width over the cells and fits every line inside its
/// cell.
///
/// Cells keep their measured proportions. A line that is too wide for its cell
/// shrinks, down to [`TITLE_MIN_FONT_MM`], and wraps at that floor, so no title
/// text can cross a cell divider or the frame.
fn title_layout(layout: &SheetLayout, cells: &[TitleCell]) -> TitleLayout {
    let natural: Vec<f64> = cells
        .iter()
        .map(|cell| {
            cell.lines.iter().map(|line| text_width_mm(&line.text, line.size)).fold(0.0_f64, f64::max) + 2.0 * TITLE_PAD_MM
        })
        .collect();
    let total: f64 = natural.iter().sum();
    let widths: Vec<f64> = if total <= layout.title.width {
        let extra = layout.title.width - total;
        natural.iter().map(|width| width + extra * width / total.max(1.0)).collect()
    }
    else {
        // Keep every cell usable: the shortest cell still gets a third of an
        // even share, and the widest cell gives up the difference.
        let share = layout.title.width / (cells.len().max(1) as f64 * 3.0);
        let mut widths: Vec<f64> = natural.iter().map(|width| (width * layout.title.width / total).max(share)).collect();
        let mut excess = widths.iter().sum::<f64>() - layout.title.width;
        while excess > 1e-9 {
            let mut widest = 0;
            for index in 1..widths.len() {
                if widths[index] > widths[widest] {
                    widest = index;
                }
            }
            let room = widths[widest] - share;
            if room <= 1e-9 {
                break;
            }
            let take = excess.min(room);
            widths[widest] -= take;
            excess -= take;
        }
        widths
    };

    let slot = (layout.title.height - 2.0 * TITLE_PAD_MM) / 2.0;
    let mut dividers = Vec::with_capacity(widths.len());
    let mut lines = Vec::new();
    let mut x = layout.title.x;
    for (cell, width) in cells.iter().zip(&widths) {
        let pad = TITLE_PAD_MM.min(width * 0.25);
        let room = (width - 2.0 * pad).max(0.1);
        for (index, entry) in cell.lines.iter().enumerate() {
            let slot_top = layout.title.y + TITLE_PAD_MM + index as f64 * slot;
            let (text_lines, size) = fit_title_line(&entry.text, entry.size, room);
            let count = text_lines.len().max(1) as f64;
            let step = (size * 1.35).min(slot / count);
            let band = (count - 1.0) * step;
            let first = slot_top + (slot - band) / 2.0 + 0.3 * size;
            for (line_index, text) in text_lines.into_iter().enumerate() {
                let baseline = first + line_index as f64 * step;
                lines.push(TitleLine {
                    bounds: Rect { x: x + pad, y: baseline - size, width: room, height: size * 1.35 },
                    text,
                    size,
                    colour: entry.colour,
                    colour_rgb: entry.colour_rgb,
                    x: x + pad,
                    baseline,
                });
            }
        }
        x += width;
        dividers.push(x);
    }
    TitleLayout { dividers, lines }
}

/// Fits `text` into `room` millimetres, shrinking or wrapping at the font floor.
fn fit_title_line(text: &str, size: f64, room: f64) -> (Vec<String>, f64) {
    let room = room * TITLE_FIT_FRACTION;
    let em = text_width_mm(text, 1.0);
    if em <= 0.0 {
        return (vec![String::new()], size);
    }
    if em * size <= room {
        return (vec![text.to_string()], size);
    }
    let fitted = room / em;
    if fitted >= TITLE_MIN_FONT_MM {
        return (vec![text.to_string()], fitted.min(size));
    }
    (wrap_title_line(text, TITLE_MIN_FONT_MM, room), TITLE_MIN_FONT_MM)
}

/// Greedy wrap that keeps every visual line inside `room`; the tail is trimmed
/// with an ellipsis if it would need more than four lines.
fn wrap_title_line(text: &str, size: f64, room: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
        if text_width_mm(&candidate, size) <= room {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        for character in word.chars() {
            if !current.is_empty() {
                let mut candidate = current.clone();
                candidate.push(character);
                if text_width_mm(&candidate, size) > room {
                    lines.push(std::mem::take(&mut current));
                }
            }
            current.push(character);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    while lines.len() > 4 {
        let tail = lines.pop().unwrap_or_default();
        if let Some(last) = lines.last_mut() {
            last.push(' ');
            last.push_str(&tail);
        }
    }
    if let Some(last) = lines.last_mut() {
        if text_width_mm(last, size) > room {
            let mut kept = String::new();
            for character in last.chars() {
                let mut candidate = kept.clone();
                candidate.push(character);
                candidate.push('…');
                if text_width_mm(&candidate, size) <= room {
                    kept.push(character);
                }
            }
            kept.push('…');
            *last = if text_width_mm(&kept, size) <= room { kept } else { String::new() };
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn measure_table(headers: &[&str], rows: &[Vec<String>], available: f64, font: f64, pad: f64) -> MeasuredTable {
    let mut widths = vec![0.0_f64; headers.len()];
    for (index, header) in headers.iter().enumerate() {
        widths[index] = text_width_mm(header, font) + 2.0 * pad;
    }
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if index < widths.len() {
                widths[index] = widths[index].max(text_width_mm(cell, font) + 2.0 * pad);
            }
        }
    }
    let total: f64 = widths.iter().sum();
    if total > available {
        let scale = available / total;
        return MeasuredTable { widths: widths.iter().map(|width| width * scale).collect(), scale };
    }
    let extra = (available - total) / widths.len().max(1) as f64;
    MeasuredTable { widths: widths.iter().map(|width| width + extra).collect(), scale: 1.0 }
}

fn fitted_font(text: &str, size: f64, width: f64) -> f64 {
    let natural = text_width_mm(text, size);
    if natural <= width || natural <= 0.0 { size } else { size * width / natural }
}

fn palette(index: usize) -> (&'static str, [u8; 3]) {
    PALETTE[index % PALETTE.len()]
}

fn fixed_colour(hex: &str, rgb: [u8; 3]) -> RowColour {
    RowColour { hex: hex.to_string(), rgb }
}

fn shade(colour: [u8; 3], factor: f64) -> (String, [u8; 3]) {
    let rgb = [
        (f64::from(colour[0]) * factor).round() as u8,
        (f64::from(colour[1]) * factor).round() as u8,
        (f64::from(colour[2]) * factor).round() as u8,
    ];
    (format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]), rgb)
}

fn shade_factor(level: usize, count: usize) -> f64 {
    if count <= 1 { 1.0 } else { 1.0 - 0.65 * (f64::from(level as u32) / f64::from(count as u32 - 1)) }
}

fn operation_count_note(options: &DrawingOptions, count: usize) -> String {
    if options.filtered(count) {
        format!("{} of {} operation(s)", options.selected(count).len(), count)
    }
    else {
        format!("{count} operation(s)")
    }
}

fn toolpath_legend(
    document: &CamDocument,
    report: &ToolpathReport,
    drawings: &[OpDrawing],
    selected: &[usize],
) -> Vec<LegendRow> {
    let mut rows = Vec::new();
    for &index in selected {
        let operation = &report.operations[index];
        let detail = match document.tools.iter().find(|tool| tool.id == operation.tool) {
            Some(tool) => format!("tool {} d{}", tool.id, number(tool.diameter)),
            None => format!("tool {}", operation.tool),
        };
        let (hex, rgb) = palette(index);
        rows.push(LegendRow {
            colour: fixed_colour(hex, rgb),
            style: Style::Solid,
            label: format!("#{}  {}  {detail}", index + 1, operation_name(operation.kind, &operation.operation)),
        });
    }
    rows.push(LegendRow {
        colour: fixed_colour(INK, INK_RGB),
        style: Style::Solid,
        label: "cutting move (G1/G2/G3)".to_string(),
    });
    rows.push(LegendRow {
        colour: fixed_colour(RAPID_COLOR, RAPID_RGB),
        style: Style::Dashed,
        label: "rapid move (G0)".to_string(),
    });
    rows.push(LegendRow { colour: fixed_colour(INK, INK_RGB), style: Style::Dot, label: "plunge / start point".to_string() });
    rows.push(LegendRow {
        colour: fixed_colour(INK, INK_RGB),
        style: Style::Ring,
        label: "drill hole (tool diameter)".to_string(),
    });
    if document.operations.iter().enumerate().any(|(index, operation)| {
        selected.contains(&index) && operation.kind == OperationKind::Pocket && !operation.islands.is_empty()
    }) {
        rows.push(LegendRow {
            colour: fixed_colour(ISLAND_COLOR, ISLAND_RGB),
            style: Style::Solid,
            label: "pocket island (nominal outline)".to_string(),
        });
    }
    if selected.iter().any(|index| !drawings[*index].tabs.is_empty()) {
        rows.push(LegendRow {
            colour: fixed_colour(TAB_COLOR, TAB_RGB),
            style: Style::Solid,
            label: "holding tab (lifted traverse)".to_string(),
        });
    }
    rows
}

fn operation_rows(
    document: &CamDocument,
    report: &ToolpathReport,
    drawings: &[OpDrawing],
    selected: &[usize],
) -> Vec<LegendRow> {
    let mut rows = Vec::new();
    for &index in selected {
        let operation = &report.operations[index];
        let detail = match document.tools.iter().find(|tool| tool.id == operation.tool) {
            Some(tool) => format!("tool {} d{} r{}", tool.id, number(tool.diameter), number(tool.radius())),
            None => format!("tool {}", operation.tool),
        };
        let name = operation_name(operation.kind, &operation.operation);
        let levels = chain_levels(&drawings[index]);
        if levels.len() <= 1 {
            let (hex, rgb) = palette(index);
            rows.push(LegendRow {
                colour: fixed_colour(hex, rgb),
                style: Style::Solid,
                label: format!("#{}  {}  {detail}", index + 1, name),
            });
        }
        else {
            for (level, z) in levels.iter().enumerate() {
                let (hex, rgb) = shade(palette(index).1, shade_factor(level, levels.len()));
                rows.push(LegendRow {
                    colour: RowColour { hex, rgb },
                    style: Style::Solid,
                    label: format!(
                        "#{}  {}  Z{} depth {} mm  {detail}",
                        index + 1,
                        name,
                        number(*z),
                        number(document.stock.height - z)
                    ),
                });
            }
        }
    }
    rows
}

fn setup_legend(document: &CamDocument) -> Vec<LegendRow> {
    document
        .operations
        .iter()
        .enumerate()
        .map(|(index, operation)| {
            let detail = match document.tools.iter().find(|tool| tool.id == operation.tool) {
                Some(tool) => format!("tool {} d{}", tool.id, number(tool.diameter)),
                None => format!("tool {}", operation.tool),
            };
            LegendRow {
                colour: fixed_colour(palette(index).0, palette(index).1),
                style: Style::Solid,
                label: format!(
                    "#{}  {}  depth {} mm  {detail}",
                    index + 1,
                    operation_name(operation.kind, &operation.id),
                    number(operation.depth)
                ),
            }
        })
        .collect()
}

fn toolpath_title_cells(document: &CamDocument, report: &ToolpathReport, id: &str, note: &str, format: &str) -> Vec<TitleCell> {
    vec![
        TitleCell { lines: vec![CellLine::ink(id, 3.0), CellLine::muted(format!("OpenCAM toolpath drawing ({format})"), 2.0)] },
        TitleCell { lines: vec![CellLine::ink(format!("units: {}", document.units), 2.4), CellLine::muted(note, 2.0)] },
        TitleCell {
            lines: vec![
                CellLine::ink(format!("REV {}", document.revision), 2.4),
                CellLine::muted(format!("est {} s", number(report.estimated_seconds)), 2.0),
            ],
        },
        TitleCell {
            lines: vec![
                CellLine::muted("fit to stock", 2.0),
                CellLine::muted(format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)), 2.0),
            ],
        },
    ]
}

fn stock_setup_title_cells(document: &CamDocument, id: &str, note: &str, format: &str) -> Vec<TitleCell> {
    vec![
        TitleCell {
            lines: vec![CellLine::ink(id, 3.0), CellLine::muted(format!("OpenCAM stock setup drawing ({format})"), 2.0)],
        },
        TitleCell { lines: vec![CellLine::ink(format!("units: {}", document.units), 2.4), CellLine::muted(note, 2.0)] },
        TitleCell {
            lines: vec![CellLine::ink(format!("REV {}", document.revision), 2.4), CellLine::muted("nominal geometry", 2.0)],
        },
        TitleCell {
            lines: vec![
                CellLine::muted("fit to stock", 2.0),
                CellLine::muted(format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)), 2.0),
            ],
        },
    ]
}

fn depth_map_title_cells(
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    note: &str,
    format: &str,
) -> Vec<TitleCell> {
    vec![
        TitleCell {
            lines: vec![CellLine::ink(id, 3.0), CellLine::muted(format!("OpenCAM depth map drawing ({format})"), 2.0)],
        },
        TitleCell { lines: vec![CellLine::ink(format!("units: {}", document.units), 2.4), CellLine::muted(note, 2.0)] },
        TitleCell {
            lines: vec![
                CellLine::ink(format!("REV {}", document.revision), 2.4),
                CellLine::muted(format!("est {} s", number(report.estimated_seconds)), 2.0),
            ],
        },
        TitleCell {
            lines: vec![
                CellLine::muted("cutting chains only", 2.0),
                CellLine::muted(format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)), 2.0),
            ],
        },
    ]
}

fn stock_removal_title_cells(document: &CamDocument, id: &str, note: &str, format: &str) -> Vec<TitleCell> {
    vec![
        TitleCell {
            lines: vec![CellLine::ink(id, 3.0), CellLine::muted(format!("OpenCAM stock removal drawing ({format})"), 2.0)],
        },
        TitleCell { lines: vec![CellLine::ink(format!("units: {}", document.units), 2.4), CellLine::muted(note, 2.0)] },
        TitleCell {
            lines: vec![CellLine::ink(format!("REV {}", document.revision), 2.4), CellLine::muted("section X-X", 2.0)],
        },
        TitleCell {
            lines: vec![
                CellLine::muted("cumulative depth", 2.0),
                CellLine::muted(format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)), 2.0),
            ],
        },
    ]
}

fn table_title_cells(id: &str, subtitle: &str, note: &str) -> Vec<TitleCell> {
    vec![
        TitleCell { lines: vec![CellLine::ink(id, 3.0), CellLine::muted(subtitle, 2.0)] },
        TitleCell { lines: vec![CellLine::ink("units: mm", 2.4), CellLine::muted(note, 2.0)] },
        TitleCell { lines: vec![CellLine::ink("measured columns", 2.0), CellLine::muted("numbers right aligned", 2.0)] },
        TitleCell { lines: vec![CellLine::muted("A4 landscape", 2.0), CellLine::muted("viewBox in mm", 2.0)] },
    ]
}

fn operation_table_rows(document: &CamDocument, report: &ToolpathReport, options: &DrawingOptions) -> Vec<Vec<String>> {
    options
        .selected(document.operations.len())
        .into_iter()
        .map(|index| {
            let operation = &document.operations[index];
            let path = report.operations.get(index);
            let tool = document.tools.iter().find(|tool| tool.id == operation.tool);
            vec![
                format!("#{}", index + 1),
                operation_name(operation.kind, &operation.id),
                operation.tool.clone(),
                tool.map_or_else(|| "-".to_string(), |tool| number(tool.diameter)),
                number(operation.depth),
                operation.stepover.map_or_else(|| "-".to_string(), number),
                tool.map_or_else(|| "-".to_string(), |tool| number(tool.feed)),
                tool.map_or_else(|| "-".to_string(), |tool| number(tool.spindle)),
                path.map_or_else(|| "-".to_string(), |path| number(path.cutting_seconds + path.rapid_seconds)),
                path.map_or_else(|| "-".to_string(), |path| number(path.cut_length)),
            ]
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Projection of the toolpath report
// ---------------------------------------------------------------------------

struct CutChain {
    z: f64,
    points: Vec<(f64, f64)>,
}

struct OpDrawing {
    cuts: Vec<CutChain>,
    rapids: Vec<((f64, f64), (f64, f64))>,
    tabs: Vec<((f64, f64), (f64, f64))>,
    plunges: Vec<(f64, f64)>,
    holes: Vec<(f64, f64)>,
    hole_radius: Option<f64>,
    start: Option<(f64, f64)>,
}

fn drill_radius(document: &CamDocument, operation: &OperationPath) -> Option<f64> {
    if operation.kind != OperationKind::Drill {
        return None;
    }
    document.tools.iter().find(|tool| tool.id == operation.tool).map(Tool::radius)
}

fn project(operation: &OperationPath, hole_radius: Option<f64>, safe_z: f64, previous: Option<(f64, f64)>) -> OpDrawing {
    let mut drawing = OpDrawing {
        cuts: Vec::new(),
        rapids: Vec::new(),
        tabs: Vec::new(),
        plunges: Vec::new(),
        holes: Vec::new(),
        hole_radius,
        start: None,
    };
    let mut chain: Vec<(f64, f64)> = Vec::new();
    let mut chain_z: Option<f64> = None;
    let mut lifted: Vec<((f64, f64), (f64, f64))> = Vec::new();
    let mut seen_plunges: HashSet<[i64; 2]> = HashSet::new();
    let mut first_rapid = true;
    for movement in &operation.segments {
        match movement.kind {
            MoveKind::Rapid => {
                let from = if first_rapid {
                    previous.unwrap_or((movement.from.x, movement.from.y))
                }
                else {
                    (movement.from.x, movement.from.y)
                };
                first_rapid = false;
                flush(&mut chain, &mut chain_z, &mut drawing.cuts);
                if (from.0 - movement.to.x).abs() > 1e-9 || (from.1 - movement.to.y).abs() > 1e-9 {
                    if (movement.from.z - movement.to.z).abs() <= 1e-9 && movement.from.z < safe_z - 1e-9 {
                        lifted.push((from, (movement.to.x, movement.to.y)));
                    }
                    else {
                        drawing.rapids.push((from, (movement.to.x, movement.to.y)));
                    }
                }
            }
            MoveKind::Plunge => {
                flush(&mut chain, &mut chain_z, &mut drawing.cuts);
                if hole_radius.is_some() {
                    drawing.holes.push((movement.from.x, movement.from.y));
                }
                else if seen_plunges
                    .insert([(movement.from.x * 1000.0).round() as i64, (movement.from.y * 1000.0).round() as i64])
                {
                    drawing.plunges.push((movement.from.x, movement.from.y));
                }
            }
            MoveKind::Cut => {
                if chain.is_empty() {
                    chain.push((movement.from.x, movement.from.y));
                    chain_z = Some(movement.to.z);
                }
                chain.push((movement.to.x, movement.to.y));
            }
            MoveKind::Arc => {
                if chain.is_empty() {
                    chain.push((movement.from.x, movement.from.y));
                    chain_z = Some(movement.to.z);
                }
                append_arc(&mut chain, movement);
            }
        }
        if drawing.start.is_none() && movement.kind != MoveKind::Rapid {
            drawing.start = Some((movement.from.x, movement.from.y));
        }
    }
    flush(&mut chain, &mut chain_z, &mut drawing.cuts);
    let mut merged: Vec<((f64, f64), (f64, f64))> = Vec::new();
    for (from, to) in lifted {
        if let Some(last) = merged.last_mut() {
            if (last.1.0 - from.0).abs() <= 1e-6 && (last.1.1 - from.1).abs() <= 1e-6 {
                last.1 = to;
                continue;
            }
        }
        merged.push((from, to));
    }
    let mut seen_tabs: HashSet<[i64; 4]> = HashSet::new();
    drawing.tabs = merged
        .into_iter()
        .filter(|(from, to)| {
            seen_tabs.insert([
                (from.0 * 1000.0).round() as i64,
                (from.1 * 1000.0).round() as i64,
                (to.0 * 1000.0).round() as i64,
                (to.1 * 1000.0).round() as i64,
            ])
        })
        .collect();
    drawing
}

fn flush(chain: &mut Vec<(f64, f64)>, chain_z: &mut Option<f64>, cuts: &mut Vec<CutChain>) {
    if chain.len() > 1 {
        cuts.push(CutChain { z: chain_z.take().unwrap_or(0.0), points: std::mem::take(chain) });
    }
    else {
        chain.clear();
        *chain_z = None;
    }
}

fn append_arc(points: &mut Vec<(f64, f64)>, movement: &Move) {
    let Some(arc) = &movement.arc
    else {
        points.push((movement.to.x, movement.to.y));
        return;
    };
    let centre = (arc.centre.x, arc.centre.y);
    let radius = ((movement.from.x - centre.0).powi(2) + (movement.from.y - centre.1).powi(2)).sqrt();
    if radius <= 1e-9 {
        points.push((movement.to.x, movement.to.y));
        return;
    }
    let start = (movement.from.y - centre.1).atan2(movement.from.x - centre.0);
    let end = (movement.to.y - centre.1).atan2(movement.to.x - centre.0);
    let sweep = sweep_angle(start, end, arc.clockwise);
    let chords = ((sweep / std::f64::consts::TAU) * ARC_CHORDS_PER_TURN as f64).ceil().max(1.0) as usize;
    for step in 1..chords {
        let fraction = step as f64 / chords as f64;
        let angle = if arc.clockwise { start - sweep * fraction } else { start + sweep * fraction };
        points.push((centre.0 + radius * angle.cos(), centre.1 + radius * angle.sin()));
    }
    points.push((movement.to.x, movement.to.y));
}

fn sweep_angle(start: f64, end: f64, clockwise: bool) -> f64 {
    let mut sweep = if clockwise { start - end } else { end - start };
    while sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    sweep.min(std::f64::consts::TAU)
}

fn project_all(document: &CamDocument, report: &ToolpathReport) -> Vec<OpDrawing> {
    let mut drawings = Vec::with_capacity(report.operations.len());
    let mut previous: Option<(f64, f64)> = None;
    for operation in &report.operations {
        drawings.push(project(operation, drill_radius(document, operation), report.safe_z, previous));
        previous = operation.segments.last().map(|movement| (movement.to.x, movement.to.y)).or(previous);
    }
    drawings
}

fn chain_levels(drawing: &OpDrawing) -> Vec<f64> {
    let mut levels: Vec<f64> = Vec::new();
    for chain in &drawing.cuts {
        let key = (chain.z * 1000.0).round();
        if !levels.iter().any(|level| (level * 1000.0).round() == key) {
            levels.push(chain.z);
        }
    }
    levels
}

fn level_index(levels: &[f64], z: f64) -> usize {
    let key = (z * 1000.0).round();
    levels.iter().position(|level| (level * 1000.0).round() == key).unwrap_or(0)
}

fn toolpath_frame(document: &CamDocument, drawings: &[OpDrawing], selected: &[usize]) -> Frame {
    let mut frame =
        Frame { min_x: 0.0, min_y: 0.0, max_x: document.stock.width.max(1.0), max_y: document.stock.depth.max(1.0) };
    for &index in selected {
        let drawing = &drawings[index];
        for point in drawing.cuts.iter().flat_map(|chain| chain.points.iter()) {
            frame.include(point.0, point.1);
        }
        for point in drawing.holes.iter().chain(drawing.plunges.iter()) {
            frame.include(point.0, point.1);
        }
        if let Some(radius) = drawing.hole_radius {
            for point in &drawing.holes {
                frame.include(point.0 - radius, point.1 - radius);
                frame.include(point.0 + radius, point.1 + radius);
            }
        }
        for (from, to) in &drawing.rapids {
            frame.include(from.0, from.1);
            frame.include(to.0, to.1);
        }
    }
    frame
}

fn setup_frame(document: &CamDocument) -> Frame {
    let mut frame =
        Frame { min_x: 0.0, min_y: 0.0, max_x: document.stock.width.max(1.0), max_y: document.stock.depth.max(1.0) };
    for operation in &document.operations {
        match operation.kind {
            OperationKind::Face => {}
            OperationKind::Contour | OperationKind::Pocket => {
                frame.include(operation.x, operation.y);
                frame.include(operation.x + operation.width, operation.y + operation.height);
                for island in &operation.islands {
                    frame.include(island.x, island.y);
                    frame.include(island.x + island.width, island.y + island.height);
                }
            }
            OperationKind::Engrave => {
                for point in &operation.path {
                    frame.include(point.x, point.y);
                }
            }
            OperationKind::Drill => {
                let radius = document.tools.iter().find(|tool| tool.id == operation.tool).map_or(0.0, Tool::radius);
                for hole in operation.resolved_holes() {
                    frame.include(hole.x - radius, hole.y - radius);
                    frame.include(hole.x + radius, hole.y + radius);
                }
            }
        }
    }
    frame
}

fn operation_label_point(operation: &Operation) -> (f64, f64) {
    match operation.kind {
        OperationKind::Face => (0.0, 0.0),
        OperationKind::Contour | OperationKind::Pocket => (operation.x, operation.y + operation.height),
        OperationKind::Engrave => {
            (operation.path.first().map_or(0.0, |point| point.x), operation.path.first().map_or(0.0, |point| point.y))
        }
        OperationKind::Drill => {
            let hole = operation.resolved_holes().into_iter().next().unwrap_or(crate::model::Hole { x: 0.0, y: 0.0 });
            (hole.x, hole.y)
        }
    }
}

fn operation_name(kind: OperationKind, id: &str) -> String {
    if kind.as_str() == id { id.to_string() } else { format!("{} {}", kind.as_str(), id) }
}

// ---------------------------------------------------------------------------
// Stock-removal section
// ---------------------------------------------------------------------------

struct RemovalBand {
    x0: f64,
    x1: f64,
    top_depth: f64,
    bottom_depth: f64,
    operation: usize,
}

struct Section {
    bands: Vec<RemovalBand>,
    breakpoints: Vec<f64>,
}

impl Section {
    fn new(document: &CamDocument, selected: &[usize]) -> Self {
        let mut intervals: Vec<(f64, f64, f64, usize)> = Vec::new();
        for &index in selected {
            let operation = &document.operations[index];
            let depth = operation.depth.clamp(0.0, document.stock.height);
            if depth <= 0.0 {
                continue;
            }
            match operation.kind {
                OperationKind::Face => intervals.push((0.0, document.stock.width, depth, index)),
                OperationKind::Contour | OperationKind::Pocket => {
                    if operation.shape.as_deref() == Some("circle") {
                        let radius = operation.width / 2.0;
                        intervals.push((operation.x, operation.x + 2.0 * radius, depth, index));
                    }
                    else if operation.width > 0.0 {
                        intervals.push((operation.x, operation.x + operation.width, depth, index));
                    }
                }
                OperationKind::Drill => {
                    let radius = document.tools.iter().find(|tool| tool.id == operation.tool).map_or(0.0, Tool::radius);
                    for hole in operation.resolved_holes() {
                        intervals.push((hole.x - radius, hole.x + radius, depth, index));
                    }
                }
                OperationKind::Engrave => {
                    if let (Some(min), Some(max)) = (
                        operation.path.iter().map(|point| point.x).reduce(f64::min),
                        operation.path.iter().map(|point| point.x).reduce(f64::max),
                    ) {
                        intervals.push((min, max, depth, index));
                    }
                }
            }
        }
        let mut breakpoints = vec![0.0, document.stock.width.max(1.0)];
        for (x0, x1, _, _) in &intervals {
            breakpoints.push(x0.clamp(0.0, document.stock.width));
            breakpoints.push(x1.clamp(0.0, document.stock.width));
        }
        breakpoints.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        breakpoints.dedup_by(|left, right| (*left - *right).abs() <= 1e-6);
        let mut bands = Vec::new();
        for pair in breakpoints.windows(2) {
            let (x0, x1) = (pair[0], pair[1]);
            if x1 - x0 <= 1e-6 {
                continue;
            }
            let middle = (x0 + x1) / 2.0;
            let mut levels: Vec<(f64, usize)> = intervals
                .iter()
                .filter(|(start, end, _, _)| *start - 1e-6 <= middle && middle <= *end + 1e-6)
                .map(|(_, _, depth, index)| (*depth, *index))
                .collect();
            levels.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut previous = 0.0;
            for (depth, index) in levels {
                if depth <= previous + 1e-6 {
                    continue;
                }
                bands.push(RemovalBand { x0, x1, top_depth: previous, bottom_depth: depth, operation: index });
                previous = depth;
            }
        }
        Self { bands, breakpoints }
    }

    fn profile(&self, height: f64) -> Vec<(f64, f64)> {
        let mut points: Vec<(f64, f64)> = Vec::new();
        for pair in self.breakpoints.windows(2) {
            let (x0, x1) = (pair[0], pair[1]);
            let middle = (x0 + x1) / 2.0;
            let depth = self
                .bands
                .iter()
                .filter(|band| band.x0 - 1e-6 <= middle && middle <= band.x1 + 1e-6)
                .map(|band| band.bottom_depth)
                .fold(0.0_f64, f64::max);
            let z = height - depth;
            let previous_z = points.last().map_or(z, |point| point.1);
            if points.is_empty() || (previous_z - z).abs() > 1e-6 {
                points.push((x0, z));
            }
            points.push((x1, z));
        }
        points
    }

    fn allowance(&self, height: f64) -> (f64, f64) {
        let deepest = self.bands.iter().map(|band| band.bottom_depth).fold(0.0_f64, f64::max);
        (height - deepest, deepest)
    }

    fn legend(&self, document: &CamDocument, selected: &[usize]) -> Vec<LegendRow> {
        let mut rows = Vec::new();
        for &index in selected {
            let operation = &document.operations[index];
            let detail = match document.tools.iter().find(|tool| tool.id == operation.tool) {
                Some(tool) => format!("tool {} d{}", tool.id, number(tool.diameter)),
                None => format!("tool {}", operation.tool),
            };
            rows.push(LegendRow {
                colour: fixed_colour(palette(index).0, palette(index).1),
                style: Style::Solid,
                label: format!(
                    "#{}  {}  depth {} mm  {detail}",
                    index + 1,
                    operation_name(operation.kind, &operation.id),
                    number(operation.depth)
                ),
            });
        }
        rows.push(LegendRow {
            colour: fixed_colour(INK, INK_RGB),
            style: Style::Solid,
            label: "final part outline".to_string(),
        });
        rows.push(LegendRow {
            colour: fixed_colour(STOCK_COLOR, STOCK_RGB),
            style: Style::Solid,
            label: "stock allowance below cut floor".to_string(),
        });
        rows
    }
}

// ---------------------------------------------------------------------------
// Small utilities
// ---------------------------------------------------------------------------

fn rgb(colour: [u8; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", f64::from(colour[0]) / 255.0, f64::from(colour[1]) / 255.0, f64::from(colour[2]) / 255.0)
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn pdf_text(text: &str) -> (String, usize) {
    let mut escaped = String::from("(");
    let mut dropped = 0;
    for character in text.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '(' => escaped.push_str("\\("),
            ')' => escaped.push_str("\\)"),
            '\n' | '\r' | '\t' => escaped.push(' '),
            character if character.is_ascii() && !character.is_ascii_control() => escaped.push(character),
            _ => {
                escaped.push('?');
                dropped += 1;
            }
        }
    }
    escaped.push(')');
    (escaped, dropped)
}

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    let rounded = if rounded.abs() < 0.0005 { 0.0 } else { rounded };
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}
