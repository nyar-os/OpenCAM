//! Tool-grouped operation sequencing.
//!
//! A document stores its operations in the order the machinist declared them.
//! That order is safe but it can alternate between tools, so the machine changes
//! tools far more often than necessary. [`plan`] proposes the same operations
//! grouped by tool and [`reorder`] builds the reordered document: tools keep the
//! order of their first appearance and the operations inside a tool keep their
//! document order, so the plan is stable and deterministic.

use serde::Serialize;

use crate::model::{CamDocument, Operation, OperationKind};

/// One tool group of a sequencing plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGroup {
    /// Tool the group uses.
    pub tool: String,
    /// Operation ids of the group, in document order.
    pub operations: Vec<String>,
}

/// One operation in its proposed execution position.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEntry {
    /// One-based position in the proposed program.
    pub order: usize,
    /// Operation identifier.
    pub operation: String,
    /// Operation strategy.
    pub kind: OperationKind,
    /// Tool the operation uses.
    pub tool: String,
}

/// Tool-grouped execution plan of one document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceReport {
    /// Report format identifier, always `open-cam-sequence`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Number of operations in the document.
    pub operation_count: usize,
    /// Tool changes of the current document order.
    pub before_tool_changes: usize,
    /// Tool changes of the proposed order.
    pub after_tool_changes: usize,
    /// `true` when the proposed order equals the document order.
    pub unchanged: bool,
    /// `true` when the call reordered and saved the document; a plan-only call
    /// and an already grouped document report `false`.
    pub applied: bool,
    /// Document revision the report describes; the saved revision after an
    /// applied plan and the current one otherwise.
    pub revision: u32,
    /// One group per distinct tool, in first-appearance order.
    pub groups: Vec<SequenceGroup>,
    /// Every operation in its proposed position, in execution order.
    pub operations: Vec<SequenceEntry>,
}

/// Builds the tool-grouped plan of a document without changing it.
pub fn plan(document: &CamDocument, revision: u32) -> SequenceReport {
    let order = grouped_order(&document.operations);
    let unchanged = order.iter().enumerate().all(|(position, index)| position == *index);

    let mut groups: Vec<SequenceGroup> = Vec::new();
    let mut operations = Vec::with_capacity(order.len());
    for index in &order {
        let operation = &document.operations[*index];
        match groups.last_mut() {
            Some(group) if group.tool == operation.tool => group.operations.push(operation.id.clone()),
            _ => groups.push(SequenceGroup { tool: operation.tool.clone(), operations: vec![operation.id.clone()] }),
        }
        operations.push(SequenceEntry {
            order: operations.len() + 1,
            operation: operation.id.clone(),
            kind: operation.kind,
            tool: operation.tool.clone(),
        });
    }

    let after_tool_changes =
        order.windows(2).filter(|pair| document.operations[pair[0]].tool != document.operations[pair[1]].tool).count();

    SequenceReport {
        format: "open-cam-sequence".to_string(),
        version: 1,
        units: document.units.clone(),
        operation_count: document.operations.len(),
        before_tool_changes: tool_changes(&document.operations),
        after_tool_changes,
        unchanged,
        applied: false,
        revision,
        groups,
        operations,
    }
}

/// Returns a copy of `document` whose operations follow the grouped plan.
pub fn reorder(document: &CamDocument) -> CamDocument {
    let order = grouped_order(&document.operations);
    let mut reordered = document.clone();
    reordered.operations = order.into_iter().map(|index| document.operations[index].clone()).collect();
    reordered
}

/// Indices of `operations` ordered by tool, stable inside each tool.
fn grouped_order(operations: &[Operation]) -> Vec<usize> {
    let mut tools: Vec<&str> = Vec::new();
    for operation in operations {
        if !tools.contains(&operation.tool.as_str()) {
            tools.push(operation.tool.as_str());
        }
    }
    let mut order = Vec::with_capacity(operations.len());
    for tool in tools {
        for (index, operation) in operations.iter().enumerate() {
            if operation.tool == tool {
                order.push(index);
            }
        }
    }
    order
}

/// Number of adjacent pairs that use different tools.
fn tool_changes(operations: &[Operation]) -> usize {
    operations.windows(2).filter(|pair| pair[0].tool != pair[1].tool).count()
}
