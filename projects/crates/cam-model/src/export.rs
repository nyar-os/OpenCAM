//! File writers.
//!
//! Every writer produces deterministic text and every write goes through
//! [`write_atomic`], so a reader never observes a half-written program.

use std::{fs, path::Path};

use crate::{CamError, model::CamDocument, toolpath::ToolpathReport};

/// Writes `contents` to `path` through a temporary file and a rename.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), CamError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|error| CamError::new("io/write", format!("cannot create `{}`: {error}", parent.display())))?;
        }
    }
    let temporary = path.with_extension(format!("{}.tmp", path.extension().and_then(|value| value.to_str()).unwrap_or("tmp")));
    fs::write(&temporary, contents)
        .map_err(|error| CamError::new("io/write", format!("cannot write `{}`: {error}", temporary.display())))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| CamError::new("io/write", format!("cannot replace `{}`: {error}", path.display())))?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| CamError::new("io/write", format!("cannot replace `{}`: {error}", path.display())))?;
    Ok(())
}

/// Renders the toolpath moves as CSV, one row per segment.
pub fn segments_csv(report: &ToolpathReport) -> String {
    let mut text = String::from("operation,kind,fromX,fromY,fromZ,toX,toY,toZ,length,feed\n");
    for operation in &report.operations {
        for segment in &operation.segments {
            text.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{}\n",
                escape(&operation.operation),
                match segment.kind {
                    crate::toolpath::MoveKind::Rapid => "rapid",
                    crate::toolpath::MoveKind::Plunge => "plunge",
                    crate::toolpath::MoveKind::Cut => "cut",
                },
                number(segment.from.x),
                number(segment.from.y),
                number(segment.from.z),
                number(segment.to.x),
                number(segment.to.y),
                number(segment.to.z),
                number(segment.length),
                number(segment.feed),
            ));
        }
    }
    text
}

/// Renders a Markdown summary of the document and its toolpaths.
pub fn markdown(document: &CamDocument, report: &ToolpathReport) -> String {
    let mut text = String::new();
    text.push_str("# OpenCAM toolpath report\n\n");
    text.push_str("## Stock\n\n");
    text.push_str("| Width | Depth | Height |\n|---:|---:|---:|\n");
    text.push_str(&format!(
        "| {} | {} | {} |\n\n",
        number(document.stock.width),
        number(document.stock.depth),
        number(document.stock.height)
    ));

    text.push_str("## Tools\n\n");
    text.push_str("| Id | Number | Diameter | Flutes | Spindle | Feed | Plunge |\n");
    text.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
    for tool in &document.tools {
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            escape(&tool.id),
            tool.number,
            number(tool.diameter),
            tool.flutes,
            number(tool.spindle),
            number(tool.feed),
            number(tool.plunge_feed)
        ));
    }
    text.push('\n');

    text.push_str("## Operations\n\n");
    text.push_str("| Id | Kind | Tool | Depth | Moves | Cut length | Estimated time (s) |\n");
    text.push_str("|---|---|---|---:|---:|---:|---:|\n");
    for operation in &report.operations {
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            escape(&operation.operation),
            operation.kind.as_str(),
            escape(&operation.tool),
            number(document.operations.iter().find(|entry| entry.id == operation.operation).map_or(0.0, |entry| entry.depth)),
            operation.moves,
            number(operation.cut_length),
            number(operation.cutting_seconds + operation.rapid_seconds)
        ));
    }

    text.push_str(&format!(
        "\nTotal cut length {} mm, rapid length {} mm, estimated cycle time {} s.\n",
        number(report.cut_length),
        number(report.rapid_length),
        number(report.estimated_seconds)
    ));
    text
}

fn escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
    else {
        value.to_string()
    }
}

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}
