//! Vector drawing export.
//!
//! [`svg`] and [`pdf`] draw the stock outline and the moves [`crate::toolpath`]
//! already generated: cutting polylines, dashed rapid traverses, plunge and
//! start markers, one legend row per operation and a title block. Pocket
//! islands are added as their nominal outlines and contour holding tabs as
//! thick marker segments on the lifted traverses, so the sheet shows the
//! geometry the moves only avoid. Rapids form an underlay below the cutting
//! polylines; the first rapid of an operation is drawn from where the previous
//! operation ended, and cutting chains are shaded from the full operation
//! colour at the first Z level to 35 % at the deepest, with one legend row per
//! level. Three further sheets share the same style: [`tool_list_svg`] and
//! [`tool_list_pdf`] tabulate the declared tools, [`stock_setup_svg`] and
//! [`stock_setup_pdf`] place the nominal operation geometry on the stock, and
//! [`depth_map_svg`] and [`depth_map_pdf`] show only the cutting chains with
//! their Z levels. Nothing here regenerates a toolpath, and every sheet is
//! deterministic: coordinates are rounded to three decimals, operations keep
//! document order and no timestamp is written.

use std::collections::HashSet;

use crate::{
    model::{CamDocument, Island, OperationKind, Tool},
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
const ISLAND_COLOR: &str = "#475569";
const ISLAND_RGB: [u8; 3] = [71, 85, 105];
const TAB_COLOR: &str = "#9a3412";
const TAB_RGB: [u8; 3] = [154, 52, 18];
const PATH_WIDTH_MM: f64 = 0.5;
const RAPID_WIDTH_MM: f64 = 0.3;
const ISLAND_WIDTH_MM: f64 = 0.35;
const TAB_WIDTH_MM: f64 = 1.4;
const DRILL_RING_WIDTH_MM: f64 = 0.5;
const PX_PER_MM: f64 = 8.0;
const PAD_MM: f64 = 5.0;
const MIN_SHEET_WIDTH_MM: f64 = 120.0;
const TITLE_HEIGHT_MM: f64 = 18.0;
const LEGEND_ROW_MM: f64 = 4.2;
const LEGEND_GAP_MM: f64 = 2.0;
const ARC_CHORDS_PER_TURN: usize = 72;

const PDF_PAGE_WIDTH: f64 = 842.0;
const PDF_PAGE_HEIGHT: f64 = 595.0;
const PDF_MARGIN: f64 = 30.0;
const PDF_TITLE_BLOCK: f64 = 46.0;
const PDF_LEGEND_ROW: f64 = 9.0;
const MM_TO_PT: f64 = 2.834_646;

/// Renders the sheet as SVG with a millimetre `viewBox` and `8` px per mm.
///
/// The drawing area holds the stock rectangle and every toolpath move; the
/// legend names the moves and each operation with its tool; the title block
/// carries the document id, revision, units, operation count, estimated time
/// and scale. An empty document still produces a valid sheet.
pub fn svg(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    let drawings = project_all(document, report);
    let islands: Vec<&Island> = document
        .operations
        .iter()
        .filter(|operation| operation.kind == OperationKind::Pocket)
        .flat_map(|operation| &operation.islands)
        .collect();
    let frame = frame(document, &drawings);
    let legend = legend(document, report, &drawings);
    let legend_height = legend.len() as f64 * LEGEND_ROW_MM + LEGEND_GAP_MM;
    let sheet_width = frame.width.max(MIN_SHEET_WIDTH_MM);
    let sheet_height = frame.height + legend_height + TITLE_HEIGHT_MM;

    let map_x = |x: f64| PAD_MM + (x - frame.min_x);
    let map_y = |y: f64| PAD_MM + (frame.max_y - y);

    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {0} {1}\" width=\"{2}\" height=\"{3}\">\n",
        number(sheet_width),
        number(sheet_height),
        number(sheet_width * PX_PER_MM),
        number(sheet_height * PX_PER_MM)
    ));
    svg.push_str(&format!("  <title>OpenCAM drawing {}</title>\n", escape_xml(id)));
    svg.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        number(sheet_width),
        number(sheet_height)
    ));

    svg.push_str("  <g data-stock=\"true\">\n");
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{STOCK_COLOR}\" stroke-width=\"0.35\"/>\n",
        number(map_x(0.0)),
        number(map_y(document.stock.depth)),
        number(document.stock.width),
        number(document.stock.depth)
    ));
    svg.push_str(&format!(
        "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"2.6\" fill=\"{MUTED}\">stock {} x {} x {} mm</text>\n",
        number(map_x(0.0) + 0.8),
        number(map_y(document.stock.depth) - 1.0),
        number(document.stock.width),
        number(document.stock.depth),
        number(document.stock.height)
    ));
    svg.push_str("  </g>\n");

    if !islands.is_empty() {
        svg.push_str("  <g data-islands=\"true\">\n");
        for island in &islands {
            if island.shape.as_deref() == Some("circle") {
                svg.push_str(&format!(
                    "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{ISLAND_COLOR}\" stroke-width=\"{ISLAND_WIDTH_MM}\"/>\n",
                    number(map_x(island.x + island.width / 2.0)),
                    number(map_y(island.y + island.height / 2.0)),
                    number(island.width / 2.0)
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
                .map(|(x, y)| format!("{},{}", number(map_x(x)), number(map_y(y))))
                .collect::<Vec<_>>()
                .join(" ");
                svg.push_str(&format!(
                    "    <polygon points=\"{points}\" fill=\"none\" stroke=\"{ISLAND_COLOR}\" stroke-width=\"{ISLAND_WIDTH_MM}\" stroke-linejoin=\"round\"/>\n"
                ));
            }
        }
        svg.push_str("  </g>\n");
    }

    svg.push_str("  <g data-rapids=\"true\">\n");
    for drawing in &drawings {
        for (from, to) in &drawing.rapids {
            svg.push_str(&format!(
                "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{STOCK_COLOR}\" stroke-width=\"{RAPID_WIDTH_MM}\" stroke-dasharray=\"1.5 1\"/>\n",
                number(map_x(from.0)),
                number(map_y(from.1)),
                number(map_x(to.0)),
                number(map_y(to.1))
            ));
        }
    }
    svg.push_str("  </g>\n");

    for (index, (operation, drawing)) in report.operations.iter().zip(&drawings).enumerate() {
        let (hex, _) = palette(index);
        svg.push_str(&format!(
            "  <g data-operation=\"{}\" data-index=\"{index}\" data-tool=\"{}\">\n",
            escape_xml(&operation.operation),
            escape_xml(&operation.tool)
        ));
        let levels = chain_levels(drawing);
        let shades: Vec<String> =
            (0..levels.len()).map(|level| shade(palette(index).1, shade_factor(level, levels.len())).0).collect();
        for chain in &drawing.cuts {
            if chain.points.len() < 2 {
                continue;
            }
            let shade = &shades[level_index(&levels, chain.z)];
            let points = chain
                .points
                .iter()
                .map(|(x, y)| format!("{},{}", number(map_x(*x)), number(map_y(*y))))
                .collect::<Vec<_>>()
                .join(" ");
            svg.push_str(&format!(
                "    <polyline points=\"{points}\" fill=\"none\" stroke=\"{shade}\" stroke-width=\"{PATH_WIDTH_MM}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n"
            ));
        }
        for (from, to) in &drawing.tabs {
            svg.push_str(&format!(
                "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{TAB_COLOR}\" stroke-width=\"{TAB_WIDTH_MM}\" stroke-linecap=\"round\"/>\n",
                number(map_x(from.0)),
                number(map_y(from.1)),
                number(map_x(to.0)),
                number(map_y(to.1))
            ));
        }
        if let Some(radius) = drawing.hole_radius {
            for (x, y) in &drawing.holes {
                svg.push_str(&format!(
                    "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{DRILL_RING_WIDTH_MM}\"/>\n",
                    number(map_x(*x)),
                    number(map_y(*y)),
                    number(radius)
                ));
            }
        }
        for (x, y) in &drawing.plunges {
            svg.push_str(&format!(
                "    <circle cx=\"{}\" cy=\"{}\" r=\"0.6\" fill=\"{hex}\"/>\n",
                number(map_x(*x)),
                number(map_y(*y))
            ));
        }
        if let Some((x, y)) = drawing.start {
            svg.push_str(&format!(
                "    <circle cx=\"{}\" cy=\"{}\" r=\"1.4\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"0.35\"/>\n",
                number(map_x(x)),
                number(map_y(y))
            ));
        }
        svg.push_str("  </g>\n");
    }

    let legend_top = frame.height + LEGEND_GAP_MM;
    svg.push_str("  <g data-legend=\"true\">\n");
    for (row, entry) in legend.iter().enumerate() {
        let baseline = legend_top + 3.0 + row as f64 * LEGEND_ROW_MM;
        let color = entry.colour.hex.as_str();
        match entry.style {
            Style::Solid => svg.push_str(&format!(
                "    <line x1=\"2\" y1=\"{}\" x2=\"9\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"0.6\"/>\n",
                number(baseline - 0.8),
                number(baseline - 0.8)
            )),
            Style::Dashed => svg.push_str(&format!(
                "    <line x1=\"2\" y1=\"{}\" x2=\"9\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"0.4\" stroke-dasharray=\"1.5 1\"/>\n",
                number(baseline - 0.8),
                number(baseline - 0.8)
            )),
            Style::Dot => svg.push_str(&format!(
                "    <circle cx=\"5.5\" cy=\"{}\" r=\"0.7\" fill=\"{color}\"/>\n",
                number(baseline - 0.8)
            )),
            Style::Ring => svg.push_str(&format!(
                "    <circle cx=\"5.5\" cy=\"{}\" r=\"1.6\" fill=\"none\" stroke=\"{color}\" stroke-width=\"0.5\"/>\n",
                number(baseline - 0.8)
            )),
        }
        svg.push_str(&format!(
            "    <text x=\"10.5\" y=\"{}\" font-family=\"monospace\" font-size=\"2.4\" fill=\"{INK}\">{}</text>\n",
            number(baseline),
            escape_xml(&entry.label)
        ));
    }
    svg.push_str("  </g>\n");

    push_svg_title_block(&mut svg, document, report, id, sheet_width, sheet_height);
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the sheet as a one-page A4 landscape PDF 1.4 document.
///
/// The content is fitted to the page, centred above the legend and title block
/// and drawn with Helvetica. Non-ASCII characters become `?` and are counted in
/// [`Drawing::dropped`].
pub fn pdf(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    let drawings = project_all(document, report);
    let islands: Vec<&Island> = document
        .operations
        .iter()
        .filter(|operation| operation.kind == OperationKind::Pocket)
        .flat_map(|operation| &operation.islands)
        .collect();
    let frame = frame(document, &drawings);
    let legend = legend(document, report, &drawings);
    let legend_height = legend.len() as f64 * PDF_LEGEND_ROW + 8.0;
    let title_top = PDF_MARGIN + PDF_TITLE_BLOCK;
    let legend_top = title_top + legend_height;
    let area_left = PDF_MARGIN;
    let area_bottom = legend_top;
    let area_width = PDF_PAGE_WIDTH - 2.0 * PDF_MARGIN;
    let area_height = PDF_PAGE_HEIGHT - PDF_MARGIN - legend_top;
    let scale = (area_width / frame.width).min(area_height / frame.height).max(0.01);
    let offset_x = area_left + (area_width - frame.width * scale) / 2.0;
    let offset_y = area_bottom + (area_height - frame.height * scale) / 2.0;
    let map_x = |x: f64| offset_x + (x - frame.min_x) * scale;
    let map_y = |y: f64| offset_y + (y - frame.min_y) * scale;
    let percent = (scale / MM_TO_PT * 100.0).round();

    let mut dropped = 0usize;
    let mut content = String::new();
    content.push_str(&format!("1 1 1 rg 0 0 {PDF_PAGE_WIDTH:.2} {PDF_PAGE_HEIGHT:.2} re f\n"));

    content.push_str(&format!(
        "{} RG 0.6 w [2 2] 0 d {:.2} {:.2} {:.2} {:.2} re S [] 0 d\n",
        rgb(STOCK_RGB),
        map_x(0.0),
        map_y(0.0),
        document.stock.width * scale,
        document.stock.depth * scale
    ));
    push_pdf_text(
        &mut content,
        map_x(0.0) + 1.0,
        map_y(document.stock.depth) + 3.0,
        7.0,
        "0.4",
        &format!(
            "stock {} x {} x {} mm",
            number(document.stock.width),
            number(document.stock.depth),
            number(document.stock.height)
        ),
        &mut dropped,
    );

    for island in &islands {
        if island.shape.as_deref() == Some("circle") {
            push_pdf_circle(
                &mut content,
                map_x(island.x + island.width / 2.0),
                map_y(island.y + island.height / 2.0),
                island.width / 2.0 * scale,
                ISLAND_RGB,
                true,
            );
        }
        else {
            content.push_str(&format!(
                "{} RG 0.8 w {:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
                rgb(ISLAND_RGB),
                map_x(island.x),
                map_y(island.y),
                map_x(island.x + island.width),
                map_y(island.y),
                map_x(island.x + island.width),
                map_y(island.y + island.height),
                map_x(island.x),
                map_y(island.y + island.height)
            ));
        }
    }

    for drawing in &drawings {
        for (from, to) in &drawing.rapids {
            content.push_str(&format!(
                "{} RG 0.5 w [2 2] 0 d {:.2} {:.2} m {:.2} {:.2} l S [] 0 d\n",
                rgb(STOCK_RGB),
                map_x(from.0),
                map_y(from.1),
                map_x(to.0),
                map_y(to.1)
            ));
        }
    }

    for (index, drawing) in drawings.iter().enumerate() {
        let colour = palette(index).1;
        let levels = chain_levels(drawing);
        let shades: Vec<[u8; 3]> = (0..levels.len()).map(|level| shade(colour, shade_factor(level, levels.len())).1).collect();
        for chain in &drawing.cuts {
            if chain.points.len() < 2 {
                continue;
            }
            content.push_str(&format!("{} RG 1.1 w 1 J 1 j ", rgb(shades[level_index(&levels, chain.z)])));
            for (position, (x, y)) in chain.points.iter().enumerate() {
                let operator = if position == 0 { "m" } else { "l" };
                content.push_str(&format!("{:.2} {:.2} {operator} ", map_x(*x), map_y(*y)));
            }
            content.push_str("S\n");
        }
        for (from, to) in &drawing.tabs {
            content.push_str(&format!(
                "{} RG 3 w 1 J {:.2} {:.2} m {:.2} {:.2} l S\n",
                rgb(TAB_RGB),
                map_x(from.0),
                map_y(from.1),
                map_x(to.0),
                map_y(to.1)
            ));
        }
        if let Some(radius) = drawing.hole_radius {
            for (x, y) in &drawing.holes {
                content.push_str(&format!("% drill hole r={}\n", number(radius)));
                push_pdf_circle(&mut content, map_x(*x), map_y(*y), radius * scale, colour, true);
            }
        }
        for (x, y) in &drawing.plunges {
            push_pdf_circle(&mut content, map_x(*x), map_y(*y), 1.5, colour, false);
        }
        if let Some((x, y)) = drawing.start {
            push_pdf_circle(&mut content, map_x(x), map_y(y), 4.0, colour, true);
            push_pdf_text(&mut content, map_x(x) + 5.0, map_y(y) + 4.0, 6.0, "0.3", &format!("#{}", index + 1), &mut dropped);
        }
    }

    let mut row_y = legend_top - 7.0;
    for entry in &legend {
        match entry.style {
            Style::Solid => content.push_str(&format!(
                "{0} RG 0.9 w {1:.2} {row_y:.2} m {2:.2} {row_y:.2} l S\n",
                rgb(entry.colour.rgb),
                PDF_MARGIN,
                PDF_MARGIN + 18.0
            )),
            Style::Dashed => content.push_str(&format!(
                "{0} RG 0.6 w [3 2] 0 d {1:.2} {row_y:.2} m {2:.2} {row_y:.2} l S [] 0 d\n",
                rgb(entry.colour.rgb),
                PDF_MARGIN,
                PDF_MARGIN + 18.0
            )),
            Style::Dot => push_pdf_circle(&mut content, PDF_MARGIN + 9.0, row_y, 2.0, entry.colour.rgb, false),
            Style::Ring => push_pdf_circle(&mut content, PDF_MARGIN + 9.0, row_y, 3.2, entry.colour.rgb, true),
        }
        push_pdf_text(&mut content, PDF_MARGIN + 24.0, row_y - 2.0, 7.0, "0", &entry.label, &mut dropped);
        row_y -= PDF_LEGEND_ROW;
    }

    push_pdf_title_block(&mut content, document, report, id, title_top, percent, &mut dropped);

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

/// Renders the declared tools as an SVG table sheet.
///
/// One row per tool in document order: id, name, tool number, diameter,
/// radius, flutes, spindle speed and the two feeds. An empty tool table still
/// produces a valid sheet.
pub fn tool_list_svg(document: &CamDocument, id: &str) -> Drawing {
    let headers = ["id", "name", "T", "d mm", "r mm", "flutes", "rpm", "feed", "plunge"];
    let widths = [16.0, 26.0, 6.0, 10.0, 10.0, 8.0, 12.0, 12.0, 12.0];
    let table_width: f64 = widths.iter().sum();
    let sheet_width = table_width.max(MIN_SHEET_WIDTH_MM);
    let left = (sheet_width - table_width) / 2.0;
    let top = 8.0;
    let row = 5.0;
    let rows: Vec<[String; 9]> = document
        .tools
        .iter()
        .map(|tool| {
            [
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
    let sheet_height = top + row * (rows.len() as f64 + 2.0) + 4.0 + TITLE_HEIGHT_MM;

    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {0} {1}\" width=\"{2}\" height=\"{3}\">\n",
        number(sheet_width),
        number(sheet_height),
        number(sheet_width * PX_PER_MM),
        number(sheet_height * PX_PER_MM)
    ));
    svg.push_str(&format!("  <title>OpenCAM tool list {}</title>\n", escape_xml(id)));
    svg.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        number(sheet_width),
        number(sheet_height)
    ));
    svg.push_str("  <g data-tool-table=\"true\">\n");
    let column_x = |index: usize| left + widths[..index].iter().sum::<f64>();
    let text = |svg: &mut String, x: f64, y: f64, fill: &str, value: &str| {
        svg.push_str(&format!(
            "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"2.2\" fill=\"{fill}\">{}</text>\n",
            number(x),
            number(y),
            escape_xml(value)
        ));
    };
    for (index, header) in headers.iter().enumerate() {
        text(&mut svg, column_x(index) + 1.0, top + 3.2, INK, header);
    }
    svg.push_str(&format!(
        "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
        number(left),
        number(top + row),
        number(left + table_width),
        number(top + row)
    ));
    for (row_index, cells) in rows.iter().enumerate() {
        let baseline = top + row * (row_index as f64 + 1.0) + 3.2;
        for (index, cell) in cells.iter().enumerate() {
            text(&mut svg, column_x(index) + 1.0, baseline, INK, cell);
        }
        svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{STOCK_COLOR}\" stroke-width=\"0.2\"/>\n",
            number(left),
            number(top + row * (row_index as f64 + 2.0)),
            number(left + table_width),
            number(top + row * (row_index as f64 + 2.0))
        ));
    }
    if rows.is_empty() {
        text(&mut svg, left + 1.0, top + row * 2.0 + 3.2, MUTED, "no tools declared");
    }
    svg.push_str("  </g>\n");
    push_svg_block(
        &mut svg,
        document,
        id,
        "OpenCAM tool list drawing (SVG)",
        &format!("{} tool(s)", document.tools.len()),
        sheet_width,
        sheet_height,
    );
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the declared tools as a one-page A4 landscape PDF table.
pub fn tool_list_pdf(document: &CamDocument, id: &str) -> Drawing {
    let headers = ["id", "name", "T", "d mm", "r mm", "flutes", "rpm", "feed", "plunge"];
    let widths = [45.0, 70.0, 30.0, 40.0, 40.0, 40.0, 50.0, 50.0, 50.0];
    let table_width: f64 = widths.iter().sum();
    let left = (PDF_PAGE_WIDTH - table_width) / 2.0;
    let row = 12.0;
    let top = PDF_PAGE_HEIGHT - PDF_MARGIN - 14.0;
    let mut dropped = 0usize;
    let mut content = String::new();
    content.push_str(&format!("1 1 1 rg 0 0 {PDF_PAGE_WIDTH:.2} {PDF_PAGE_HEIGHT:.2} re f\n"));
    let column_x = |index: usize| left + widths[..index].iter().sum::<f64>();
    for (index, header) in headers.iter().enumerate() {
        push_pdf_text(&mut content, column_x(index) + 3.0, top, 8.0, "0", header, &mut dropped);
    }
    content.push_str(&format!("0 0 0 RG 0.6 w {left:.2} {:.2} m {:.2} {:.2} l S\n", top - 4.0, left + table_width, top - 4.0));
    for (row_index, tool) in document.tools.iter().enumerate() {
        let baseline = top - row * (row_index as f64 + 1.0);
        let cells = [
            tool.id.clone(),
            tool.name.clone(),
            number(f64::from(tool.number)),
            number(tool.diameter),
            number(tool.radius()),
            number(f64::from(tool.flutes)),
            number(tool.spindle),
            number(tool.feed),
            number(tool.plunge_feed),
        ];
        for (index, cell) in cells.iter().enumerate() {
            push_pdf_text(&mut content, column_x(index) + 3.0, baseline, 8.0, "0", cell, &mut dropped);
        }
        content.push_str(&format!(
            "0.6 0.6 0.6 RG 0.4 w {left:.2} {:.2} m {:.2} {:.2} l S\n",
            baseline - 4.0,
            left + table_width,
            baseline - 4.0
        ));
    }
    if document.tools.is_empty() {
        push_pdf_text(&mut content, left + 3.0, top - row, 8.0, "0.4", "no tools declared", &mut dropped);
    }
    let title_top = PDF_MARGIN + PDF_TITLE_BLOCK;
    push_pdf_block(
        &mut content,
        document,
        id,
        "OpenCAM tool list drawing (PDF)",
        &format!("{} tool(s)", document.tools.len()),
        title_top,
        &mut dropped,
    );
    finish_pdf_page(&content, dropped)
}

/// Renders the nominal operation geometry on the stock as an SVG setup sheet.
pub fn stock_setup_svg(document: &CamDocument, id: &str) -> Drawing {
    let frame = setup_frame(document);
    let legend = setup_legend(document);
    let legend_height = legend.len() as f64 * LEGEND_ROW_MM + LEGEND_GAP_MM;
    let sheet_width = frame.width.max(MIN_SHEET_WIDTH_MM);
    let sheet_height = frame.height + legend_height + TITLE_HEIGHT_MM;
    let map_x = |x: f64| PAD_MM + (x - frame.min_x);
    let map_y = |y: f64| PAD_MM + (frame.max_y - y);

    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {0} {1}\" width=\"{2}\" height=\"{3}\">\n",
        number(sheet_width),
        number(sheet_height),
        number(sheet_width * PX_PER_MM),
        number(sheet_height * PX_PER_MM)
    ));
    svg.push_str(&format!("  <title>OpenCAM stock setup {}</title>\n", escape_xml(id)));
    svg.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        number(sheet_width),
        number(sheet_height)
    ));
    svg.push_str("  <g data-stock=\"true\">\n");
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{STOCK_COLOR}\" stroke-width=\"0.35\"/>\n",
        number(map_x(0.0)),
        number(map_y(document.stock.depth)),
        number(document.stock.width),
        number(document.stock.depth)
    ));
    svg.push_str("  </g>\n");
    for (index, operation) in document.operations.iter().enumerate() {
        let (hex, _) = palette(index);
        svg.push_str(&format!("  <g data-operation=\"{}\" data-index=\"{index}\">\n", escape_xml(&operation.id)));
        match operation.kind {
            OperationKind::Face => svg.push_str(&format!(
                "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"0.35\" stroke-dasharray=\"1.5 1\"/>\n",
                number(map_x(0.0)),
                number(map_y(document.stock.depth)),
                number(document.stock.width),
                number(document.stock.depth)
            )),
            OperationKind::Contour | OperationKind::Pocket => {
                if operation.shape.as_deref() == Some("circle") {
                    svg.push_str(&format!(
                        "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\"/>\n",
                        number(map_x(operation.x + operation.width / 2.0)),
                        number(map_y(operation.y + operation.height / 2.0)),
                        number(operation.width / 2.0)
                    ));
                }
                else {
                    svg.push_str(&format!(
                        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\"/>\n",
                        number(map_x(operation.x)),
                        number(map_y(operation.y + operation.height)),
                        number(operation.width),
                        number(operation.height)
                    ));
                }
                for island in &operation.islands {
                    svg.push_str(&format!(
                        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{ISLAND_COLOR}\" stroke-width=\"{ISLAND_WIDTH_MM}\"/>\n",
                        number(map_x(island.x)),
                        number(map_y(island.y + island.height)),
                        number(island.width),
                        number(island.height)
                    ));
                }
            }
            OperationKind::Engrave => {
                if operation.path.len() >= 2 {
                    let mut points: Vec<String> =
                        operation.path.iter().map(|point| format!("{},{}", number(map_x(point.x)), number(map_y(point.y)))).collect();
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
                            "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{DRILL_RING_WIDTH_MM}\"/>\n",
                            number(map_x(hole.x)),
                            number(map_y(hole.y)),
                            number(radius)
                        ));
                    }
                    else {
                        svg.push_str(&format!(
                            "    <circle cx=\"{}\" cy=\"{}\" r=\"0.6\" fill=\"{hex}\"/>\n",
                            number(map_x(hole.x)),
                            number(map_y(hole.y))
                        ));
                    }
                }
            }
        }
        let (label_x, label_y) = match operation.kind {
            OperationKind::Face => (0.0, document.stock.depth),
            OperationKind::Contour | OperationKind::Pocket => (operation.x, operation.y + operation.height),
            OperationKind::Engrave => {
                (operation.path.first().map_or(0.0, |point| point.x), operation.path.first().map_or(0.0, |point| point.y))
            }
            OperationKind::Drill => {
                let hole = operation.resolved_holes().into_iter().next().unwrap_or(crate::model::Hole { x: 0.0, y: 0.0 });
                (hole.x, hole.y)
            }
        };
        svg.push_str(&format!(
            "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"2.4\" fill=\"{INK}\">#{index} {}</text>\n",
            number(map_x(label_x) + 0.8),
            number(map_y(label_y) + 3.0 + f64::from(index as u32) * 2.6),
            escape_xml(&operation_name(operation.kind, &operation.id))
        ));
        svg.push_str("  </g>\n");
    }
    let legend_top = frame.height + LEGEND_GAP_MM;
    svg.push_str("  <g data-legend=\"true\">\n");
    for (row, entry) in legend.iter().enumerate() {
        let baseline = legend_top + 3.0 + row as f64 * LEGEND_ROW_MM;
        svg.push_str(&format!(
            "    <line x1=\"2\" y1=\"{}\" x2=\"9\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.6\"/>\n",
            number(baseline - 0.8),
            number(baseline - 0.8),
            entry.colour.hex
        ));
        svg.push_str(&format!(
            "    <text x=\"10.5\" y=\"{}\" font-family=\"monospace\" font-size=\"2.4\" fill=\"{INK}\">{}</text>\n",
            number(baseline),
            escape_xml(&entry.label)
        ));
    }
    svg.push_str("  </g>\n");
    push_svg_block(
        &mut svg,
        document,
        id,
        "OpenCAM stock setup drawing (SVG)",
        &format!("{} operation(s)", document.operations.len()),
        sheet_width,
        sheet_height,
    );
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the nominal operation geometry on the stock as one PDF page.
pub fn stock_setup_pdf(document: &CamDocument, id: &str) -> Drawing {
    let frame = setup_frame(document);
    let legend = setup_legend(document);
    let legend_height = legend.len() as f64 * PDF_LEGEND_ROW + 8.0;
    let title_top = PDF_MARGIN + PDF_TITLE_BLOCK;
    let legend_top = title_top + legend_height;
    let area_width = PDF_PAGE_WIDTH - 2.0 * PDF_MARGIN;
    let area_height = PDF_PAGE_HEIGHT - PDF_MARGIN - legend_top;
    let scale = (area_width / frame.width).min(area_height / frame.height).max(0.01);
    let offset_x = PDF_MARGIN + (area_width - frame.width * scale) / 2.0;
    let offset_y = legend_top + (area_height - frame.height * scale) / 2.0;
    let map_x = |x: f64| offset_x + (x - frame.min_x) * scale;
    let map_y = |y: f64| offset_y + (frame.max_y - y) * scale;

    let mut dropped = 0usize;
    let mut content = String::new();
    content.push_str(&format!("1 1 1 rg 0 0 {PDF_PAGE_WIDTH:.2} {PDF_PAGE_HEIGHT:.2} re f\n"));
    content.push_str(&format!(
        "{} RG 0.6 w [2 2] 0 d {:.2} {:.2} {:.2} {:.2} re S [] 0 d\n",
        rgb(STOCK_RGB),
        map_x(0.0),
        map_y(document.stock.depth),
        document.stock.width * scale,
        document.stock.depth * scale
    ));
    for (index, operation) in document.operations.iter().enumerate() {
        let colour = palette(index).1;
        match operation.kind {
            OperationKind::Face => content.push_str(&format!(
                "{} RG 0.6 w [2 2] 0 d {:.2} {:.2} {:.2} {:.2} re S [] 0 d\n",
                rgb(colour),
                map_x(0.0),
                map_y(document.stock.depth),
                document.stock.width * scale,
                document.stock.depth * scale
            )),
            OperationKind::Contour | OperationKind::Pocket => {
                if operation.shape.as_deref() == Some("circle") {
                    push_pdf_circle(
                        &mut content,
                        map_x(operation.x + operation.width / 2.0),
                        map_y(operation.y + operation.height / 2.0),
                        operation.width / 2.0 * scale,
                        colour,
                        true,
                    );
                }
                else {
                    push_pdf_rect(
                        &mut content,
                        map_x(operation.x),
                        map_y(operation.y + operation.height),
                        operation.width * scale,
                        operation.height * scale,
                        colour,
                    );
                }
                for island in &operation.islands {
                    push_pdf_rect(
                        &mut content,
                        map_x(island.x),
                        map_y(island.y + island.height),
                        island.width * scale,
                        island.height * scale,
                        ISLAND_RGB,
                    );
                }
            }
            OperationKind::Engrave => {
                if operation.path.len() >= 2 {
                    content.push_str(&format!("{} RG 1.1 w 1 J 1 j ", rgb(colour)));
                    for (position, point) in operation.path.iter().enumerate() {
                        let operator = if position == 0 { "m" } else { "l" };
                        content.push_str(&format!("{:.2} {:.2} {operator} ", map_x(point.x), map_y(point.y)));
                    }
                    if operation.closed {
                        content.push_str("h ");
                    }
                    content.push_str("S\n");
                }
            }
            OperationKind::Drill => {
                let radius = document.tools.iter().find(|tool| tool.id == operation.tool).map(Tool::radius);
                for hole in operation.resolved_holes() {
                    push_pdf_circle(
                        &mut content,
                        map_x(hole.x),
                        map_y(hole.y),
                        radius.map_or(1.5, |r| r * scale),
                        colour,
                        radius.is_some(),
                    );
                }
            }
        }
        push_pdf_text(
            &mut content,
            map_x(operation.x) + 2.0,
            map_y(operation.y + operation.height) - 2.0,
            7.0,
            "0",
            &format!("#{} {}", index + 1, operation_name(operation.kind, &operation.id)),
            &mut dropped,
        );
    }
    let mut row_y = legend_top - 7.0;
    for entry in &legend {
        content.push_str(&format!(
            "{0} RG 0.9 w {1:.2} {row_y:.2} m {2:.2} {row_y:.2} l S\n",
            rgb(entry.colour.rgb),
            PDF_MARGIN,
            PDF_MARGIN + 18.0
        ));
        push_pdf_text(&mut content, PDF_MARGIN + 24.0, row_y - 2.0, 7.0, "0", &entry.label, &mut dropped);
        row_y -= PDF_LEGEND_ROW;
    }
    push_pdf_block(
        &mut content,
        document,
        id,
        "OpenCAM stock setup drawing (PDF)",
        &format!("{} operation(s)", document.operations.len()),
        title_top,
        &mut dropped,
    );
    finish_pdf_page(&content, dropped)
}

/// Renders every cutting chain coloured by its Z level as an SVG depth map.
pub fn depth_map_svg(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    let drawings = project_all(document, report);
    let frame = frame(document, &drawings);
    let legend = operation_rows(document, report, &drawings);
    let legend_height = legend.len() as f64 * LEGEND_ROW_MM + LEGEND_GAP_MM;
    let sheet_width = frame.width.max(MIN_SHEET_WIDTH_MM);
    let sheet_height = frame.height + legend_height + TITLE_HEIGHT_MM;
    let map_x = |x: f64| PAD_MM + (x - frame.min_x);
    let map_y = |y: f64| PAD_MM + (frame.max_y - y);

    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {0} {1}\" width=\"{2}\" height=\"{3}\">\n",
        number(sheet_width),
        number(sheet_height),
        number(sheet_width * PX_PER_MM),
        number(sheet_height * PX_PER_MM)
    ));
    svg.push_str(&format!("  <title>OpenCAM depth map {}</title>\n", escape_xml(id)));
    svg.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        number(sheet_width),
        number(sheet_height)
    ));
    svg.push_str("  <g data-stock=\"true\">\n");
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{STOCK_COLOR}\" stroke-width=\"0.35\"/>\n",
        number(map_x(0.0)),
        number(map_y(document.stock.depth)),
        number(document.stock.width),
        number(document.stock.depth)
    ));
    svg.push_str("  </g>\n");
    let mut annotations = 0usize;
    for (index, drawing) in drawings.iter().enumerate() {
        let levels = chain_levels(drawing);
        svg.push_str(&format!("  <g data-depth-map=\"true\" data-index=\"{index}\">\n"));
        for (level, z) in levels.iter().enumerate() {
            let (hex, _) = shade(palette(index).1, shade_factor(level, levels.len()));
            for chain in drawing.cuts.iter().filter(|chain| level_index(&levels, chain.z) == level) {
                if chain.points.len() < 2 {
                    continue;
                }
                let points = chain
                    .points
                    .iter()
                    .map(|point| format!("{},{}", number(map_x(point.0)), number(map_y(point.1))))
                    .collect::<Vec<_>>()
                    .join(" ");
                svg.push_str(&format!(
                    "    <polyline points=\"{points}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n"
                ));
            }
            if let Some(chain) = drawing.cuts.iter().find(|chain| level_index(&levels, chain.z) == level) {
                let (x, y) = chain.points.first().copied().unwrap_or((0.0, 0.0));
                svg.push_str(&format!(
                    "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"2.2\" fill=\"{INK}\">Z{}</text>\n",
                    number(map_x(x) + 0.8),
                    number(map_y(y) + 2.0 + f64::from(annotations as u32) * 2.6),
                    number(*z)
                ));
                annotations += 1;
            }
        }
        svg.push_str("  </g>\n");
    }
    let legend_top = frame.height + LEGEND_GAP_MM;
    svg.push_str("  <g data-legend=\"true\">\n");
    for (row, entry) in legend.iter().enumerate() {
        let baseline = legend_top + 3.0 + row as f64 * LEGEND_ROW_MM;
        svg.push_str(&format!(
            "    <line x1=\"2\" y1=\"{}\" x2=\"9\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.6\"/>\n",
            number(baseline - 0.8),
            number(baseline - 0.8),
            entry.colour.hex
        ));
        svg.push_str(&format!(
            "    <text x=\"10.5\" y=\"{}\" font-family=\"monospace\" font-size=\"2.4\" fill=\"{INK}\">{}</text>\n",
            number(baseline),
            escape_xml(&entry.label)
        ));
    }
    svg.push_str("  </g>\n");
    push_svg_block(
        &mut svg,
        document,
        id,
        "OpenCAM depth map drawing (SVG)",
        &format!("{} operation(s)", report.operations.len()),
        sheet_width,
        sheet_height,
    );
    svg.push_str("</svg>\n");
    Drawing { text: svg, dropped: 0 }
}

/// Renders the Z-level coloured cutting chains as one PDF page.
pub fn depth_map_pdf(document: &CamDocument, report: &ToolpathReport, id: &str) -> Drawing {
    let drawings = project_all(document, report);
    let frame = frame(document, &drawings);
    let legend = operation_rows(document, report, &drawings);
    let legend_height = legend.len() as f64 * PDF_LEGEND_ROW + 8.0;
    let title_top = PDF_MARGIN + PDF_TITLE_BLOCK;
    let legend_top = title_top + legend_height;
    let area_width = PDF_PAGE_WIDTH - 2.0 * PDF_MARGIN;
    let area_height = PDF_PAGE_HEIGHT - PDF_MARGIN - legend_top;
    let scale = (area_width / frame.width).min(area_height / frame.height).max(0.01);
    let offset_x = PDF_MARGIN + (area_width - frame.width * scale) / 2.0;
    let offset_y = legend_top + (area_height - frame.height * scale) / 2.0;
    let map_x = |x: f64| offset_x + (x - frame.min_x) * scale;
    let map_y = |y: f64| offset_y + (frame.max_y - y) * scale;

    let mut dropped = 0usize;
    let mut content = String::new();
    content.push_str(&format!("1 1 1 rg 0 0 {PDF_PAGE_WIDTH:.2} {PDF_PAGE_HEIGHT:.2} re f\n"));
    content.push_str(&format!(
        "{} RG 0.6 w {:.2} {:.2} {:.2} {:.2} re S\n",
        rgb(STOCK_RGB),
        map_x(0.0),
        map_y(document.stock.depth),
        document.stock.width * scale,
        document.stock.depth * scale
    ));
    for (index, drawing) in drawings.iter().enumerate() {
        let levels = chain_levels(drawing);
        for (level, z) in levels.iter().enumerate() {
            let colour = shade(palette(index).1, shade_factor(level, levels.len())).1;
            for chain in drawing.cuts.iter().filter(|chain| level_index(&levels, chain.z) == level) {
                if chain.points.len() < 2 {
                    continue;
                }
                content.push_str(&format!("{} RG 1.1 w 1 J 1 j ", rgb(colour)));
                for (position, (x, y)) in chain.points.iter().enumerate() {
                    let operator = if position == 0 { "m" } else { "l" };
                    content.push_str(&format!("{:.2} {:.2} {operator} ", map_x(*x), map_y(*y)));
                }
                content.push_str("S\n");
            }
            if let Some(chain) = drawing.cuts.iter().find(|chain| level_index(&levels, chain.z) == level) {
                let (x, y) = chain.points.first().copied().unwrap_or((0.0, 0.0));
                push_pdf_text(
                    &mut content,
                    map_x(x) + 2.0,
                    map_y(y) + 2.0,
                    6.5,
                    "0",
                    &format!("Z{}", number(*z)),
                    &mut dropped,
                );
            }
        }
    }
    let mut row_y = legend_top - 7.0;
    for entry in &legend {
        content.push_str(&format!(
            "{0} RG 0.9 w {1:.2} {row_y:.2} m {2:.2} {row_y:.2} l S\n",
            rgb(entry.colour.rgb),
            PDF_MARGIN,
            PDF_MARGIN + 18.0
        ));
        push_pdf_text(&mut content, PDF_MARGIN + 24.0, row_y - 2.0, 7.0, "0", &entry.label, &mut dropped);
        row_y -= PDF_LEGEND_ROW;
    }
    push_pdf_block(
        &mut content,
        document,
        id,
        "OpenCAM depth map drawing (PDF)",
        &format!("{} operation(s)", report.operations.len()),
        title_top,
        &mut dropped,
    );
    finish_pdf_page(&content, dropped)
}

/// Shared PDF page assembly of the non-toolpath sheets.
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

fn push_pdf_rect(content: &mut String, x: f64, y: f64, width: f64, height: f64, colour: [u8; 3]) {
    content.push_str(&format!(
        "{} RG 1 w {:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
        rgb(colour),
        x,
        y,
        x + width,
        y,
        x + width,
        y + height,
        x,
        y + height
    ));
}

/// Shared title block of the non-toolpath SVG sheets.
fn push_svg_block(
    svg: &mut String,
    document: &CamDocument,
    id: &str,
    subtitle: &str,
    note: &str,
    sheet_width: f64,
    sheet_height: f64,
) {
    let top = sheet_height - TITLE_HEIGHT_MM;
    svg.push_str("  <g data-title-block=\"true\">\n");
    svg.push_str(&format!(
        "    <line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
        number(top),
        number(sheet_width),
        number(top)
    ));
    for fraction in [0.45, 0.75] {
        let cursor = sheet_width * fraction;
        svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
            number(cursor),
            number(top),
            number(cursor),
            number(sheet_height)
        ));
    }
    let cell = |svg: &mut String, x: f64, y: f64, size: f64, fill: &str, text: &str| {
        svg.push_str(&format!(
            "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"{}\" fill=\"{fill}\">{}</text>\n",
            number(x),
            number(y),
            number(size),
            escape_xml(text)
        ));
    };
    cell(svg, 1.5, top + 6.0, 3.4, INK, id);
    cell(svg, 1.5, top + 11.5, 2.2, MUTED, subtitle);
    let mid = sheet_width * 0.45 + 1.5;
    cell(svg, mid, top + 6.0, 2.6, INK, &format!("units: {}", document.units));
    cell(svg, mid, top + 11.5, 2.2, MUTED, &format!("REV {}", document.revision));
    let right = sheet_width * 0.75 + 1.5;
    cell(svg, right, top + 6.0, 2.4, INK, note);
    cell(
        svg,
        right,
        top + 11.5,
        2.2,
        MUTED,
        &format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)),
    );
    svg.push_str("  </g>\n");
}

/// Shared title block of the non-toolpath PDF sheets.
fn push_pdf_block(
    content: &mut String,
    document: &CamDocument,
    id: &str,
    subtitle: &str,
    note: &str,
    title_top: f64,
    dropped: &mut usize,
) {
    let left = PDF_MARGIN;
    let right = PDF_PAGE_WIDTH - PDF_MARGIN;
    content.push_str(&format!("0 0 0 RG 0.6 w {left:.2} {title_top:.2} m {right:.2} {title_top:.2} l S\n"));
    for fraction in [0.45, 0.75] {
        let cursor = left + (right - left) * fraction;
        content.push_str(&format!("{cursor:.2} {PDF_MARGIN:.2} m {cursor:.2} {title_top:.2} l S\n"));
    }
    push_pdf_text(content, left + 3.0, title_top - 14.0, 9.0, "0", id, dropped);
    push_pdf_text(content, left + 3.0, title_top - 26.0, 6.5, "0.4", subtitle, dropped);
    let mid = left + (right - left) * 0.45 + 3.0;
    push_pdf_text(content, mid, title_top - 14.0, 7.0, "0", &format!("units: {}", document.units), dropped);
    push_pdf_text(content, mid, title_top - 26.0, 6.5, "0.4", &format!("REV {}", document.revision), dropped);
    let right_x = left + (right - left) * 0.75 + 3.0;
    push_pdf_text(content, right_x, title_top - 14.0, 6.5, "0.4", note, dropped);
    push_pdf_text(
        content,
        right_x,
        title_top - 26.0,
        6.5,
        "0.4",
        &format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)),
        dropped,
    );
}

/// Bounding frame of the stock and every nominal operation geometry.
fn setup_frame(document: &CamDocument) -> Frame {
    let mut min_x = 0.0_f64;
    let mut max_x = document.stock.width.max(1.0);
    let mut min_y = 0.0_f64;
    let mut max_y = document.stock.depth.max(1.0);
    let mut include = |x: f64, y: f64| {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    };
    for operation in &document.operations {
        match operation.kind {
            OperationKind::Face => {}
            OperationKind::Contour | OperationKind::Pocket => {
                include(operation.x, operation.y);
                include(operation.x + operation.width, operation.y + operation.height);
                for island in &operation.islands {
                    include(island.x, island.y);
                    include(island.x + island.width, island.y + island.height);
                }
            }
            OperationKind::Engrave => {
                for point in &operation.path {
                    include(point.x, point.y);
                }
            }
            OperationKind::Drill => {
                let radius = document.tools.iter().find(|tool| tool.id == operation.tool).map_or(0.0, Tool::radius);
                for hole in operation.resolved_holes() {
                    include(hole.x - radius, hole.y - radius);
                    include(hole.x + radius, hole.y + radius);
                }
            }
        }
    }
    Frame {
        min_x,
        min_y,
        max_y,
        width: (max_x - min_x).max(1.0) + 2.0 * PAD_MM,
        height: (max_y - min_y).max(1.0) + 2.0 * PAD_MM,
    }
}

/// One legend row per operation of the setup sheet.
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
                    "#{} {}  depth {} mm  {detail}",
                    index + 1,
                    operation_name(operation.kind, &operation.id),
                    number(operation.depth)
                ),
            }
        })
        .collect()
}

/// `kind id`, printed once when a default id already names the kind.
fn operation_name(kind: OperationKind, id: &str) -> String {
    if kind.as_str() == id { id.to_string() } else { format!("{} {}", kind.as_str(), id) }
}

/// One continuous cutting chain and the Z height it was cut at.
struct CutChain {
    z: f64,
    points: Vec<(f64, f64)>,
}

/// Projected primitives of one operation, in millimetres.
struct OpDrawing {
    cuts: Vec<CutChain>,
    rapids: Vec<((f64, f64), (f64, f64))>,
    tabs: Vec<((f64, f64), (f64, f64))>,
    plunges: Vec<(f64, f64)>,
    holes: Vec<(f64, f64)>,
    hole_radius: Option<f64>,
    start: Option<(f64, f64)>,
}

/// Radius of the drilling tool, or `None` for a non-drill operation.
fn drill_radius(document: &CamDocument, operation: &OperationPath) -> Option<f64> {
    if operation.kind != OperationKind::Drill {
        return None;
    }
    document.tools.iter().find(|tool| tool.id == operation.tool).map(|tool| tool.radius())
}

/// Projects one operation. `previous` is the XY the machine actually starts
/// from: the end of the previous operation, or `None` for the first one.
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
                // The report starts every operation at the origin; the sheet
                // draws the traverse from where the previous operation ended.
                let from = if first_rapid {
                    previous.unwrap_or((movement.from.x, movement.from.y))
                }
                else {
                    (movement.from.x, movement.from.y)
                };
                first_rapid = false;
                flush(&mut chain, &mut chain_z, &mut drawing.cuts);
                if (from.0 - movement.to.x).abs() > 1e-9 || (from.1 - movement.to.y).abs() > 1e-9 {
                    // A horizontal rapid below the clearance plane crosses a holding tab.
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
    // Chain consecutive lifted chords into one marker per tab, then drop the
    // repeats the remaining Z levels produce at the same XY interval.
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

/// Samples one arc at the engine's own ring resolution (72 chords per turn).
///
/// The endpoint is taken from the move itself, so the polyline closes exactly
/// on the point the generator emitted.
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

/// Bounding frame of the stock and every drawable point, with drawing padding.
struct Frame {
    min_x: f64,
    min_y: f64,
    max_y: f64,
    width: f64,
    height: f64,
}

fn frame(document: &CamDocument, drawings: &[OpDrawing]) -> Frame {
    let mut min_x = 0.0_f64;
    let mut max_x = document.stock.width.max(1.0);
    let mut min_y = 0.0_f64;
    let mut max_y = document.stock.depth.max(1.0);
    let mut include = |x: f64, y: f64| {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    };
    for drawing in drawings {
        for point in drawing.cuts.iter().flat_map(|chain| chain.points.iter()) {
            include(point.0, point.1);
        }
        for point in drawing.holes.iter().chain(drawing.plunges.iter()) {
            include(point.0, point.1);
        }
        if let Some(radius) = drawing.hole_radius {
            for point in &drawing.holes {
                include(point.0 - radius, point.1 - radius);
                include(point.0 + radius, point.1 + radius);
            }
        }
        for (from, to) in &drawing.rapids {
            include(from.0, from.1);
            include(to.0, to.1);
        }
    }
    Frame {
        min_x,
        min_y,
        max_y,
        width: (max_x - min_x).max(1.0) + 2.0 * PAD_MM,
        height: (max_y - min_y).max(1.0) + 2.0 * PAD_MM,
    }
}

#[derive(Clone, Copy)]
enum Style {
    Solid,
    Dashed,
    Dot,
    Ring,
}

/// A resolved row colour, so shaded per-level colours can appear in a legend.
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

fn fixed_colour(hex: &str, rgb: [u8; 3]) -> RowColour {
    RowColour { hex: hex.to_string(), rgb }
}

fn legend(document: &CamDocument, report: &ToolpathReport, drawings: &[OpDrawing]) -> Vec<LegendRow> {
    let mut rows = vec![
        LegendRow { colour: fixed_colour(INK, INK_RGB), style: Style::Solid, label: "cutting move (G1/G2/G3)".to_string() },
        LegendRow { colour: fixed_colour(STOCK_COLOR, STOCK_RGB), style: Style::Dashed, label: "rapid move (G0)".to_string() },
        LegendRow { colour: fixed_colour(INK, INK_RGB), style: Style::Dot, label: "plunge / start point".to_string() },
        LegendRow { colour: fixed_colour(INK, INK_RGB), style: Style::Ring, label: "drill hole (tool diameter)".to_string() },
    ];
    if document.operations.iter().any(|operation| operation.kind == OperationKind::Pocket && !operation.islands.is_empty()) {
        rows.push(LegendRow {
            colour: fixed_colour(ISLAND_COLOR, ISLAND_RGB),
            style: Style::Solid,
            label: "pocket island (nominal outline)".to_string(),
        });
    }
    if drawings.iter().any(|drawing| !drawing.tabs.is_empty()) {
        rows.push(LegendRow {
            colour: fixed_colour(TAB_COLOR, TAB_RGB),
            style: Style::Solid,
            label: "holding tab (lifted traverse)".to_string(),
        });
    }
    rows.extend(operation_rows(document, report, drawings));
    rows
}

/// One legend row per operation, or one shaded row per Z level when an
/// operation cuts at several depths.
fn operation_rows(document: &CamDocument, report: &ToolpathReport, drawings: &[OpDrawing]) -> Vec<LegendRow> {
    let mut rows = Vec::new();
    for (index, operation) in report.operations.iter().enumerate() {
        let detail = match document.tools.iter().find(|tool| tool.id == operation.tool) {
            Some(tool) => format!("tool {} d{} r{}", tool.id, number(tool.diameter), number(tool.radius())),
            None => format!("tool {}", operation.tool),
        };
        // A default operation id repeats the kind, so "face face" prints once.
        let name = operation_name(operation.kind, &operation.operation);
        let levels = chain_levels(&drawings[index]);
        if levels.len() <= 1 {
            let (hex, rgb) = palette(index);
            rows.push(LegendRow {
                colour: fixed_colour(hex, rgb),
                style: Style::Solid,
                label: format!("#{} {}  {detail}", index + 1, name),
            });
        }
        else {
            for (level, z) in levels.iter().enumerate() {
                let (hex, rgb) = shade(palette(index).1, shade_factor(level, levels.len()));
                rows.push(LegendRow {
                    colour: RowColour { hex, rgb },
                    style: Style::Solid,
                    label: format!(
                        "#{} {}  Z{} depth {} mm  {detail}",
                        index + 1,
                        name,
                        number(*z),
                        number(document.stock.height - z),
                    ),
                });
            }
        }
    }
    rows
}

fn palette(index: usize) -> (&'static str, [u8; 3]) {
    PALETTE[index % PALETTE.len()]
}

/// Projects every operation, threading the end of one operation into the
/// first rapid of the next so the sheet shows the real traverse between them.
fn project_all(document: &CamDocument, report: &ToolpathReport) -> Vec<OpDrawing> {
    let mut drawings = Vec::with_capacity(report.operations.len());
    let mut previous: Option<(f64, f64)> = None;
    for operation in &report.operations {
        drawings.push(project(operation, drill_radius(document, operation), report.safe_z, previous));
        previous = operation.segments.last().map(|movement| (movement.to.x, movement.to.y)).or(previous);
    }
    drawings
}

/// Distinct chain Z levels of one operation, in cutting order.
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

/// The first level keeps the full colour and the deepest fades to 35 %.
fn shade_factor(level: usize, count: usize) -> f64 {
    if count <= 1 { 1.0 } else { 1.0 - 0.65 * (f64::from(level as u32) / f64::from(count as u32 - 1)) }
}

fn shade(colour: [u8; 3], factor: f64) -> (String, [u8; 3]) {
    let rgb = [
        (f64::from(colour[0]) * factor).round() as u8,
        (f64::from(colour[1]) * factor).round() as u8,
        (f64::from(colour[2]) * factor).round() as u8,
    ];
    (format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]), rgb)
}

fn push_svg_title_block(
    svg: &mut String,
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    sheet_width: f64,
    sheet_height: f64,
) {
    let top = sheet_height - TITLE_HEIGHT_MM;
    svg.push_str("  <g data-title-block=\"true\">\n");
    svg.push_str(&format!(
        "    <line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
        number(top),
        number(sheet_width),
        number(top)
    ));
    let columns = [0.40, 0.22, 0.16, 0.22];
    let mut cursor = 0.0;
    for fraction in &columns[1..] {
        cursor += sheet_width * fraction;
        svg.push_str(&format!(
            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
            number(cursor),
            number(top),
            number(cursor),
            number(sheet_height)
        ));
    }
    let cell_x = |index: usize| 1.5 + sheet_width * columns[..index].iter().sum::<f64>();
    let line = |svg: &mut String, x: f64, y: f64, size: f64, fill: &str, text: &str| {
        svg.push_str(&format!(
            "    <text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"{}\" fill=\"{fill}\">{}</text>\n",
            number(x),
            number(y),
            number(size),
            escape_xml(text)
        ));
    };
    line(svg, cell_x(0), top + 6.0, 3.4, INK, id);
    line(svg, cell_x(0), top + 11.5, 2.2, MUTED, "OpenCAM toolpath drawing (SVG)");
    line(svg, cell_x(1), top + 6.0, 2.6, INK, &format!("units: {}", document.units));
    line(svg, cell_x(1), top + 11.5, 2.2, MUTED, &format!("{} operation(s)", report.operations.len()));
    line(svg, cell_x(2), top + 6.0, 2.8, INK, &format!("REV {}", document.revision));
    line(svg, cell_x(2), top + 11.5, 2.2, MUTED, &format!("est {} s", number(report.estimated_seconds)));
    line(svg, cell_x(3), top + 6.0, 2.2, MUTED, "scale 1:1 (8 px/mm)");
    line(
        svg,
        cell_x(3),
        top + 11.5,
        2.2,
        MUTED,
        &format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)),
    );
    svg.push_str("  </g>\n");
}

#[allow(clippy::too_many_arguments)]
fn push_pdf_title_block(
    content: &mut String,
    document: &CamDocument,
    report: &ToolpathReport,
    id: &str,
    title_top: f64,
    percent: f64,
    dropped: &mut usize,
) {
    let left = PDF_MARGIN;
    let right = PDF_PAGE_WIDTH - PDF_MARGIN;
    let width = right - left;
    content.push_str(&format!("0 0 0 RG 0.6 w {left:.2} {title_top:.2} m {right:.2} {title_top:.2} l S\n"));
    let columns = [0.40, 0.22, 0.16, 0.22];
    let mut cursor = left;
    for fraction in &columns[1..] {
        cursor += width * fraction;
        content.push_str(&format!("{cursor:.2} {PDF_MARGIN:.2} m {cursor:.2} {title_top:.2} l S\n"));
    }
    let cell_x = |index: usize| left + width * columns[..index].iter().sum::<f64>() + 3.0;
    let line1 = title_top - 14.0;
    let line2 = title_top - 26.0;
    let line3 = title_top - 37.0;
    push_pdf_text(content, cell_x(0), line1, 9.0, "0", id, dropped);
    push_pdf_text(content, cell_x(0), line2, 6.5, "0.4", "OpenCAM toolpath drawing (PDF)", dropped);
    push_pdf_text(content, cell_x(1), line1, 7.0, "0", &format!("units: {}", document.units), dropped);
    push_pdf_text(content, cell_x(1), line2, 6.5, "0.4", &format!("{} operation(s)", report.operations.len()), dropped);
    push_pdf_text(content, cell_x(2), line1, 7.5, "0", &format!("REV {}", document.revision), dropped);
    push_pdf_text(content, cell_x(2), line2, 6.5, "0.4", &format!("est {} s", number(report.estimated_seconds)), dropped);
    push_pdf_text(content, cell_x(3), line1, 6.5, "0.4", &format!("scale {percent:.0}%"), dropped);
    push_pdf_text(
        content,
        cell_x(3),
        line2,
        6.5,
        "0.4",
        &format!("stock {} x {} mm", number(document.stock.width), number(document.stock.depth)),
        dropped,
    );
    push_pdf_text(content, cell_x(3), line3, 5.5, "0.4", "A4 landscape", dropped);
}

fn push_pdf_text(content: &mut String, x: f64, y: f64, size: f64, gray: &str, text: &str, dropped: &mut usize) {
    let (literal, count) = pdf_text(text);
    *dropped += count;
    content.push_str(&format!("{gray} {gray} {gray} rg BT /F1 {size:.2} Tf {x:.2} {y:.2} Td {literal} Tj ET\n"));
}

fn push_pdf_circle(content: &mut String, cx: f64, cy: f64, radius: f64, colour: [u8; 3], stroke_only: bool) {
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

fn rgb(colour: [u8; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", f64::from(colour[0]) / 255.0, f64::from(colour[1]) / 255.0, f64::from(colour[2]) / 255.0)
}

/// Escapes a string for an XML text node or attribute.
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Escapes a string for a PDF literal string and counts the characters the
/// base-14 font cannot encode; those are written as `?`.
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

/// Formats a millimetre coordinate with at most three decimals and no `-0`.
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
