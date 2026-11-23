use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use cam_model::{CamDocument, Hole, OpenCam, Operation, OperationKind, OperationUpdate, Stock, Tool};

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
    Operation::new(id, OperationKind::Face, tool, 1.0, 1.0)
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

fn drill(id: &str, tool: &str, peck: Option<f64>) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Drill, tool, 6.0, 6.0);
    operation.holes = vec![Hole::new(20.0, 20.0), Hole::new(40.0, 20.0)];
    operation.peck = peck;
    operation
}

fn engrave(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, tool, 0.5, 0.5);
    operation.path = vec![cam_model::PathPoint::new(10.0, 10.0), cam_model::PathPoint::new(30.0, 10.0)];
    operation
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-operation-update-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

/// Creates a session with two tools, a face, a pocket, a drill with peck and an engrave.
fn session(directory: &Path) -> (PathBuf, OpenCam) {
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("t6");
    session.add_tool(tool("t3", 2, 3.0)).expect("t3");
    session.add_operation(face("face", "t6")).expect("face");
    session.add_operation(pocket("floor", "t6")).expect("pocket");
    session.add_operation(drill("holes", "t6", Some(2.0))).expect("drill");
    session.add_operation(engrave("logo", "t6")).expect("engrave");
    (path, session)
}

fn document_at(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read")).expect("document JSON")
}

fn operation<'a>(document: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    document["operations"].as_array().expect("operations").iter().find(|entry| entry["id"] == id).expect("operation exists")
}

#[test]
fn an_update_changes_only_the_supplied_fields() {
    let directory = workspace("partial");
    let (path, mut session) = session(&directory);

    session
        .update_operation(&OperationUpdate {
            name: Some("floor rough".to_string()),
            depth: Some(4.0),
            stepdown: Some(2.0),
            stepover: Some(1.5),
            ..OperationUpdate::new("floor")
        })
        .expect("update");

    let saved = document_at(&path);
    let floor = operation(&saved, "floor");
    assert_eq!(floor["name"], "floor rough");
    assert_eq!(floor["depth"], 4.0);
    assert_eq!(floor["stepdown"], 2.0);
    assert_eq!(floor["stepover"], 1.5);
    assert_eq!(floor["tool"], "t6");
    assert_eq!(floor["kind"], "pocket");
    assert_eq!(floor["x"], 10.0);
    assert_eq!(floor["y"], 10.0);
    assert_eq!(floor["width"], 40.0);
    assert_eq!(floor["height"], 30.0);
    assert_eq!(operation(&saved, "face")["depth"], 1.0);
    assert_eq!(operation(&saved, "holes")["peck"], 2.0);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn changing_the_tool_changes_the_tool_usage() {
    let directory = workspace("tool");
    let (_path, mut session) = session(&directory);

    session
        .update_operation(&OperationUpdate { tool: Some("t3".to_string()), ..OperationUpdate::new("face") })
        .expect("update");

    let table = session.tool_table().expect("tool table");
    assert_eq!(table.tools[0].tool.id, "t6");
    assert_eq!(table.tools[0].used_by, ["floor".to_string(), "holes".to_string(), "logo".to_string()]);
    assert_eq!(table.tools[1].tool.id, "t3");
    assert_eq!(table.tools[1].used_by, ["face".to_string()]);

    let report = session.operation_list().expect("list");
    assert_eq!(report.operations[0].tool, "t3");
    assert_eq!(report.operations[1].tool, "t6");
    assert_eq!(report.used_tools, 2);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn peck_and_dwell_are_drill_only_and_mutually_exclusive() {
    let directory = workspace("drill");
    let (_path, mut session) = session(&directory);

    session.update_operation(&OperationUpdate { peck: Some(1.5), ..OperationUpdate::new("holes") }).expect("peck");
    assert_eq!(session.operation_list().expect("list").operations[2].kind, OperationKind::Drill);

    let combined = session
        .update_operation(&OperationUpdate { dwell: Some(0.25), ..OperationUpdate::new("holes") })
        .expect_err("dwell with peck");
    assert_eq!(combined.code, "operation/invalid");

    let both = session
        .update_operation(&OperationUpdate { peck: Some(1.0), dwell: Some(0.5), ..OperationUpdate::new("face") })
        .expect_err("peck and dwell on face");
    assert_eq!(both.code, "operation/invalid");

    let peck_on_pocket = session
        .update_operation(&OperationUpdate { peck: Some(1.0), ..OperationUpdate::new("floor") })
        .expect_err("peck on pocket");
    assert_eq!(peck_on_pocket.code, "operation/invalid");

    let dwell_on_engrave = session
        .update_operation(&OperationUpdate { dwell: Some(0.5), ..OperationUpdate::new("logo") })
        .expect_err("dwell on engrave");
    assert_eq!(dwell_on_engrave.code, "operation/invalid");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn dwell_is_accepted_by_a_drill_without_peck() {
    let directory = workspace("dwell");
    let mut session = OpenCam::new();
    let path = directory.join("part.ocam");
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("tool");
    session.add_operation(drill("holes", "t6", None)).expect("drill");

    session.update_operation(&OperationUpdate { dwell: Some(0.5), ..OperationUpdate::new("holes") }).expect("dwell");
    let saved = document_at(&path);
    assert_eq!(operation(&saved, "holes")["dwell"], 0.5);
    assert!(operation(&saved, "holes")["peck"].is_null());

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn invalid_values_are_rejected() {
    let directory = workspace("values");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");

    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let depth = session
            .update_operation(&OperationUpdate { depth: Some(value), ..OperationUpdate::new("floor") })
            .expect_err("invalid depth");
        assert_eq!(depth.code, "operation/invalid");
        let stepdown = session
            .update_operation(&OperationUpdate { stepdown: Some(value), ..OperationUpdate::new("floor") })
            .expect_err("invalid stepdown");
        assert_eq!(stepdown.code, "operation/invalid");
        let stepover = session
            .update_operation(&OperationUpdate { stepover: Some(value), ..OperationUpdate::new("floor") })
            .expect_err("invalid stepover");
        assert_eq!(stepover.code, "operation/invalid");
        let peck = session
            .update_operation(&OperationUpdate { peck: Some(value), ..OperationUpdate::new("holes") })
            .expect_err("invalid peck");
        assert_eq!(peck.code, "operation/invalid");
    }

    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_empty_update_unknown_id_and_unknown_tool_report_stable_codes() {
    let directory = workspace("codes");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    assert_eq!(session.update_operation(&OperationUpdate::new("floor")).expect_err("no fields").code, "operation/invalid");
    assert_eq!(
        session
            .update_operation(&OperationUpdate { depth: Some(2.0), ..OperationUpdate::new("missing") })
            .expect_err("unknown id")
            .code,
        "operation/unknown-id"
    );
    assert_eq!(
        session
            .update_operation(&OperationUpdate { tool: Some("t9".to_string()), ..OperationUpdate::new("floor") })
            .expect_err("unknown tool")
            .code,
        "operation/unknown-tool"
    );
    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_failed_update_leaves_the_document_and_revision_untouched() {
    let directory = workspace("failed");
    let (path, mut session) = session(&directory);
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    // A depth beyond the stock height fails the whole-document validation.
    let deep = session
        .update_operation(&OperationUpdate { depth: Some(11.0), ..OperationUpdate::new("floor") })
        .expect_err("depth beyond the stock");
    assert_eq!(deep.code, "path/outside-stock");
    // A stepover larger than the tool diameter is rejected by the shared validator.
    let wide = session
        .update_operation(&OperationUpdate { stepover: Some(9.0), ..OperationUpdate::new("floor") })
        .expect_err("stepover beyond the tool");
    assert_eq!(wide.code, "operation/invalid");

    assert_eq!(fs::read_to_string(&path).expect("read"), before);
    assert_eq!(session.summary().expect("summary").revision, revision);
    assert_eq!(document_at(&path)["revision"], revision);

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_successful_update_bumps_the_revision_once_and_round_trips() {
    let directory = workspace("bump");
    let (path, mut session) = session(&directory);
    let before = session.summary().expect("summary").revision;

    let summary = session
        .update_operation(&OperationUpdate {
            depth: Some(2.5),
            stepdown: Some(1.25),
            name: Some("floor finish".to_string()),
            ..OperationUpdate::new("floor")
        })
        .expect("update");
    assert_eq!(summary.revision, before + 1);

    let mut reopened = OpenCam::new();
    reopened.open(&path).expect("reopen");
    assert_eq!(reopened.summary().expect("summary").revision, summary.revision);
    let report = reopened.operation_list().expect("list");
    let floor = report.operations.iter().find(|entry| entry.id == "floor").expect("floor");
    assert_eq!(floor.depth, 2.5);
    assert_eq!(floor.stepdown, 1.25);
    assert_eq!(floor.name, "floor finish");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_update_is_deterministic() {
    let request = OperationUpdate {
        depth: Some(2.0),
        stepdown: Some(1.0),
        name: Some("floor".to_string()),
        ..OperationUpdate::new("floor")
    };

    let first = workspace("deterministic-first");
    let (first_path, mut first_session) = session(&first);
    first_session.update_operation(&request).expect("first update");

    let second = workspace("deterministic-second");
    let (second_path, mut second_session) = session(&second);
    second_session.update_operation(&request).expect("second update");

    assert_eq!(fs::read_to_string(&first_path).expect("read first"), fs::read_to_string(&second_path).expect("read second"));

    let _ = fs::remove_dir_all(&first);
    let _ = fs::remove_dir_all(&second);
}

#[test]
fn the_candidate_document_keeps_every_operation_and_field_it_did_not_edit() {
    let directory = workspace("candidate");
    let (_path, mut session) = session(&directory);
    let before = session.operation_list().expect("list");

    session.update_operation(&OperationUpdate { depth: Some(4.0), ..OperationUpdate::new("floor") }).expect("update");
    let after = session.operation_list().expect("list");

    assert_eq!(before.operation_count, after.operation_count);
    assert_eq!(before.operations.len(), after.operations.len());
    for (old, new) in before.operations.iter().zip(&after.operations) {
        assert_eq!(old.id, new.id);
        if old.id == "floor" {
            assert_eq!(new.depth, 4.0);
        }
        else {
            assert_eq!(old, new);
        }
    }

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_model_update_returns_an_unsaved_candidate() {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool("t6", 1, 6.0));
    document.operations.push(pocket("floor", "t6"));

    let candidate =
        cam_model::operation_update(&document, &OperationUpdate { depth: Some(2.0), ..OperationUpdate::new("floor") })
            .expect("candidate");
    assert_eq!(document.operations[0].depth, 3.0);
    assert_eq!(candidate.operations[0].depth, 2.0);
    assert_eq!(candidate.revision, document.revision);
}
