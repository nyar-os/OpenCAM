//! G-code writer.
//!
//! The writer emits a single, deterministic program: no timestamps, no machine
//! probing and no vendor post-processor options. Every move comes from
//! [`crate::toolpath`], so the program and the toolpath report cannot disagree.

use crate::{
    CamError,
    model::{CamDocument, OperationKind},
    toolpath::{self, MoveKind, ToolpathReport},
};

/// Renders a complete G-code program for a document.
///
/// The document is validated and its toolpaths generated first; the first
/// validation error is returned unchanged.
pub fn render(document: &CamDocument) -> Result<String, CamError> {
    let report = toolpath::generate(document)?;
    Ok(render_with(document, &report))
}

fn render_with(document: &CamDocument, report: &ToolpathReport) -> String {
    let safe_z = report.safe_z;
    let mut lines = Vec::new();
    lines.push("%".to_string());
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
            lines.push(format!("(tool {} diameter {})", tool.id, number(tool.diameter)));
        }
        lines.push(format!("G0 Z{}", number(safe_z)));

        if operation.kind == OperationKind::Drill {
            let depth = number(document.stock.height - operation.depth);
            let retract = number(safe_z);
            let feed = number(tool.map_or(0.0, |tool| tool.plunge_feed));
            for hole in &operation.holes {
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
                let command = match segment.kind {
                    MoveKind::Rapid => "G0",
                    MoveKind::Plunge | MoveKind::Cut => "G1",
                };
                let feed = if segment.kind == MoveKind::Rapid { String::new() } else { format!(" F{}", number(segment.feed)) };
                lines.push(format!(
                    "{command} X{} Y{} Z{}{feed}",
                    number(segment.to.x),
                    number(segment.to.y),
                    number(segment.to.z)
                ));
            }
        }

        lines.push(format!("G0 Z{}", number(safe_z)));
        lines.push("M5".to_string());
    }

    lines.push(format!("G0 Z{}", number(safe_z)));
    lines.push("M30".to_string());
    lines.push("%".to_string());
    lines.join("\n") + "\n"
}

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}
