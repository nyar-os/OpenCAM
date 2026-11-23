use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process,
};

use cam_model::{CamDocument, Hole, OpenCam, Operation, OperationKind, OperationReorder, Stock, Tool};
use serde_json::Value;

fn tool(id: &str, number: u32, diameter: f64) -> Tool {
    Tool {
        id: id.to_string(),
        name: format!("{id} flat"),
        number,
        diameter,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn face(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Face, tool, 1.0, 1.0);
    operation.name = "top face".to_string();
    operation
}

fn pocket(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Pocket, tool, 3.0, 1.0);
    operation.name = "floor rough".to_string();
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.stepover = Some(2.0);
    operation
}

fn drill(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Drill, tool, 6.0, 6.0);
    operation.name = "holes".to_string();
    operation.holes = vec![Hole::new(20.0, 20.0), Hole::new(40.0, 20.0)];
    operation.peck = Some(2.0);
    operation
}

fn engrave(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, tool, 0.5, 0.5);
    operation.name = "logo".to_string();
    operation.path = vec![cam_model::PathPoint::new(10.0, 10.0), cam_model::PathPoint::new(30.0, 10.0)];
    operation
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-operation-reorder-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

/// Creates a session with a face, a pocket, a drill and an engrave on one tool.
fn session(directory: &Path) -> (PathBuf, OpenCam) {
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("t6");
    session.add_operation(face("face", "t6")).expect("face");
    session.add_operation(pocket("floor", "t6")).expect("pocket");
    session.add_operation(drill("holes", "t6")).expect("drill");
    session.add_operation(engrave("logo", "t6")).expect("engrave");
    (path, session)
}

fn document_at(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read")).expect("document JSON")
}

fn operation_ids(document: &Value) -> Vec<String> {
    document["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .map(|operation| operation["id"].as_str().expect("id").to_string())
        .collect()
}

/// Every saved operation keyed by id, so two documents can be compared without
/// depending on their order.
fn operations_by_id(document: &Value) -> BTreeMap<String, Value> {
    document["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .map(|operation| (operation["id"].as_str().expect("id").to_string(), operation.clone()))
        .collect()
}

#[test]
fn moving_an_operation_up_keeps_every_field_of_every_operation() {
    let directory = workspace("up");
    let (path, mut session) = session(&directory);
    let before = document_at(&path);
    let before_operations = operations_by_id(&before);
    let revision = session.summary().expect("summary").revision;

    let report = session.reorder_operation(&OperationReorder::new("logo", 0)).expect("reorder");

    assert_eq!(report.format, "open-cam-operation-reorder");
    assert_eq!(report.id, "logo");
    assert_eq!(report.from, 3);
    assert_eq!(report.to, 0);
    assert_eq!(report.operation_count, 4);
    assert!(report.changed);
    assert_eq!(report.revision, revision + 1);
    assert_eq!(operation_ids(&document_at(&path)), ["logo", "face", "floor", "holes"]);
    assert_eq!(operations_by_id(&document_at(&path)), before_operations);
    assert!(session.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn moving_an_operation_down_changes_only_the_order() {
    let directory = workspace("down");
    let (path, mut session) = session(&directory);
    let before = document_at(&path);
    let revision = session.summary().expect("summary").revision;

    let report = session.reorder_operation(&OperationReorder::new("face", 3)).expect("reorder");

    assert_eq!(report.from, 0);
    assert_eq!(report.to, 3);
    assert!(report.changed);
    assert_eq!(report.revision, revision + 1);
    assert_eq!(operation_ids(&document_at(&path)), ["floor", "holes", "logo", "face"]);
    assert_eq!(operations_by_id(&document_at(&path)), operations_by_id(&before));
    assert!(session.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_operation_moves_to_the_first_and_the_last_index() {
    let directory = workspace("ends");
    let (path, mut session) = session(&directory);

    let first = session.reorder_operation(&OperationReorder::new("floor", 0)).expect("first");
    assert_eq!(first.from, 1);
    assert_eq!(first.to, 0);
    assert_eq!(operation_ids(&document_at(&path)), ["floor", "face", "holes", "logo"]);

    let last = session.reorder_operation(&OperationReorder::new("floor", 3)).expect("last");
    assert_eq!(last.from, 0);
    assert_eq!(last.to, 3);
    assert_eq!(operation_ids(&document_at(&path)), ["face", "holes", "logo", "floor"]);
    assert!(session.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn moving_an_operation_to_its_current_index_is_a_no_op() {
    let directory = workspace("no-op");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    for id in ["floor", "logo"] {
        let index = operation_ids(&document_at(&path)).iter().position(|candidate| candidate == id).expect("index");
        let report = session.reorder_operation(&OperationReorder::new(id, index)).expect("no-op");

        assert!(!report.changed);
        assert_eq!(report.from, index);
        assert_eq!(report.to, index);
        assert_eq!(report.revision, revision);
        assert_eq!(fs::read_to_string(&path).expect("read"), before);
        assert_eq!(session.summary().expect("summary").revision, revision);
    }

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_index_out_of_range_reports_invalid_and_writes_nothing() {
    let directory = workspace("range");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    for to in [4, 99] {
        let error = session.reorder_operation(&OperationReorder::new("floor", to)).expect_err("out of range");
        assert_eq!(error.code, "operation/invalid");
        assert_eq!(fs::read_to_string(&path).expect("read"), before);
        assert_eq!(session.summary().expect("summary").revision, revision);
    }

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_unknown_id_reports_unknown_id_and_writes_nothing() {
    let directory = workspace("unknown");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    let error = session.reorder_operation(&OperationReorder::new("missing", 0)).expect_err("unknown id");
    assert_eq!(error.code, "operation/unknown-id");
    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_failed_candidate_leaves_the_file_byte_identical_and_keeps_the_revision() {
    let directory = workspace("failed");
    let (path, session) = session(&directory);
    drop(session);

    // Corrupt one operation so the candidate left after the reorder is invalid.
    let mut document = document_at(&path);
    document["operations"][1]["depth"] = serde_json::json!(11.0);
    fs::write(&path, serde_json::to_string_pretty(&document).expect("serialize")).expect("write");
    let before = fs::read_to_string(&path).expect("read");

    let mut session = OpenCam::new();
    session.open(&path).expect("open");
    let revision = session.summary().expect("summary").revision;

    let error = session.reorder_operation(&OperationReorder::new("face", 2)).expect_err("invalid candidate");
    assert_eq!(error.code, "path/outside-stock");
    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn derived_reports_follow_the_new_order_without_changing_ids() {
    let directory = workspace("derived");
    let (_path, mut session) = session(&directory);

    session.reorder_operation(&OperationReorder::new("holes", 0)).expect("reorder");

    let ids: Vec<String> = session.operation_list().expect("list").operations.iter().map(|entry| entry.id.clone()).collect();
    assert_eq!(ids, ["holes", "face", "floor", "logo"]);

    let table = session.tool_table().expect("tool table");
    assert_eq!(table.tools[0].used_by, ["holes", "face", "floor", "logo"]);

    let plan = session.sequence().expect("sequence");
    let planned: Vec<&str> = plan.operations.iter().map(|entry| entry.operation.as_str()).collect();
    assert_eq!(planned, ["holes", "face", "floor", "logo"]);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_model_reorder_returns_an_unsaved_candidate() {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool("t6", 1, 6.0));
    document.operations.push(face("face", "t6"));
    document.operations.push(pocket("floor", "t6"));

    let (candidate, report) = cam_model::operation_reorder(&document, &OperationReorder::new("floor", 0)).expect("candidate");

    assert_eq!(document.operations[0].id, "face");
    assert_eq!(candidate.operations[0].id, "floor");
    assert_eq!(candidate.operations[1].id, "face");
    assert_eq!(candidate.revision, document.revision);
    assert!(report.changed);
    assert_eq!(report.from, 1);
    assert_eq!(report.to, 0);
}

#[test]
fn a_successful_reorder_round_trips_through_the_file() {
    let directory = workspace("round-trip");
    let (path, mut session) = session(&directory);

    let report = session.reorder_operation(&OperationReorder::new("floor", 3)).expect("reorder");
    drop(session);

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert_eq!(reopened.summary().expect("summary").revision, report.revision);
    assert_eq!(operation_ids(&document_at(&path)), ["face", "holes", "logo", "floor"]);
    assert!(reopened.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn reorders_are_deterministic() {
    let first = workspace("deterministic-first");
    let (first_path, mut first_session) = session(&first);
    first_session.reorder_operation(&OperationReorder::new("logo", 1)).expect("first reorder");

    let second = workspace("deterministic-second");
    let (second_path, mut second_session) = session(&second);
    second_session.reorder_operation(&OperationReorder::new("logo", 1)).expect("second reorder");

    assert_eq!(fs::read_to_string(&first_path).expect("read first"), fs::read_to_string(&second_path).expect("read second"));

    let _ = fs::remove_dir_all(&first);
    let _ = fs::remove_dir_all(&second);
}
