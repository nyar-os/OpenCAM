use cam_model::{CamDocument, Hole, Operation, OperationKind, Severity, Stock, Tool, validate::validate};

fn tool(id: &str) -> Tool {
    Tool {
        id: id.to_string(),
        name: id.to_string(),
        number: 1,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

#[test]
fn empty_document_is_valid_with_warnings() {
    let document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    let report = validate(&document);
    assert!(report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "tool/none"));
    assert!(report.issues.iter().any(|issue| issue.code == "operation/none"));
}

#[test]
fn missing_tool_rejects_the_operation() {
    let mut document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    document.operations.push(Operation::new("face", OperationKind::Face, "missing", 1.0, 0.5));
    let report = validate(&document);
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "operation/unknown-tool"));
}

#[test]
fn depth_beyond_stock_is_an_error() {
    let mut document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    document.tools.push(tool("t1"));
    document.operations.push(Operation::new("deep", OperationKind::Face, "t1", 20.0, 5.0));
    let report = validate(&document);
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "path/outside-stock"));
}

#[test]
fn holes_must_stay_on_the_stock() {
    let mut document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    document.tools.push(tool("t1"));
    let mut operation = Operation::new("holes", OperationKind::Drill, "t1", 4.0, 4.0);
    operation.holes = vec![Hole::new(120.0, 10.0)];
    document.operations.push(operation);
    let report = validate(&document);
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "path/outside-stock"));
}

#[test]
fn pocket_narrower_than_the_tool_is_an_error() {
    let mut document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    document.tools.push(tool("t1"));
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t1", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 5.0;
    operation.height = 5.0;
    document.operations.push(operation);
    let report = validate(&document);
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "operation/invalid"));
}

#[test]
fn duplicate_ids_are_errors() {
    let mut document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    document.tools.push(tool("t1"));
    document.tools.push(tool("t1"));
    let report = validate(&document);
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "tool/duplicate-id"));
    assert!(report.issues.iter().any(|issue| issue.severity == Severity::Error));
}

#[test]
fn unsupported_units_are_rejected() {
    let mut document = CamDocument::new(Stock::new(100.0, 100.0, 10.0));
    document.units = "in".to_string();
    let report = validate(&document);
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| issue.code == "document/units"));
}
