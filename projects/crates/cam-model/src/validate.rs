//! Document validation.
//!
//! Validation is the support claim of the engine: a document that reports
//! `ok: true` is machineable by the built-in strategies. Every finding carries a
//! stable code so the CLI and the SDK can react without parsing prose.

use serde::Serialize;

use crate::{
    CamError,
    model::{CamDocument, FORMAT, Operation, OperationKind, TabSettings, Tool, UNITS, VERSION},
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
        }
        else if !tool_ids.insert(tool.id.as_str()) {
            issues.push(Issue::error("tool/duplicate-id", format!("tool `{}` is declared twice", tool.id)));
        }
        if tool.number == 0 {
            issues.push(Issue::error("tool/invalid", format!("tool `{}` needs a positive number", tool.id)));
        }
        else if !tool_numbers.insert(tool.number) {
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
    }
    else if operation.depth > document.stock.height {
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
    if let Some(ramp) = operation.ramp {
        if !positive(ramp) {
            issues.push(Issue::error(
                "operation/invalid",
                format!("operation `{}` ramp must be finite and greater than zero", operation.id),
            ));
        }
        if !matches!(operation.kind, OperationKind::Contour | OperationKind::Pocket | OperationKind::Engrave) {
            issues.push(Issue::error(
                "operation/invalid",
                format!("operation `{}` uses ramp but only `contour`, `pocket` and `engrave` support it", operation.id),
            ));
        }
    }
    match operation.strategy.as_deref() {
        None | Some("raster") => {}
        Some("spiral") => {
            if operation.kind != OperationKind::Pocket {
                issues.push(Issue::error(
                    "operation/invalid",
                    format!("operation `{}` uses strategy `spiral` but only `pocket` supports it", operation.id),
                ));
            }
        }
        Some(other) => issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` strategy `{other}` is unknown; only `raster` or `spiral` is supported", operation.id),
        )),
    }
    match operation.shape.as_deref() {
        None | Some("rect") => {}
        Some("circle") => {
            if !matches!(operation.kind, OperationKind::Pocket | OperationKind::Contour) {
                issues.push(Issue::error(
                    "operation/invalid",
                    format!("operation `{}` shape `circle` is only supported by `contour` and `pocket`", operation.id),
                ));
            }
        }
        Some(other) => issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` shape `{other}` is unknown; only `rect` or `circle` is supported", operation.id),
        )),
    }
    if operation.tabs.is_some() && operation.kind != OperationKind::Contour {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` uses tabs but only `contour` supports them", operation.id),
        ));
    }
    if operation.closed && operation.kind != OperationKind::Engrave {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` uses closed but only `engrave` supports it", operation.id),
        ));
    }
    if operation.finish.is_some() && operation.kind != OperationKind::Pocket {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` uses finish but only `pocket` supports it", operation.id),
        ));
    }
    match operation.kind {
        OperationKind::Face => check_no_islands(operation, issues),
        OperationKind::Contour => {
            check_no_islands(operation, issues);
            check_rectangle(operation, document, issues);
            if operation.shape.as_deref() == Some("circle") {
                if operation.tabs.is_some() {
                    issues.push(Issue::error(
                        "operation/invalid",
                        format!("operation `{}` uses tabs but a circular contour does not support them", operation.id),
                    ));
                }
                if positive(operation.width)
                    && positive(operation.height)
                    && (operation.width - operation.height).abs() > EPSILON
                {
                    issues.push(Issue::error(
                        "operation/circle-size",
                        format!(
                            "operation `{}` circular contour needs height equal to width, which is the diameter",
                            operation.id
                        ),
                    ));
                }
            }
            else if let Some(tabs) = operation.tabs {
                check_tabs(operation, tabs, issues);
            }
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
            let spiral = operation.strategy.as_deref() == Some("spiral");
            let circle = operation.shape.as_deref() == Some("circle");
            if circle {
                if operation.strategy.is_some() {
                    issues.push(Issue::error(
                        "operation/strategy-circle",
                        format!(
                            "operation `{}` uses shape `circle`, which supports only concentric rings and no explicit strategy",
                            operation.id
                        ),
                    ));
                }
                if !operation.islands.is_empty() {
                    issues.push(Issue::error(
                        "operation/strategy-island",
                        format!("operation `{}` uses shape `circle`, which does not support islands", operation.id),
                    ));
                }
                if positive(operation.width)
                    && positive(operation.height)
                    && (operation.width - operation.height).abs() > EPSILON
                {
                    issues.push(Issue::error(
                        "operation/circle-size",
                        format!(
                            "operation `{}` circular pocket needs height equal to width, which is the diameter",
                            operation.id
                        ),
                    ));
                }
            }
            if spiral && !operation.islands.is_empty() {
                issues.push(Issue::error(
                    "operation/strategy-island",
                    format!("operation `{}` uses strategy `spiral`, which does not support islands", operation.id),
                ));
            }
            if operation.finish == Some(true) && !operation.islands.is_empty() {
                issues.push(Issue::error(
                    "operation/invalid",
                    format!(
                        "operation `{}` enables finish but the wall finish pass does not support islands; remove them or disable finish",
                        operation.id
                    ),
                ));
            }
            check_rectangle(operation, document, issues);
            if positive(tool.diameter) && (operation.width <= tool.diameter || operation.height <= tool.diameter) {
                issues.push(Issue::error(
                    "operation/invalid",
                    format!("operation `{}` pocket is narrower than the tool diameter {}", operation.id, tool.diameter),
                ));
            }
            if !spiral && !circle {
                check_islands(operation, tool, issues);
            }
        }
        OperationKind::Engrave => {
            check_no_islands(operation, issues);
            if !operation.holes.is_empty() {
                issues.push(Issue::error(
                    "operation/invalid",
                    format!("operation `{}` uses holes but only `drill` supports them", operation.id),
                ));
            }
            validate_polyline(operation, document, issues);
        }
        OperationKind::Drill => {
            check_no_islands(operation, issues);
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

/// Checks the holding tabs of a `contour` operation.
///
/// Tabs need at least one entry, a positive width and a positive height that
/// does not exceed the cut depth. The tabs may not consume the whole profile:
/// `count × width` must stay below the rectangle perimeter
/// `2 × (width + height)`, which keeps one cut interval between every pair of
/// tabs.
fn check_tabs(operation: &Operation, tabs: TabSettings, issues: &mut Vec<Issue>) {
    if tabs.count == 0 {
        issues
            .push(Issue::error("operation/invalid", format!("operation `{}` tabs need a count of at least one", operation.id)));
    }
    if !positive(tabs.width) {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` tab width must be finite and greater than zero", operation.id),
        ));
    }
    if !positive(tabs.height) {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` tab height must be finite and greater than zero", operation.id),
        ));
    }
    else if positive(operation.depth) && tabs.height > operation.depth + EPSILON {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` tab height {} exceeds the cut depth {}", operation.id, tabs.height, operation.depth),
        ));
    }
    if !positive(operation.width) || !positive(operation.height) || !positive(tabs.width) || tabs.count == 0 {
        return;
    }
    let footprint = tabs.width * f64::from(tabs.count);
    let perimeter = 2.0 * (operation.width + operation.height);
    if footprint >= perimeter {
        issues.push(Issue::error(
            "operation/invalid",
            format!(
                "operation `{}` tab count × width {} must be less than the profile perimeter {}",
                operation.id, footprint, perimeter
            ),
        ));
    }
}

/// Checks the polyline of an `engrave` operation.
///
/// A polyline needs at least two points, every point must lie on the stock and
/// adjacent points may not coincide. A repeated adjacent point is rejected
/// rather than skipped, so a caller never loses a vertex silently. A closed
/// polyline is rejected when its last point already coincides with the first,
/// because the generator appends that closing move itself.
fn validate_polyline(operation: &Operation, document: &CamDocument, issues: &mut Vec<Issue>) {
    if operation.path.len() < 2 {
        issues.push(Issue::error("operation/invalid", format!("operation `{}` needs at least two path points", operation.id)));
        return;
    }
    for (index, point) in operation.path.iter().enumerate() {
        let inside = point.x.is_finite()
            && point.y.is_finite()
            && point.x >= -EPSILON
            && point.y >= -EPSILON
            && point.x <= document.stock.width + EPSILON
            && point.y <= document.stock.depth + EPSILON;
        if !inside {
            issues.push(Issue::error(
                "path/outside-stock",
                format!("operation `{}` path point {} at ({}, {}) is outside the stock", operation.id, index, point.x, point.y),
            ));
        }
    }
    for (index, pair) in operation.path.windows(2).enumerate() {
        let dx = pair[1].x - pair[0].x;
        let dy = pair[1].y - pair[0].y;
        if (dx * dx + dy * dy).sqrt() <= EPSILON {
            issues.push(Issue::error(
                "operation/invalid",
                format!(
                    "operation `{}` path points {} and {} coincide; remove the duplicate point",
                    operation.id,
                    index,
                    index + 1
                ),
            ));
        }
    }
    if operation.closed && operation.path.len() > 2 {
        let first = operation.path[0];
        let last = operation.path[operation.path.len() - 1];
        let dx = last.x - first.x;
        let dy = last.y - first.y;
        if (dx * dx + dy * dy).sqrt() <= EPSILON {
            issues.push(Issue::error(
                "operation/invalid",
                format!(
                    "operation `{}` is closed and its last path point coincides with the first; remove the duplicate point",
                    operation.id
                ),
            ));
        }
    }
}

fn check_no_islands(operation: &Operation, issues: &mut Vec<Issue>) {
    if !operation.islands.is_empty() {
        issues.push(Issue::error(
            "operation/invalid",
            format!("operation `{}` uses islands but only `pocket` supports them", operation.id),
        ));
    }
}

fn check_islands(operation: &Operation, tool: &Tool, issues: &mut Vec<Issue>) {
    let mut expanded = Vec::with_capacity(operation.islands.len());
    for (index, island) in operation.islands.iter().enumerate() {
        let numeric = island.x.is_finite()
            && island.y.is_finite()
            && island.width.is_finite()
            && island.height.is_finite()
            && island.width > 0.0
            && island.height > 0.0;
        if !numeric {
            issues.push(Issue::error(
                "operation/island-invalid",
                format!(
                    "operation `{}` island {} needs finite coordinates and a positive width and height",
                    operation.id, index
                ),
            ));
            continue;
        }
        if !positive(operation.width) || !positive(operation.height) || !positive(tool.diameter) {
            continue;
        }

        let inside_pocket = island.x >= operation.x - EPSILON
            && island.y >= operation.y - EPSILON
            && island.x + island.width <= operation.x + operation.width + EPSILON
            && island.y + island.height <= operation.y + operation.height + EPSILON;
        if !inside_pocket {
            issues.push(Issue::error(
                "operation/island-outside-pocket",
                format!("operation `{}` island {} leaves the pocket boundary", operation.id, index),
            ));
            continue;
        }

        let radius = tool.radius();
        let inner_x0 = operation.x + radius;
        let inner_y0 = operation.y + radius;
        let inner_x1 = operation.x + operation.width - radius;
        let inner_y1 = operation.y + operation.height - radius;
        let x0 = island.x - radius;
        let y0 = island.y - radius;
        let x1 = island.x + island.width + radius;
        let y1 = island.y + island.height + radius;
        if x0 < inner_x0 - EPSILON || y0 < inner_y0 - EPSILON || x1 > inner_x1 + EPSILON || y1 > inner_y1 + EPSILON {
            issues.push(Issue::error(
                "operation/island-too-close",
                format!(
                    "operation `{}` island {} is closer than the cutter radius {} to the pocket wall",
                    operation.id, index, radius
                ),
            ));
            continue;
        }
        expanded.push((index, x0, y0, x1, y1));
    }

    for (position, &(first_index, ax0, ay0, ax1, ay1)) in expanded.iter().enumerate() {
        for &(second_index, bx0, by0, bx1, by1) in &expanded[position + 1..] {
            let overlap_x = ax1.min(bx1) - ax0.max(bx0);
            let overlap_y = ay1.min(by1) - ay0.max(by0);
            if overlap_x > EPSILON && overlap_y > EPSILON {
                issues.push(Issue::error(
                    "operation/island-overlap",
                    format!(
                        "operation `{}` islands {} and {} are closer than the cutter diameter",
                        operation.id, first_index, second_index
                    ),
                ));
            }
        }
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

/// Tolerance used when comparing island and pocket edges, in millimetres.
const EPSILON: f64 = 1e-9;

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
