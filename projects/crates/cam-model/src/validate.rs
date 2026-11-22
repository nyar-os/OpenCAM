//! Document validation.
//!
//! Validation is the support claim of the engine: a document that reports
//! `ok: true` is machineable by the built-in strategies. Every finding carries a
//! stable code so the CLI and the SDK can react without parsing prose.

use serde::Serialize;

use crate::{
    CamError,
    model::{CamDocument, FORMAT, Operation, OperationKind, Tool, UNITS, VERSION},
};

/// Severity of one finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The document cannot be machined.
    Error,
    /// The document can be machined but the value is suspicious.
    Warning,
    /// Informational finding.
    Notice,
}

/// One validation finding.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Stable finding code.
    pub code: String,
    /// Severity of the finding.
    pub severity: Severity,
    /// Human readable explanation.
    pub message: String,
}

impl Issue {
    fn error(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), severity: Severity::Error, message: message.into() }
    }

    fn warning(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), severity: Severity::Warning, message: message.into() }
    }
}

/// Result of validating a document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    /// `true` when no finding has [`Severity::Error`].
    pub ok: bool,
    /// Every finding in the order the validator produced them.
    pub issues: Vec<Issue>,
}

/// Validates a document and returns every finding.
pub fn validate(document: &CamDocument) -> ValidationReport {
    let mut issues = Vec::new();

    if document.format != FORMAT {
        issues.push(Issue::error("document/format", format!("format is `{}` but `{FORMAT}` is required", document.format)));
    }
    if document.version != VERSION {
        issues.push(Issue::error("document/version", format!("version is {} but {VERSION} is required", document.version)));
    }
    if document.units != UNITS {
        issues.push(Issue::error("document/units", format!("units are `{}` but only `{UNITS}` is supported", document.units)));
    }
    if !positive(document.stock.width) || !positive(document.stock.depth) || !positive(document.stock.height) {
        issues.push(Issue::error("stock/invalid", "stock width, depth and height must be finite and greater than zero"));
    }

    if document.tools.is_empty() {
        issues.push(Issue::warning("tool/none", "the document has no tools; operations cannot be added"));
    }
    let mut tool_ids = std::collections::HashSet::new();
    let mut tool_numbers = std::collections::HashSet::new();
    for tool in &document.tools {
        if tool.id.trim().is_empty() {
            issues.push(Issue::error("tool/invalid", "a tool id must be a non-empty string"));
        } else if !tool_ids.insert(tool.id.as_str()) {
            issues.push(Issue::error("tool/duplicate-id", format!("tool `{}` is declared twice", tool.id)));
        }
        if tool.number == 0 {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive number", tool.id)));
        } else if !tool_numbers.insert(tool.number) {
            issues.push(Issue::warning(
                "tool/duplicate-number",
                format!("tool number {} is used by more than one tool", tool.number),
            ));
        }
        if !positive(tool.diameter) {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive diameter", tool.id)));
        }
        if tool.flutes == 0 {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs at least one flute", tool.id)));
        }
        if !positive(tool.spindle) {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive spindle speed", tool.id)));
        }
        if !positive(tool.feed) {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive feed", tool.id)));
        }
        if !positive(tool.plunge_feed) {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive plunge feed", tool.id)));
        }
    }

    if document.operations.is_empty() {
        issues.push(Issue::warning("operation/none", "the document has no machining operations"));
    }
    let mut operation_ids = std::collections::HashSet::new();
    for operation in &document.operations {
        validate_operation(operation, document, &mut issues);
        if !operation_ids.insert(operation.id.as_str()) {
            issues.push(Issue::error("operation/duplicate-id", format!("operation `{}` is declared twice", operation.id)));
        }
    }

    let ok = !issues.iter().any(|issue| issue.severity == Severity::Error);
    ValidationReport { ok, issues }
}

/// Returns the first error of a report as a [`CamError`], if any.
pub fn first_error(report: &ValidationReport) -> Option<CamError> {
    report
        .issues
        .iter()
        .find(|issue| issue.severity == Severity::Error)
        .map(|issue| CamError::new(issue.code.clone(), issue.message.clone()))
}

/// Checks a single tool and returns the first error, if any.
pub fn check_tool(tool: &Tool) -> Result<(), CamError> {
    let mut issues = Vec::new();
    check_tool_entry(tool, &mut issues);
    issues
        .into_iter()
        .find(|issue| issue.severity == Severity::Error)
        .map(|issue| CamError::new(issue.code, issue.message))
        .map_or(Ok(()), Err)
}

/// Checks a single operation against a document and returns the first error.
pub fn check_operation(operation: &Operation, document: &CamDocument) -> Result<(), CamError> {
    let mut issues = Vec::new();
    validate_operation(operation, document, &mut issues);
    issues
        .into_iter()
        .find(|issue| issue.severity == Severity::Error)
        .map(|issue| CamError::new(issue.code, issue.message))
        .map_or(Ok(()), Err)
}

fn validate_operation(operation: &Operation, document: &CamDocument, issues: &mut Vec<Issue>) {
    if operation.id.trim().is_empty() {
        issues.push(Issue::error("operation/invalid", "an operation id must be a non-empty string"));
    }
    if !document.tools.iter().any(|tool| tool.id == operation.tool) {
        issues.push(Issue::error(
            "operation/unknown-tool",
            format!("operation `{}` references unknown tool `{}`", operation.id, operation.tool),
        ));
        return;
    }
    let tool = document.tools.iter().find(|tool| tool.id == operation.tool).expect("checked above");
    if !positive(operation.depth) {
        issues.push(Issue::error("operation/invalid", format!("operation `{}` needs a positive depth", operation.id)));
    } else if operation.depth > document.stock.height {
        issues.push(Issue::error(
            "path/outside-stock",
            format!(
                "operation `{}` depth {} exceeds the stock height {}",
                operation.id, operation.depth, document.stock.height
            ),
        ));
    }
    if !positive(operation.stepdown) {
        issues.push(Issue::error("operation/invalid", format!("operation `{}` needs a positive stepdown", operation.id)));
    }
    if let Some(stepover) = operation.stepover {
        if !positive(stepover) || stepover > tool.diameter {
            issues.push(Issue::error(
                "operation/invalid",
                format!(
                    "operation `{}` stepover must be greater than zero and at most the tool diameter {}",
                    operation.id, tool.diameter
                ),
            ));
        }
    }
    match operation.kind {
        OperationKind::Face => {}
        OperationKind::Contour => {
            check_rectangle(operation, document, issues);
            if !positive(tool.diameter) {
                return;
            }
            let radius = tool.radius();
            if operation.x - radius < 0.0
                || operation.y - radius < 0.0
                || operation.x + operation.width + radius > document.stock.width
                || operation.y + operation.height + radius > document.stock.depth
            {
                issues.push(Issue::warning(
                    "path/outside-stock",
                    format!(
                        "operation `{}` profile leaves the stock because the cutter radius is offset outward",
                        operation.id
                    ),
                ));
            }
        }
        OperationKind::Pocket => {
            check_rectangle(operation, document, issues);
            if positive(tool.diameter) && (operation.width <= tool.diameter || operation.height <= tool.diameter) {
                issues.push(Issue::error(
                    "operation/invalid",
                    format!("operation `{}` pocket is narrower than the tool diameter {}", operation.id, tool.diameter),
                ));
            }
        }
        OperationKind::Drill => {
            if operation.holes.is_empty() {
                issues.push(Issue::error("operation/invalid", format!("operation `{}` has no holes", operation.id)));
            }
            if let Some(peck) = operation.peck {
                if !positive(peck) {
                    issues.push(Issue::error(
                        "operation/invalid",
                        format!("operation `{}` peck must be greater than zero", operation.id),
                    ));
                }
            }
            for (index, hole) in operation.holes.iter().enumerate() {
                let inside = hole.x.is_finite()
                    && hole.y.is_finite()
                    && hole.x >= 0.0
                    && hole.y >= 0.0
                    && hole.x <= document.stock.width
                    && hole.y <= document.stock.depth;
                if !inside {
                    issues.push(Issue::error(
                        "path/outside-stock",
                        format!("operation `{}` hole {} at ({}, {}) is outside the stock", operation.id, index, hole.x, hole.y),
                    ));
                }
            }
        }
    }
}

fn check_rectangle(operation: &Operation, document: &CamDocument, issues: &mut Vec<Issue>) {
    if !positive(operation.width) || !positive(operation.height) {
        issues
            .push(Issue::error("operation/invalid", format!("operation `{}` needs a positive width and height", operation.id)));
        return;
    }
    let inside = operation.x.is_finite()
        && operation.y.is_finite()
        && operation.x >= 0.0
        && operation.y >= 0.0
        && operation.x + operation.width <= document.stock.width
        && operation.y + operation.height <= document.stock.depth;
    if !inside {
        issues.push(Issue::error(
            "path/outside-stock",
            format!("operation `{}` rectangle leaves the stock boundary", operation.id),
        ));
    }
}

fn check_tool_entry(tool: &Tool, issues: &mut Vec<Issue>) {
    if tool.id.trim().is_empty() {
        issues.push(Issue::error("tool/invalid", "a tool id must be a non-empty string"));
    }
    if tool.number == 0 {
        issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive number", tool.id)));
    }
    if !positive(tool.diameter) {
        issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive diameter", tool.id)));
    }
    if tool.flutes == 0 {
        issues.push(Issue::error("tool/invalid", format!("tool `{}` needs at least one flute", tool.id)));
    }
    if !positive(tool.spindle) {
        issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive spindle speed", tool.id)));
    }
    if !positive(tool.feed) {
        issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive feed", tool.id)));
    }
    if !positive(tool.plunge_feed) {
        issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive plunge feed", tool.id)));
    }
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
