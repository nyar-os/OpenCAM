use cam_model::{CamDocument, Hole, Operation, OperationKind, Stock, Tool};

fn tool() -> Tool {
    Tool {
        id: "t6".to_string(),
        name: "6 mm flat".to_string(),
        number: 3,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    }
}

fn document() -> CamDocument {
    let mut document = CamDocument::new(Stock::new(60.0, 40.0, 8.0));
    document.tools.push(tool());
    let mut drill = Operation::new("holes", OperationKind::Drill, "t6", 4.0, 4.0);
    drill.holes = vec![Hole::new(10.0, 10.0)];
    document.operations.push(drill);
    let mut face = Operation::new("face", OperationKind::Face, "t6", 1.0, 1.0);
    face.stepover = Some(5.0);
    document.operations.push(face);
    document
}

#[test]
fn program_has_a_fixed_header_and_footer() {
    let program = cam_model::render_gcode(&document()).expect("gcode");
    let lines: Vec<&str> = program.lines().collect();
    assert_eq!(lines[0], "%");
    assert!(lines.contains(&"G21 G90 G17 G94"));
    assert!(lines.contains(&"T3 M6"));
    assert!(lines.contains(&"M30"));
    assert_eq!(lines.last().copied(), Some("%"));
}

#[test]
fn program_is_byte_identical_on_every_run() {
    let first = cam_model::render_gcode(&document()).expect("gcode");
    let second = cam_model::render_gcode(&document()).expect("gcode");
    assert_eq!(first, second);
}

#[test]
fn drilling_uses_a_canned_cycle_and_is_cancelled() {
    let program = cam_model::render_gcode(&document()).expect("gcode");
    assert!(program.contains("G81 X10 Y10 Z4 R13 F300"));
    assert!(program.contains("G80"));
}

#[test]
fn every_operation_lifts_to_the_clearance_plane() {
    let program = cam_model::render_gcode(&document()).expect("gcode");
    assert!(program.contains("(operation holes - drill)"));
    assert!(program.contains("(operation face - face)"));
    assert!(program.contains("G0 Z13"));
}
