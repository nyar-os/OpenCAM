use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, Hole, OpenCam, Operation, OperationKind, SequenceReport, Stock, Tool, sequence_plan, sequence_reorder,
};

fn tool(id: &str, number: u32) -> Tool {
    Tool {
        id: id.to_string(),
        name: format!("{id} flat"),
        number,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn operation(id: &str, tool: &str, kind: OperationKind) -> Operation {
    let mut operation = Operation::new(id, kind, tool, 1.0, 1.0);
    if kind == OperationKind::Drill {
        operation.holes = vec![Hole::new(5.0, 5.0)];
    }
    if kind == OperationKind::Pocket || kind == OperationKind::Contour {
        operation.x = 10.0;
        operation.y = 10.0;
        operation.width = 20.0;
        operation.height = 20.0;
    }
    operation
}

/// Alternating tools: `t6`, `t3`, `t6`, `t3`, `t6` -> four tool changes.
fn document() -> CamDocument {
    let mut document = CamDocument::new(Stock::new(60.0, 40.0, 8.0));
    document.tools.push(tool("t6", 1));
    document.tools.push(tool("t3", 2));
    document.operations.push(operation("face", "t6", OperationKind::Face));
    document.operations.push(operation("holes", "t3", OperationKind::Drill));
    document.operations.push(operation("floor", "t6", OperationKind::Pocket));
    document.operations.push(operation("profile", "t3", OperationKind::Contour));
    document.operations.push(operation("logo", "t6", OperationKind::Engrave));
    document
}

#[test]
fn the_plan_groups_operations_by_tool_and_counts_changes() {
    let report = sequence_plan(&document(), 0);
    assert_eq!(report.format, "open-cam-sequence");
    assert_eq!(report.operation_count, 5);
    assert_eq!(report.before_tool_changes, 4);
    assert_eq!(report.after_tool_changes, 1);
    assert!(!report.unchanged);
    assert!(!report.applied);
    assert_eq!(report.revision, 0);

    let ids: Vec<&str> = report.operations.iter().map(|entry| entry.operation.as_str()).collect();
    assert_eq!(ids, ["face", "floor", "logo", "holes", "profile"]);
    assert_eq!(report.operations[0].order, 1);
    assert_eq!(report.operations[4].order, 5);
    assert_eq!(report.operations[3].kind, OperationKind::Drill);
    assert_eq!(report.operations[3].tool, "t3");

    assert_eq!(report.groups.len(), 2);
    assert_eq!(report.groups[0].tool, "t6");
    assert_eq!(report.groups[0].operations, ["face", "floor", "logo"]);
    assert_eq!(report.groups[1].tool, "t3");
    assert_eq!(report.groups[1].operations, ["holes", "profile"]);
}

#[test]
fn the_plan_is_stable_and_keeps_the_order_inside_a_tool() {
    let reordered = sequence_reorder(&document());
    let again = sequence_plan(&reordered, 0);
    assert!(again.unchanged);
    assert_eq!(again.before_tool_changes, 1);
    assert_eq!(again.after_tool_changes, 1);
    let ids: Vec<&str> = again.operations.iter().map(|entry| entry.operation.as_str()).collect();
    assert_eq!(ids, ["face", "floor", "logo", "holes", "profile"]);
}

#[test]
fn a_single_tool_document_is_unchanged() {
    let mut document = CamDocument::new(Stock::new(60.0, 40.0, 8.0));
    document.tools.push(tool("t6", 1));
    document.operations.push(operation("one", "t6", OperationKind::Face));
    document.operations.push(operation("two", "t6", OperationKind::Pocket));
    let report = sequence_plan(&document, 7);
    assert!(report.unchanged);
    assert_eq!(report.revision, 7);
    assert_eq!(report.before_tool_changes, 0);
    assert_eq!(report.after_tool_changes, 0);
}

#[test]
fn an_empty_document_has_no_groups() {
    let report = sequence_plan(&CamDocument::new(Stock::new(60.0, 40.0, 8.0)), 0);
    assert!(report.unchanged);
    assert_eq!(report.operation_count, 0);
    assert_eq!(report.before_tool_changes, 0);
    assert_eq!(report.after_tool_changes, 0);
    assert!(report.groups.is_empty());
    assert!(report.operations.is_empty());
}

#[test]
fn reordering_returns_a_copy_and_leaves_the_source_untouched() {
    let document = document();
    let reordered = sequence_reorder(&document);
    let source: Vec<&str> = document.operations.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(source, ["face", "holes", "floor", "profile", "logo"]);
    let target: Vec<&str> = reordered.operations.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(target, ["face", "floor", "logo", "holes", "profile"]);
}

#[test]
fn the_plan_is_deterministic() {
    let first = sequence_plan(&document(), 3);
    let second = sequence_plan(&document(), 3);
    assert_eq!(first, second);
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-sequence-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

#[test]
fn applying_rewrites_the_document_once_and_reports_the_revision() {
    let directory = workspace("apply");
    let path = directory.join("bracket.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(60.0, 40.0, 8.0)).expect("stock");
    session.add_tool(tool("t6", 1)).expect("t6");
    session.add_tool(tool("t3", 2)).expect("t3");
    session.add_operation(operation("face", "t6", OperationKind::Face)).expect("face");
    session.add_operation(operation("holes", "t3", OperationKind::Drill)).expect("holes");
    session.add_operation(operation("floor", "t6", OperationKind::Pocket)).expect("floor");

    let plan = session.sequence().expect("sequence");
    assert!(!plan.unchanged);
    assert!(!plan.applied);
    assert_eq!(plan.before_tool_changes, 2);
    assert_eq!(plan.after_tool_changes, 1);
    let before_apply = session.summary().expect("summary").revision;

    let applied = session.apply_sequence().expect("apply");
    assert!(applied.applied);
    assert_eq!(applied.revision, before_apply + 1);
    assert_eq!(applied.before_tool_changes, 2);
    assert_eq!(applied.after_tool_changes, 1);

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    let report = reopened.sequence().expect("sequence");
    assert!(report.unchanged);
    let ids: Vec<&str> = report.operations.iter().map(|entry| entry.operation.as_str()).collect();
    assert_eq!(ids, ["face", "floor", "holes"]);

    let summary = session.summary().expect("summary");
    let again = session.apply_sequence().expect("apply again");
    assert!(!again.applied);
    assert_eq!(again.revision, summary.revision);
    assert_eq!(session.summary().expect("summary").revision, summary.revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn reorder_places_all_operations_of_one_tool_together() {
    let report: SequenceReport = sequence_plan(&document(), 0);
    let reordered = sequence_reorder(&document());
    assert_eq!(reordered.operations.len(), report.operation_count);
    let touched: Vec<&str> = reordered.operations.iter().map(|entry| entry.tool.as_str()).collect();
    assert_eq!(touched, ["t6", "t6", "t6", "t3", "t3"]);
}
