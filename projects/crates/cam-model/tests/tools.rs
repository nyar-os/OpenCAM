use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use cam_model::{LIBRARY_FORMAT, LIBRARY_VERSION, OpenCam, Operation, OperationKind, Stock, Tool, ToolLibrary};

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

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-tools-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

/// Creates a session with two tools and two operations on the first tool.
fn session(directory: &Path) -> (PathBuf, OpenCam) {
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1)).expect("first tool");
    session.add_tool(tool("t3", 2)).expect("second tool");
    session.add_operation(Operation::new("face", OperationKind::Face, "t6", 1.0, 1.0)).expect("face");
    let mut pocket = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    pocket.x = 10.0;
    pocket.y = 10.0;
    pocket.width = 40.0;
    pocket.height = 30.0;
    pocket.stepover = Some(6.0);
    session.add_operation(pocket).expect("pocket");
    (path, session)
}

#[test]
fn tool_table_lists_every_tool_with_its_operations() {
    let directory = workspace("table");
    let (_path, session) = session(&directory);
    let table = session.tool_table().expect("tool table");

    assert_eq!(table.format, "open-cam-tool-table");
    assert_eq!(table.version, 1);
    assert_eq!(table.units, "mm");
    assert_eq!(table.tool_count, 2);
    assert_eq!(table.unused_count, 1);

    let used = &table.tools[0];
    assert_eq!(used.tool.id, "t6");
    assert!(used.used);
    assert_eq!(used.used_by, vec!["face".to_string(), "pocket".to_string()]);
    assert_eq!(used.tool.diameter, 6.0);

    let unused = &table.tools[1];
    assert_eq!(unused.tool.id, "t3");
    assert!(!unused.used);
    assert!(unused.used_by.is_empty());
}

#[test]
fn a_library_written_from_one_document_merges_into_another() {
    let directory = workspace("roundtrip");
    let (_path, session) = session(&directory);
    let library_path = directory.join("shop-tools.json");
    let report = session.write_tool_library(&library_path).expect("write library");

    assert_eq!(report.format, "tools");
    assert_eq!(report.files, vec![library_path.display().to_string()]);
    assert!(report.bytes > 0);

    let text = fs::read_to_string(&library_path).expect("read library");
    assert!(text.contains("\"format\": \"open-cam-tools\""));
    let library: ToolLibrary = serde_json::from_str(&text).expect("parse library");
    assert_eq!(library.format, LIBRARY_FORMAT);
    assert_eq!(library.version, LIBRARY_VERSION);
    assert_eq!(library.units, "mm");
    assert_eq!(library.tools, vec![tool("t6", 1), tool("t3", 2)]);

    let target = directory.join("second.ocam");
    let mut second = OpenCam::new();
    second.create(&target).expect("create second");
    second.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    let imported = second.import_tool_library(&library_path).expect("import");

    assert_eq!(imported.format, "open-cam-tool-import");
    assert_eq!(imported.version, 1);
    assert_eq!(imported.path, library_path.display().to_string());
    assert_eq!(imported.imported, vec!["t6".to_string(), "t3".to_string()]);
    assert_eq!(imported.tool_count, 2);
    assert_eq!(second.tool_table().expect("table").tools[0].tool, tool("t6", 1));
    assert_eq!(second.summary().expect("summary").revision, 2, "the import is one edit");
}

#[test]
fn a_rejected_library_leaves_the_document_untouched() {
    let directory = workspace("reject");
    let (_path, mut session) = session(&directory);

    let duplicate = directory.join("duplicate.json");
    fs::write(&duplicate, serde_json::to_string(&ToolLibrary::new(vec![tool("t6", 1)])).expect("serialise")).expect("write");
    let error = session.import_tool_library(&duplicate).expect_err("duplicate tool");
    assert_eq!(error.code, "tool/duplicate-id");
    assert_eq!(session.summary().expect("summary").tools, 2);
    assert_eq!(session.summary().expect("summary").revision, 5);

    let twice = directory.join("twice.json");
    fs::write(&twice, serde_json::to_string(&ToolLibrary::new(vec![tool("t1", 3), tool("t1", 4)])).expect("serialise"))
        .expect("write");
    assert_eq!(session.import_tool_library(&twice).expect_err("library duplicate").code, "tool/duplicate-id");

    let wrong_format = directory.join("wrong-format.json");
    fs::write(&wrong_format, r#"{"format":"other","version":1,"units":"mm","tools":[]}"#).expect("write");
    assert_eq!(session.import_tool_library(&wrong_format).expect_err("format").code, "tool/library-invalid");

    let wrong_version = directory.join("wrong-version.json");
    fs::write(&wrong_version, r#"{"format":"open-cam-tools","version":2,"units":"mm","tools":[]}"#).expect("write");
    assert_eq!(session.import_tool_library(&wrong_version).expect_err("version").code, "tool/library-invalid");

    let wrong_units = directory.join("wrong-units.json");
    fs::write(&wrong_units, r#"{"format":"open-cam-tools","version":1,"units":"in","tools":[]}"#).expect("write");
    assert_eq!(session.import_tool_library(&wrong_units).expect_err("units").code, "tool/library-invalid");

    let malformed = directory.join("malformed.json");
    fs::write(&malformed, "not json").expect("write");
    assert_eq!(session.import_tool_library(&malformed).expect_err("malformed").code, "tool/library-invalid");

    let invalid_tool = directory.join("invalid-tool.json");
    let mut broken = tool("t9", 5);
    broken.diameter = 0.0;
    fs::write(&invalid_tool, serde_json::to_string(&ToolLibrary::new(vec![broken])).expect("serialise")).expect("write");
    assert_eq!(session.import_tool_library(&invalid_tool).expect_err("invalid tool").code, "tool/invalid");

    assert_eq!(session.import_tool_library(&directory.join("missing.json")).expect_err("missing file").code, "io/read");

    let summary = session.summary().expect("summary");
    assert_eq!(summary.tools, 2, "every rejected import leaves the tool table alone");
    assert_eq!(summary.revision, 5, "a rejected import does not bump the revision");
}
