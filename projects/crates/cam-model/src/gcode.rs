//! G-code writer.
//!
//! The writer emits a single, deterministic program: no timestamps, no machine
//! probing and no vendor post-processor options. Every move comes from
//! [`crate::toolpath`], so the program and the toolpath report cannot disagree.
//! [`PostOptions`] changes only the text around those moves — a program number,
//! line numbers and coolant — and never a coordinate, a feed or a cycle.

use serde::{Deserialize, Serialize};

use crate::{
    CamError,
    model::{CamDocument, OperationKind},
    toolpath::{self, MoveKind, ToolpathReport},
};

/// Post options that change only the text of the program, never its moves.
///
/// Omitted fields keep the defaults, so an empty request renders exactly the
/// generic absolute-millimetre program. `programNumber` emits `O<n>` right
/// after the leading `%`, `lineNumbers` prefixes every line between the two
/// `%` lines with `N<n>` and `coolant` adds `M8` (`flood`) or `M7` (`mist`)
/// after the spindle start of every operation and closes it with `M9`. Units
/// and the `G90` absolute mode stay fixed; the writer never emits `G20`, `G91`
/// or a vendor dialect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostOptions {
    /// Program number emitted as `O<n>` right after the leading `%`; `None`
    /// emits no program-number line.
    pub program_number: Option<u32>,
    /// Prefixes every line between the two `%` lines with `N<n>`.
    pub line_numbers: bool,
    /// Distance between two line numbers; at least one, ten by default.
    pub line_increment: u32,
    /// Coolant mode: `off` (default), `flood` (`M8`) or `mist` (`M7`).
    pub coolant: String,
}

impl Default for PostOptions {
    fn default() -> Self {
        Self { program_number: None, line_numbers: false, line_increment: 10, coolant: "off".to_string() }
    }
}

/// Renders a complete G-code program for a document with the default post.
///
/// The document is validated and its toolpaths generated first; the first
/// validation error is returned unchanged.
pub fn render(document: &CamDocument) -> Result<String, CamError> {
    render_with_post(document, &PostOptions::default())
}

/// Renders a complete G-code program for a document and one set of post options.
///
/// The post options are checked first, so an unknown coolant or a line
/// increment of zero reports `post/invalid`; the document is validated and its
/// toolpaths generated afterwards, so the first document error is returned
/// unchanged. Without options the program is byte-identical to [`render`].
pub fn render_with_post(document: &CamDocument, post: &PostOptions) -> Result<String, CamError> {
    check_post(post)?;
    let report = toolpath::generate(document)?;
    Ok(render_with(document, &report, post))
}

fn check_post(post: &PostOptions) -> Result<(), CamError> {
    if post.line_increment == 0 {
        return Err(CamError::new("post/invalid", "line increment must be at least 1"));
    }
    if post.program_number == Some(0) {
        return Err(CamError::new("post/invalid", "program number must be greater than zero"));
    }
    if !matches!(post.coolant.as_str(), "off" | "flood" | "mist") {
        return Err(CamError::new(
            "post/invalid",
            format!("coolant `{}` is unknown; only `off`, `flood` or `mist` is supported", post.coolant),
        ));
    }
    Ok(())
}

fn render_with(document: &CamDocument, report: &ToolpathReport, post: &PostOptions) -> String {
    let safe_z = report.safe_z;
    let coolant = coolant_code(&post.coolant);
    let mut lines = Vec::new();
    lines.push("%".to_string());
    if let Some(number) = post.program_number {
        lines.push(format!("O{number}"));
    }
    lines.push(format!("(OpenCAM {})", env!("CARGO_PKG_VERSION")));
    lines.push(format!("(Units: {})", document.units));
    lines.push("(Post: generic mill, absolute, XY plane)".to_string());
    lines.push("G21 G90 G17 G94".to_string());
    lines.push(format!("G0 Z{}", number(safe_z)));

    for (index, operation) in document.operations.iter().enumerate() {
        let Some(path) = report.operations.get(index)
        else {
            continue;
        };
        let tool = document.tools.iter().find(|tool| tool.id == operation.tool);
        lines.push(format!("(operation {} - {})", operation.id, operation.kind.as_str()));
        if operation.kind == OperationKind::Contour {
            if let Some(tabs) = operation.tabs {
                lines.push(format!("(tabs {} x {} mm wide, {} mm high)", tabs.count, number(tabs.width), number(tabs.height)));
            }
        }
        if let Some(tool) = tool {
            lines.push(format!("T{} M6", tool.number));
            lines.push(format!("S{} M3", number(tool.spindle)));
            if let Some(code) = coolant {
                lines.push(code.to_string());
            }
            lines.push(format!("(tool {} diameter {})", tool.id, number(tool.diameter)));
        }
        lines.push(format!("G0 Z{}", number(safe_z)));

        if operation.kind == OperationKind::Drill {
            let depth = number(document.stock.height - operation.depth);
            let retract = number(safe_z);
            let feed = number(tool.map_or(0.0, |tool| tool.plunge_feed));
            for hole in operation.resolved_holes() {
                let x = number(hole.x);
                let y = number(hole.y);
                match (operation.peck, operation.dwell) {
                    (Some(peck), _) => {
                        lines.push(format!("G83 X{x} Y{y} Z{depth} R{retract} Q{} F{feed}", number(peck)));
                    }
                    (None, Some(dwell)) => {
                        lines.push(format!("G82 X{x} Y{y} Z{depth} R{retract} P{} F{feed}", number(dwell)));
                    }
                    (None, None) => lines.push(format!("G81 X{x} Y{y} Z{depth} R{retract} F{feed}")),
                }
            }
            lines.push("G80".to_string());
        }
        else {
            for segment in &path.segments {
                let feed = if segment.kind == MoveKind::Rapid { String::new() } else { format!(" F{}", number(segment.feed)) };
                let (command, arc) = match &segment.arc {
                    Some(arc) => {
                        let centre =
                            format!(" I{} J{}", number(arc.centre.x - segment.from.x), number(arc.centre.y - segment.from.y));
                        (if arc.clockwise { "G2" } else { "G3" }, centre)
                    }
                    None => (if segment.kind == MoveKind::Rapid { "G0" } else { "G1" }, String::new()),
                };
                lines.push(format!(
                    "{command} X{} Y{} Z{}{arc}{feed}",
                    number(segment.to.x),
                    number(segment.to.y),
                    number(segment.to.z)
                ));
            }
        }

        lines.push(format!("G0 Z{}", number(safe_z)));
        if coolant.is_some() {
            lines.push("M9".to_string());
        }
        lines.push("M5".to_string());
    }

    lines.push(format!("G0 Z{}", number(safe_z)));
    lines.push("M30".to_string());
    lines.push("%".to_string());
    if post.line_numbers {
        number_lines(&mut lines, post.line_increment);
    }
    lines.join("\n") + "\n"
}

/// Coolant command of one post option set; `None` emits no coolant line.
fn coolant_code(coolant: &str) -> Option<&'static str> {
    match coolant {
        "flood" => Some("M8"),
        "mist" => Some("M7"),
        _ => None,
    }
}

/// Prefixes every line between the two `%` markers with a sequential `N` word.
///
/// The opening and closing `%` stay unnumbered because a controller reads them
/// as the program delimiter, not as a block. Numbering starts at `increment`
/// and rises by `increment`, so the default ten yields `N10`, `N20`, ….
fn number_lines(lines: &mut [String], increment: u32) {
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.iter_mut().enumerate().take(last).skip(1) {
        let number = (index as u64) * u64::from(increment);
        let original = std::mem::take(line);
        *line = format!("N{number} {original}");
    }
}

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}
