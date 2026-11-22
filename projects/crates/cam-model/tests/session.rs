use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use cam_model::{ExportFormat, Hole, OpenCam, Operation, OperationKind, Stock, Tool};

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "6 mm flat".to_string(),
        number: 1,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

#[test]
fn create_edit_reopen_and_export() {
    let directory = workspace("session");
    let path = directory.join("bracket.ocam");

    let mut session = OpenCam::new();
    let created = session.create(&path).expect("create");
    assert_eq!(created.revision, 0);
    assert_eq!(created.tools, 0);

    session.set_stock(Stock::new(120.0, 80.0, 12.0)).expect("stock");
    session.add_tool(tool()).expect("tool");

    let mut drill = Operation::new("holes", OperationKind::Drill, "t6", 6.0, 3.0);
    drill.holes = vec![Hole::new(20.0, 20.0), Hole::new(80.0, 40.0)];
    let summary = session.add_operation(drill).expect("operation");
    assert_eq!(summary.operations, 1);
    assert_eq!(summary.revision, 3);

    let reopened = OpenCam::new().open(&path).expect("reopen");
    assert_eq!(reopened.id, "bracket");
    assert_eq!(reopened.stock.width, 120.0);
    assert_eq!(reopened.operations, 1);

    let report = session.report().expect("report");
    assert!(report.validation.ok);
    assert_eq!(report.toolpaths.operations.len(), 1);
    assert!(report.gcode_lines > 5);

    let target = directory.join("holes.nc");
    let exported = session.export(ExportFormat::Gcode, &target).expect("export");
    assert_eq!(exported.bytes, fs::metadata(&target).expect("metadata").len() as usize);
    let program = fs::read_to_string(&target).expect("gcode");
    assert!(program.contains("G81"));
}

#[test]
fn a_tool_in_use_cannot_be_removed() {
    let directory = workspace("in-use");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.add_tool(tool()).expect("tool");
    let mut pocket = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    pocket.x = 10.0;
    pocket.y = 10.0;
    pocket.width = 40.0;
    pocket.height = 30.0;
    session.add_operation(pocket).expect("operation");

    let error = session.remove_tool("t6").expect_err("tool is in use");
    assert_eq!(error.code, "tool/in-use");
}

#[test]
fn invalid_operation_is_rejected_before_it_is_saved() {
    let directory = workspace("invalid");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.add_tool(tool()).expect("tool");

    let mut operation = Operation::new("bad", OperationKind::Pocket, "t6", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 4.0;
    operation.height = 4.0;
    let error = session.add_operation(operation).expect_err("pocket narrower than the tool");
    assert_eq!(error.code, "operation/invalid");
    assert_eq!(session.summary().expect("summary").operations, 0);
}

#[test]
fn create_refuses_an_existing_file() {
    let directory = workspace("exists");
    let path = directory.join("part.ocam");
    fs::write(&path, "{}").expect("seed");
    let error = OpenCam::new().create(&path).expect_err("file exists");
    assert_eq!(error.code, "file/exists");
}

#[test]
fn missing_document_reports_no_document() {
    let error = OpenCam::new().validate().expect_err("nothing open");
    assert_eq!(error.code, "session/no-document");
}

#[test]
fn save_as_retargets_the_session() {
    let directory = workspace("save-as");
    let source = directory.join("source.ocam");
    let target = directory.join("copy.ocam");
    let mut session = OpenCam::new();
    session.create(&source).expect("create");
    let summary = session.save(Some(Path::new(&target))).expect("save as");
    assert!(summary.path.ends_with("copy.ocam"));
    assert!(target.exists());
}
