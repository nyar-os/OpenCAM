use cam_model::{
    CamDocument, MoveKind, Operation, OperationKind, Stock, Tool, generate_toolpaths, render_gcode, validate::validate,
};

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

fn document(operation: Operation) -> CamDocument {
    let mut document = CamDocument::new(Stock::new(100.0, 80.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

/// Offset profile `(7, 7) … (53, 33)`; the first cutting move runs +X.
fn contour() -> Operation {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 3.0, 1.5);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 20.0;
    operation
}

fn ramped_contour() -> Operation {
    let mut operation = contour();
    operation.ramp = Some(10.0);
    operation
}

fn pocket() -> Operation {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.stepover = Some(3.0);
    operation
}

#[test]
fn a_contour_without_ramp_is_byte_identical_to_the_previous_output() {
    let document = document(contour());
    let expected = [
        "%",
        concat!("(OpenCAM ", env!("CARGO_PKG_VERSION"), ")"),
        "(Units: mm)",
        "(Post: generic mill, absolute, XY plane)",
        "G21 G90 G17 G94",
        "G0 Z15",
        "(operation profile - contour)",
        "T1 M6",
        "S12000 M3",
        "(tool t6 diameter 6)",
        "G0 Z15",
        "G0 X7 Y7 Z15",
        "G1 X7 Y7 Z8.5 F300",
        "G1 X53 Y7 Z8.5 F1200",
        "G1 X53 Y33 Z8.5 F1200",
        "G1 X7 Y33 Z8.5 F1200",
        "G1 X7 Y7 Z8.5 F1200",
        "G0 X7 Y7 Z15",
        "G1 X7 Y7 Z7 F300",
        "G1 X53 Y7 Z7 F1200",
        "G1 X53 Y33 Z7 F1200",
        "G1 X7 Y33 Z7 F1200",
        "G1 X7 Y7 Z7 F1200",
        "G0 X7 Y7 Z15",
        "G0 Z15",
        "M5",
        "G0 Z15",
        "M30",
        "%",
    ]
    .join("\n")
        + "\n";

    assert_eq!(render_gcode(&document).expect("gcode"), expected);
    assert!(!serde_json::to_string(&document).expect("serialise").contains("ramp"));
    assert_eq!(generate_toolpaths(&document).expect("toolpaths").operations[0].segments[1].kind, MoveKind::Plunge);
}

#[test]
fn the_first_move_of_a_ramped_level_is_not_vertical() {
    let report = generate_toolpaths(&document(ramped_contour())).expect("valid ramp contour");
    let path = &report.operations[0];
    assert_eq!(path.segments[0].kind, MoveKind::Rapid);
    let entry = &path.segments[1];
    assert_eq!(entry.kind, MoveKind::Cut);
    assert!((entry.from.z - 15.0).abs() < 1e-9);
    assert!((entry.to.z - 8.5).abs() < 1e-9);
    assert!((entry.to.x - entry.from.x - 10.0).abs() < 1e-9);
    assert!((entry.to.y - entry.from.y).abs() < 1e-9);
    assert_eq!(path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).count(), 0);
}

#[test]
fn every_ramp_reaches_its_level_depth_without_cutting_lower() {
    let report = generate_toolpaths(&document(ramped_contour())).expect("valid ramp contour");
    let path = &report.operations[0];

    for (level, expected) in [(1_u8, 8.5_f64), (2, 7.0)] {
        let ramp = path
            .segments
            .iter()
            .find(|segment| {
                segment.kind == MoveKind::Cut && (segment.from.z - 15.0).abs() < 1e-9 && (segment.to.z - expected).abs() < 1e-9
            })
            .unwrap_or_else(|| panic!("level {level} has no ramp ending at {expected}"));
        assert!(ramp.to.z < ramp.from.z);
    }

    let lowest = path.segments.iter().flat_map(|segment| [segment.from.z, segment.to.z]).fold(f64::INFINITY, f64::min);
    assert!((lowest - 7.0).abs() < 1e-9, "no move goes below the last level");
}

#[test]
fn a_ramp_adds_cutting_length() {
    let plain = generate_toolpaths(&document(contour())).expect("valid contour");
    let ramped = generate_toolpaths(&document(ramped_contour())).expect("valid ramp contour");
    let difference = ramped.cut_length - plain.cut_length;
    // Two levels replace a 6.5 mm plunge with a descending run plus its return.
    assert!(difference > 10.0, "ramp must add cutting length, got {difference}");
    assert!(ramped.estimated_seconds > 0.0);
}

#[test]
fn a_ramped_pocket_enters_along_the_first_raster_line() {
    let mut operation = pocket();
    operation.ramp = Some(12.0);
    let report = generate_toolpaths(&document(operation)).expect("valid ramp pocket");
    let path = &report.operations[0];

    let entry = &path.segments[1];
    assert_eq!(entry.kind, MoveKind::Cut);
    assert!((entry.from.x - 13.0).abs() < 1e-9);
    assert!((entry.from.y - 13.0).abs() < 1e-9);
    assert!((entry.to.x - 25.0).abs() < 1e-9);
    assert!((entry.to.y - 13.0).abs() < 1e-9);
    assert!((entry.to.z - 9.0).abs() < 1e-9);
}

#[test]
fn ramp_is_rejected_on_unsupported_kinds_and_values() {
    let mut face = Operation::new("face", OperationKind::Face, "t6", 1.0, 0.5);
    face.ramp = Some(5.0);
    let report = validate(&document(face));
    assert!(!report.ok);
    assert!(report.issues.iter().any(|issue| {
        issue.code == "operation/invalid" && issue.message.contains("only `contour`, `pocket` and `engrave`")
    }));

    for ramp in [0.0, -2.0, f64::NAN] {
        let mut operation = contour();
        operation.ramp = Some(ramp);
        let report = validate(&document(operation));
        assert!(!report.ok, "ramp {ramp} must be rejected");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| { issue.code == "operation/invalid" && issue.message.contains("greater than zero") })
        );
    }
}

#[test]
fn ramp_output_is_deterministic_and_round_trips() {
    let first = generate_toolpaths(&document(ramped_contour())).expect("valid");
    let second = generate_toolpaths(&document(ramped_contour())).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));

    let document = document(ramped_contour());
    let text = serde_json::to_string(&document).expect("serialise document");
    assert!(text.contains("\"ramp\":10"));
    let restored: CamDocument = serde_json::from_str(&text).expect("deserialise");
    assert_eq!(restored.operations[0].ramp, Some(10.0));
    assert_eq!(serde_json::to_string(&restored).expect("reserialise"), text);
}
