//! Vector drawing export.
//!
//! [`svg`] and [`pdf`] draw the stock outline and the moves [`crate::toolpath`]
//! already generated: cutting polylines, dashed rapid traverses, plunge and
//! start markers, one legend row per operation and a title block. Nothing here
//! regenerates a toolpath, and every sheet is deterministic: coordinates are
//! rounded to three decimals, operations keep document order and no timestamp
//! is written.

use crate::{
    model::CamDocument,
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
const INK: &str = "#1f2933";
const MUTED: &str = "#64748b";
const PATH_WIDTH_MM: f64 = 0.5;
const RAPID_WIDTH_MM: f64 = 0.3;
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
    let drawings: Vec<OpDrawing> = report.operations.iter().map(project).collect();
    let frame = frame(document, &drawings);
    let legend = legend(document, report);
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

    for (index, (operation, drawing)) in report.operations.iter().zip(&drawings).enumerate() {
        let (hex, _) = palette(index);
        svg.push_str(&format!(
            "  <g data-operation=\"{}\" data-index=\"{index}\" data-tool=\"{}\">\n",
            escape_xml(&operation.operation),
            escape_xml(&operation.tool)
        ));
        for chain in &drawing.cuts {
            if chain.len() < 2 {
                continue;
            }
            let points =
                chain.iter().map(|(x, y)| format!("{},{}", number(map_x(*x)), number(map_y(*y)))).collect::<Vec<_>>().join(" ");
            svg.push_str(&format!(
                "    <polyline points=\"{points}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{PATH_WIDTH_MM}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n"
            ));
        }
        for (from, to) in &drawing.rapids {
            svg.push_str(&format!(
                "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{STOCK_COLOR}\" stroke-width=\"{RAPID_WIDTH_MM}\" stroke-dasharray=\"1.5 1\"/>\n",
                number(map_x(from.0)),
                number(map_y(from.1)),
                number(map_x(to.0)),
                number(map_y(to.1))
            ));
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
        let color = legend_hex(entry);
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
    let drawings: Vec<OpDrawing> = report.operations.iter().map(project).collect();
    let frame = frame(document, &drawings);
    let legend = legend(document, report);
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
        rgb([148, 163, 184]),
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

    for (index, drawing) in drawings.iter().enumerate() {
        let colour = palette(index).1;
        for chain in &drawing.cuts {
            if chain.len() < 2 {
                continue;
            }
            content.push_str(&format!("{} RG 1.1 w 1 J 1 j ", rgb(colour)));
            for (position, (x, y)) in chain.iter().enumerate() {
                let operator = if position == 0 { "m" } else { "l" };
                content.push_str(&format!("{:.2} {:.2} {operator} ", map_x(*x), map_y(*y)));
            }
            content.push_str("S\n");
        }
        for (from, to) in &drawing.rapids {
            content.push_str(&format!(
                "{} RG 0.5 w [2 2] 0 d {:.2} {:.2} m {:.2} {:.2} l S [] 0 d\n",
                rgb([148, 163, 184]),
                map_x(from.0),
                map_y(from.1),
                map_x(to.0),
                map_y(to.1)
            ));
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
                legend_rgb(entry),
                PDF_MARGIN,
                PDF_MARGIN + 18.0
            )),
            Style::Dashed => content.push_str(&format!(
                "{0} RG 0.6 w [3 2] 0 d {1:.2} {row_y:.2} m {2:.2} {row_y:.2} l S [] 0 d\n",
                legend_rgb(entry),
                PDF_MARGIN,
                PDF_MARGIN + 18.0
            )),
            Style::Dot => push_pdf_circle(&mut content, PDF_MARGIN + 9.0, row_y, 2.0, legend_colour(entry), false),
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

/// Projected primitives of one operation, in millimetres.
struct OpDrawing {
    cuts: Vec<Vec<(f64, f64)>>,
    rapids: Vec<((f64, f64), (f64, f64))>,
    plunges: Vec<(f64, f64)>,
    start: Option<(f64, f64)>,
}

fn project(operation: &OperationPath) -> OpDrawing {
    let mut drawing = OpDrawing { cuts: Vec::new(), rapids: Vec::new(), plunges: Vec::new(), start: None };
    let mut chain: Vec<(f64, f64)> = Vec::new();
    for movement in &operation.segments {
        match movement.kind {
            MoveKind::Rapid => {
                flush(&mut chain, &mut drawing.cuts);
                if (movement.from.x - movement.to.x).abs() > 1e-9 || (movement.from.y - movement.to.y).abs() > 1e-9 {
                    drawing.rapids.push(((movement.from.x, movement.from.y), (movement.to.x, movement.to.y)));
                }
            }
            MoveKind::Plunge => {
                flush(&mut chain, &mut drawing.cuts);
                drawing.plunges.push((movement.from.x, movement.from.y));
            }
            MoveKind::Cut => {
                if chain.is_empty() {
                    chain.push((movement.from.x, movement.from.y));
                }
                chain.push((movement.to.x, movement.to.y));
            }
            MoveKind::Arc => {
                if chain.is_empty() {
                    chain.push((movement.from.x, movement.from.y));
                }
                append_arc(&mut chain, movement);
            }
        }
        if drawing.start.is_none() && movement.kind != MoveKind::Rapid {
            drawing.start = Some((movement.from.x, movement.from.y));
        }
    }
    flush(&mut chain, &mut drawing.cuts);
    drawing
}

fn flush(chain: &mut Vec<(f64, f64)>, cuts: &mut Vec<Vec<(f64, f64)>>) {
    if chain.len() > 1 {
        cuts.push(std::mem::take(chain));
    }
    else {
        chain.clear();
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
        for point in drawing.cuts.iter().flatten() {
            include(point.0, point.1);
        }
        for point in &drawing.plunges {
            include(point.0, point.1);
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
}

struct LegendRow {
    colour: Option<usize>,
    style: Style,
    label: String,
}

fn legend(document: &CamDocument, report: &ToolpathReport) -> Vec<LegendRow> {
    let mut rows = vec![
        LegendRow { colour: None, style: Style::Solid, label: "cutting move (G1/G2/G3)".to_string() },
        LegendRow { colour: None, style: Style::Dashed, label: "rapid move (G0)".to_string() },
        LegendRow { colour: None, style: Style::Dot, label: "plunge / start point".to_string() },
    ];
    for (index, operation) in report.operations.iter().enumerate() {
        let detail = match document.tools.iter().find(|tool| tool.id == operation.tool) {
            Some(tool) => format!("tool {} d{} r{}", tool.id, number(tool.diameter), number(tool.radius())),
            None => format!("tool {}", operation.tool),
        };
        rows.push(LegendRow {
            colour: Some(index),
            style: Style::Solid,
            label: format!("#{} {} {}  {detail}", index + 1, operation.kind.as_str(), operation.operation),
        });
    }
    rows
}

fn legend_hex(entry: &LegendRow) -> &'static str {
    match entry.colour {
        Some(index) => palette(index).0,
        None if matches!(entry.style, Style::Dashed) => STOCK_COLOR,
        None => INK,
    }
}

fn legend_rgb(entry: &LegendRow) -> String {
    rgb(legend_colour(entry))
}

fn legend_colour(entry: &LegendRow) -> [u8; 3] {
    match entry.colour {
        Some(index) => palette(index).1,
        None => [31, 41, 51],
    }
}

fn palette(index: usize) -> (&'static str, [u8; 3]) {
    PALETTE[index % PALETTE.len()]
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
