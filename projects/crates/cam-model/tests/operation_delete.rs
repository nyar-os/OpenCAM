use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use cam_model::{CamDocument, Hole, OpenCam, Operation, OperationKind, Stock, Tool};
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
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.stepover = Some(2.0);
    operation
}

fn drill(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Drill, tool, 6.0, 6.0);
    operation.holes = vec![Hole::new(20.0, 20.0), Hole::new(40.0, 20.0)];
    operation
}

fn engrave(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, tool, 0.5, 0.5);
    operation.path = vec![cam_model::PathPoint::new(10.0, 10.0), cam_model::PathPoint::new(30.0, 10.0)];
    operation
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-operation-delete-{}-{name}", process::id()));
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

#[test]
fn deleting_an_operation_removes_only_it_and_keeps_document_order() {
    let directory = workspace("order");
    let (path, mut session) = session(&directory);
    let revision = session.summary().expect("summary").revision;

    let summary = session.delete_operation("floor").expect("delete");
    assert_eq!(summary.revision, revision + 1);

    let saved = document_at(&path);
    assert_eq!(operation_ids(&saved), ["face", "holes", "logo"]);
    assert!(session.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn deleting_the_last_operation_leaves_an_empty_valid_document() {
    let directory = workspace("last");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("tool");
    session.add_operation(face("only", "t6")).expect("face");
    let revision = session.summary().expect("summary").revision;

    let summary = session.delete_operation("only").expect("delete");
    assert_eq!(summary.operations, 0);
    assert_eq!(summary.revision, revision + 1);

    let saved = document_at(&path);
    assert!(saved["operations"].as_array().expect("operations").is_empty());
    assert!(session.validate().expect("validate").ok);
    assert!(document_at(&path)["revision"].as_u64().expect("revision") > 0);

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert_eq!(reopened.summary().expect("summary").operations, 0);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_unknown_id_reports_unknown_id_and_writes_nothing() {
    let directory = workspace("unknown");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    let error = session.delete_operation("missing").expect_err("unknown id");
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

    // Corrupt one operation so the candidate left after the delete is invalid.
    let mut document = document_at(&path);
    document["operations"][1]["depth"] = serde_json::json!(11.0);
    fs::write(&path, serde_json::to_string_pretty(&document).expect("serialize")).expect("write");
    let before = fs::read_to_string(&path).expect("read");

    let mut session = OpenCam::new();
    session.open(&path).expect("open");
    let revision = session.summary().expect("summary").revision;

    let error = session.delete_operation("face").expect_err("invalid candidate");
    assert_eq!(error.code, "path/outside-stock");
    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_delete_leaves_no_dangling_operation_reference() {
    let directory = workspace("references");
    let (_path, mut session) = session(&directory);

    session.delete_operation("floor").expect("delete");

    let table = session.tool_table().expect("tool table");
    assert_eq!(table.tools[0].used_by, ["face".to_string(), "holes".to_string(), "logo".to_string()]);

    let plan = session.sequence().expect("sequence");
    assert_eq!(plan.operation_count, 3);
    let ids: Vec<&str> = plan.operations.iter().map(|entry| entry.operation.as_str()).collect();
    assert_eq!(ids, ["face", "holes", "logo"]);

    let list = session.operation_list().expect("list");
    assert!(list.operations.iter().all(|entry| entry.id != "floor"));

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_successful_delete_bumps_the_revision_once_and_round_trips() {
    let directory = workspace("bump");
    let (path, mut session) = session(&directory);
    let before = session.summary().expect("summary").revision;

    let summary = session.delete_operation("holes").expect("delete");
    assert_eq!(summary.revision, before + 1);

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert_eq!(reopened.summary().expect("summary").revision, summary.revision);
    assert_eq!(operation_ids(&document_at(&path)), ["face", "floor", "logo"]);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_model_delete_returns_an_unsaved_candidate() {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool("t6", 1, 6.0));
    document.operations.push(face("face", "t6"));
    document.operations.push(pocket("floor", "t6"));

    let candidate = cam_model::operation_delete(&document, "floor").expect("candidate");
    assert_eq!(document.operations.len(), 2);
    assert_eq!(candidate.operations.len(), 1);
    assert_eq!(candidate.operations[0].id, "face");
    assert_eq!(candidate.revision, document.revision);
}

#[test]
fn deletes_are_deterministic() {
    let first = workspace("deterministic-first");
    let (first_path, mut first_session) = session(&first);
    first_session.delete_operation("floor").expect("first delete");

    let second = workspace("deterministic-second");
    let (second_path, mut second_session) = session(&second);
    second_session.delete_operation("floor").expect("second delete");

    assert_eq!(fs::read_to_string(&first_path).expect("read first"), fs::read_to_string(&second_path).expect("read second"));

    let _ = fs::remove_dir_all(&first);
    let _ = fs::remove_dir_all(&second);
}
