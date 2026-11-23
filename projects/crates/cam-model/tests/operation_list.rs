use std::{fs, path::PathBuf, process};

use cam_model::{
    CamDocument, EstimateRequest, Hole, HolePattern, Island, OpenCam, Operation, OperationKind, PathPoint, Stock, Tool,
    estimate_cycle_time, generate_toolpaths, operation_list,
};

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
    let mut operation = Operation::new(id, OperationKind::Face, tool, 1.0, 0.5);
    operation.name = id.to_string();
    operation.stepover = Some(4.0);
    operation
}

fn contour(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Contour, tool, 8.0, 2.0);
    operation.name = id.to_string();
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation
}

fn pocket(id: &str, tool: &str, strategy: Option<&str>) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Pocket, tool, 3.0, 1.0);
    operation.name = id.to_string();
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 60.0;
    operation.height = 40.0;
    operation.stepover = Some(2.0);
    operation.strategy = strategy.map(str::to_string);
    operation
}

fn drill_pattern(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Drill, tool, 6.0, 6.0);
    operation.name = id.to_string();
    operation.x = 20.0;
    operation.y = 20.0;
    operation.pattern = Some(HolePattern::grid(2, 2, 20.0, 20.0));
    operation.peck = Some(2.0);
    operation
}

fn drill_dwell(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Drill, tool, 5.0, 5.0);
    operation.name = id.to_string();
    operation.holes = vec![Hole::new(80.0, 70.0), Hole::new(90.0, 70.0), Hole::new(100.0, 70.0)];
    operation.dwell = Some(0.5);
    operation
}

fn engrave(id: &str, tool: &str) -> Operation {
    let mut operation = Operation::new(id, OperationKind::Engrave, tool, 0.5, 0.5);
    operation.name = id.to_string();
    operation.path = vec![PathPoint::new(10.0, 80.0), PathPoint::new(50.0, 80.0), PathPoint::new(50.0, 90.0)];
    operation
}

/// Every kind, a default and a spiral pocket, with two used tools and one unused.
fn document() -> CamDocument {
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool("t6", 1, 6.0));
    document.tools.push(tool("t3", 2, 3.0));
    document.tools.push(tool("t1", 3, 1.0));
    document.operations.push(face("face", "t6"));
    document.operations.push(contour("profile", "t6"));

    let mut island_pocket = pocket("floor", "t3", None);
    island_pocket.x = 70.0;
    island_pocket.y = 50.0;
    island_pocket.width = 40.0;
    island_pocket.height = 40.0;
    island_pocket.islands = vec![Island::new(80.0, 60.0, 10.0, 10.0)];
    document.operations.push(island_pocket);

    let mut spiral = pocket("spiral", "t3", Some("spiral"));
    spiral.x = 10.0;
    spiral.y = 50.0;
    spiral.width = 40.0;
    spiral.height = 40.0;
    spiral.finish = Some(true);
    document.operations.push(spiral);

    document.operations.push(drill_pattern("holes", "t6"));
    document.operations.push(drill_dwell("ring", "t3"));
    document.operations.push(engrave("logo", "t1"));
    document
}

#[test]
fn the_list_reports_every_operation_in_document_order_with_its_fields() {
    let report = operation_list(&document()).expect("valid document");

    assert_eq!(report.format, "open-cam-operation-list");
    assert_eq!(report.version, 1);
    assert_eq!(report.units, "mm");
    assert_eq!(report.operation_count, 7);
    assert_eq!(report.used_tools, 3);

    let ids: Vec<&str> = report.operations.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids, ["face", "profile", "floor", "spiral", "holes", "ring", "logo"]);

    let face = &report.operations[0];
    assert_eq!(face.kind, OperationKind::Face);
    assert_eq!(face.tool, "t6");
    assert_eq!(face.name, "face");
    assert_eq!(face.depth, 1.0);
    assert_eq!(face.stepdown, 0.5);
    assert_eq!(face.stepover, Some(4.0));
    assert_eq!(face.strategy, None);
    assert_eq!(face.shape, None);
    assert_eq!(face.holes, 0);
    assert_eq!(face.island_count, 0);
    assert!(!face.finish);

    let contour = &report.operations[1];
    assert_eq!(contour.strategy, None);
    assert_eq!(contour.shape.as_deref(), Some("rect"));
    assert_eq!(contour.holes, 0);

    let island_pocket = &report.operations[2];
    assert_eq!(island_pocket.strategy.as_deref(), Some("raster"));
    assert_eq!(island_pocket.shape.as_deref(), Some("rect"));
    assert_eq!(island_pocket.island_count, 1);
    assert!(!island_pocket.finish);

    let spiral = &report.operations[3];
    assert_eq!(spiral.strategy.as_deref(), Some("spiral"));
    assert_eq!(spiral.shape.as_deref(), Some("rect"));
    assert_eq!(spiral.island_count, 0);
    assert!(spiral.finish);

    let holes = &report.operations[4];
    assert_eq!(holes.kind, OperationKind::Drill);
    assert_eq!(holes.holes, 4);
    assert_eq!(holes.strategy, None);
    assert_eq!(holes.shape, None);

    let ring = &report.operations[5];
    assert_eq!(ring.holes, 3);

    let logo = &report.operations[6];
    assert_eq!(logo.kind, OperationKind::Engrave);
    assert_eq!(logo.holes, 0);
    assert_eq!(logo.island_count, 0);
    assert!(!logo.finish);
}

#[test]
fn the_list_reports_the_motion_time_of_each_operation_alone() {
    let document = document();
    let report = operation_list(&document).expect("valid document");
    let estimate = estimate_cycle_time(&document, &EstimateRequest::default()).expect("valid document");
    let paths = generate_toolpaths(&document).expect("valid document");

    for (index, entry) in report.operations.iter().enumerate() {
        let motion = estimate.operations[index].total_seconds - estimate.operations[index].toolchange_seconds;
        assert!((entry.estimated_seconds - motion).abs() < 1e-9);
        let path_motion = paths.operations[index].cutting_seconds + paths.operations[index].rapid_seconds;
        assert!((entry.estimated_seconds - path_motion).abs() < 1e-9);
        assert!(entry.estimated_seconds > 0.0);
    }
    assert!(report.operations[0].estimated_seconds < estimate.operations[0].total_seconds);
}

#[test]
fn the_list_is_deterministic() {
    let document = document();
    assert_eq!(operation_list(&document).expect("first"), operation_list(&document).expect("second"));
}

#[test]
fn listing_does_not_change_the_document_or_its_revision() {
    let directory = workspace("session");
    let path = directory.join("part.ocam");
    let mut session = OpenCam::new();
    session.create(&path).expect("create");
    session.set_stock(Stock::new(120.0, 100.0, 10.0)).expect("stock");
    session.add_tool(tool("t6", 1, 6.0)).expect("tool");
    session.add_operation(face("face", "t6")).expect("face");
    let before = fs::read_to_string(&path).expect("read");
    let revision = session.summary().expect("summary").revision;

    let report = session.operation_list().expect("list");
    assert_eq!(report.operation_count, 1);
    assert_eq!(session.summary().expect("summary").revision, revision);
    assert_eq!(fs::read_to_string(&path).expect("read"), before);

    let _ = fs::remove_dir_all(&directory);
}

fn workspace(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("open-cam-operation-list-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}
