use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use cam_model::{
    CamDocument, Hole, Island, LeadSettings, OpenCam, Operation, OperationDuplicate, OperationKind, Stock, TabSettings, Tool,
};
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
    operation.islands = vec![Island::new(20.0, 16.0, 8.0, 8.0)];
    operation
}

fn contour(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Contour, tool, 2.0, 1.0);
    operation.name = "boss".to_string();
    operation.x = 20.0;
    operation.y = 20.0;
    operation.width = 40.0;
    operation.height = 40.0;
    operation.shape = Some("circle".to_string());
    operation.arcs = Some("arc".to_string());
    operation.tabs = Some(TabSettings::new(2, 4.0, 0.5));
    operation.lead = Some(LeadSettings::new(2.0, "cw"));
    operation
}

fn drill(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Drill, tool, 6.0, 6.0);
    operation.name = "holes".to_string();
    operation.holes = vec![Hole::new(20.0, 20.0), Hole::new(40.0, 20.0)];
    operation.peck = Some(1.0);
    operation
}

fn engrave(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, tool, 0.5, 0.5);
    operation.name = "logo".to_string();
    operation.path = vec![cam_model::PathPoint::new(10.0, 10.0), cam_model::PathPoint::new(30.0, 10.0)];
    operation.closed = true;
    operation.ramp = Some(1.0);
    operation
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-operation-duplicate-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

/// Creates a session with one operation per kind, each carrying its own fields.
fn session(directory: &Path) -> (PathBuf, OpenCam) {
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("t6");
    session.add_operation(face("face", "t6")).expect("face");
    session.add_operation(pocket("floor", "t6")).expect("pocket");
    session.add_operation(contour("profile", "t6")).expect("contour");
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

fn operation_value(document: &Value, id: &str) -> Value {
    document["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .find(|operation| operation["id"] == id)
        .unwrap_or_else(|| panic!("operation `{id}` exists"))
        .clone()
}

fn without_id(mut operation: Value) -> Value {
    operation["id"] = Value::Null;
    operation
}

#[test]
fn a_duplicate_copies_every_field_and_inserts_the_copy_after_its_source() {
    let directory = workspace("fields");
    let (path, mut session) = session(&directory);
    let pairs = [
        ("face", "face-copy"),
        ("floor", "floor-copy"),
        ("profile", "profile-copy"),
        ("holes", "holes-copy"),
        ("logo", "logo-copy"),
    ];

    for (source, copy) in pairs {
        session.duplicate_operation(&OperationDuplicate::with_new_id(source, copy)).expect("duplicate");
    }

    let saved = document_at(&path);
    let ids = operation_ids(&saved);
    assert_eq!(ids.len(), 10);
    for (source, copy) in pairs {
        let index = ids.iter().position(|id| id == source).expect("source");
        assert_eq!(ids[index + 1], copy, "`{copy}` must follow `{source}`");
        assert_eq!(without_id(operation_value(&saved, source)), without_id(operation_value(&saved, copy)));
        assert_eq!(operation_value(&saved, copy)["name"], operation_value(&saved, source)["name"]);
    }

    assert_eq!(operation_value(&saved, "floor-copy")["islands"].as_array().expect("islands").len(), 1);
    assert_eq!(operation_value(&saved, "profile-copy")["kind"], "contour");
    assert_eq!(operation_value(&saved, "holes-copy")["peck"], 1.0);
    assert_eq!(operation_value(&saved, "logo-copy")["closed"], true);
    assert!(session.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_missing_id_takes_the_smallest_free_op_number() {
    let directory = workspace("generated");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("tool");
    session.add_operation(face("face", "t6")).expect("face");
    session.add_operation(face("op-1", "t6")).expect("op-1");
    session.add_operation(face("op-3", "t6")).expect("op-3");

    session.duplicate_operation(&OperationDuplicate::new("face")).expect("first");
    session.duplicate_operation(&OperationDuplicate::new("face")).expect("second");

    let ids = operation_ids(&document_at(&path));
    assert_eq!(ids, ["face", "op-4", "op-2", "op-1", "op-3"]);
    assert!(session.validate().expect("validate").ok);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_supplied_id_is_used_and_an_empty_or_used_id_is_rejected() {
    let directory = workspace("supplied");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    session.duplicate_operation(&OperationDuplicate::with_new_id("floor", "floor-copy")).expect("duplicate");
    let ids = operation_ids(&document_at(&path));
    let index = ids.iter().position(|id| id == "floor").expect("floor");
    assert_eq!(ids[index + 1], "floor-copy");
    let revision = revision + 1;

    for new_id in ["", "   "] {
        let error = session.duplicate_operation(&OperationDuplicate::with_new_id("floor", new_id)).expect_err("empty id");
        assert_eq!(error.code, "operation/invalid");
    }
    let clash = session.duplicate_operation(&OperationDuplicate::with_new_id("floor", "face")).expect_err("used id");
    assert_eq!(clash.code, "operation/duplicate-id");
    assert_eq!(session.summary().expect("summary").revision, revision);
    assert_eq!(operation_ids(&document_at(&path)), ids);
    assert_ne!(fs::read_to_string(&path).expect("read"), before);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_unknown_source_reports_unknown_id_and_writes_nothing() {
    let directory = workspace("unknown");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    let error = session.duplicate_operation(&OperationDuplicate::new("missing")).expect_err("unknown source");
    assert_eq!(error.code, "operation/unknown-id");
    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_failed_duplicate_leaves_the_file_byte_identical_and_keeps_the_revision() {
    let directory = workspace("failed");
    let (path, session) = session(&directory);
    drop(session);

    // Corrupt one operation so the duplicated candidate fails validation.
    let mut document = document_at(&path);
    document["operations"][0]["depth"] = serde_json::json!(11.0);
    fs::write(&path, serde_json::to_string_pretty(&document).expect("serialize")).expect("write");
    let before = fs::read_to_string(&path).expect("read");

    let mut session = OpenCam::new();
    session.open(&path).expect("open");
    let revision = session.summary().expect("summary").revision;

    let error = session.duplicate_operation(&OperationDuplicate::new("floor")).expect_err("invalid candidate");
    assert_eq!(error.code, "path/outside-stock");
    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_successful_duplicate_bumps_the_revision_once_and_round_trips() {
    let directory = workspace("bump");
    let (path, mut session) = session(&directory);
    let before = session.summary().expect("summary").revision;

    let summary = session.duplicate_operation(&OperationDuplicate::new("floor")).expect("duplicate");
    assert_eq!(summary.operations, 6);
    assert_eq!(summary.revision, before + 1);

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert_eq!(reopened.summary().expect("summary").revision, summary.revision);
    let list = reopened.operation_list().expect("list");
    let original = list.operations.iter().find(|entry| entry.id == "floor").expect("floor");
    let copy = list.operations.iter().find(|entry| entry.id == "op-1").expect("op-1");
    assert_eq!(copy.kind, original.kind);
    assert_eq!(copy.name, original.name);
    assert_eq!(copy.tool, original.tool);
    assert_eq!(copy.depth, original.depth);
    assert_eq!(copy.stepdown, original.stepdown);
    assert_eq!(copy.stepover, original.stepover);
    assert_eq!(copy.island_count, original.island_count);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_model_duplicate_returns_an_unsaved_candidate() {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool("t6", 1, 6.0));
    document.operations.push(pocket("floor", "t6"));

    let candidate = cam_model::operation_duplicate(&document, &OperationDuplicate::new("floor")).expect("candidate");
    assert_eq!(document.operations.len(), 1);
    assert_eq!(candidate.operations.len(), 2);
    assert_eq!(candidate.operations[0].id, "floor");
    assert_eq!(candidate.operations[1].id, "op-1");
    assert_eq!(candidate.operations[1].width, document.operations[0].width);
    assert_eq!(candidate.operations[1].islands, document.operations[0].islands);
    assert_eq!(candidate.revision, document.revision);
}

#[test]
fn duplicates_are_deterministic() {
    let first = workspace("deterministic-first");
    let (first_path, mut first_session) = session(&first);
    first_session.duplicate_operation(&OperationDuplicate::new("floor")).expect("first duplicate");

    let second = workspace("deterministic-second");
    let (second_path, mut second_session) = session(&second);
    second_session.duplicate_operation(&OperationDuplicate::new("floor")).expect("second duplicate");

    assert_eq!(fs::read_to_string(&first_path).expect("read first"), fs::read_to_string(&second_path).expect("read second"));

    let _ = fs::remove_dir_all(&first);
    let _ = fs::remove_dir_all(&second);
}
