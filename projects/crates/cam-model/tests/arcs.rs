use cam_model::{
    CamDocument, MoveKind, Operation, OperationKind, Point, Severity, Stock, TabSettings, Tool, generate_toolpaths,
    render_gcode, validate::validate,
};

const RADIUS: f64 = 3.0;

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
    let mut document = CamDocument::new(Stock::new(120.0, 100.0, 10.0));
    document.tools.push(tool());
    document.operations.push(operation);
    document
}

/// Circular pocket `(10, 10)` 80 mm across, cleared with 6 mm stepovers.
fn circular_pocket() -> Operation {
    let mut operation = Operation::new("bore", OperationKind::Pocket, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 80.0;
    operation.height = 80.0;
    operation.stepover = Some(6.0);
    operation.shape = Some("circle".to_string());
    operation
}

/// Circular contour 40 mm across, centred at `(40, 40)`, two Z levels.
fn circular_contour() -> Operation {
    let mut operation = Operation::new("boss", OperationKind::Contour, "t6", 4.0, 2.0);
    operation.x = 20.0;
    operation.y = 20.0;
    operation.width = 40.0;
    operation.height = 40.0;
    operation.shape = Some("circle".to_string());
    operation
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn error_code(operation: Operation) -> String {
    let report = validate(&document(operation));
    report.issues.iter().find(|issue| issue.severity == Severity::Error).map(|issue| issue.code.clone()).expect("an error")
}

fn arcs(operation: &cam_model::OperationPath) -> Vec<&cam_model::Move> {
    operation.segments.iter().filter(|segment| segment.kind == MoveKind::Arc).collect()
}

#[test]
fn arc_output_is_opt_in_and_keeps_the_default_byte_identical() {
    let plain = generate_toolpaths(&document(circular_pocket())).expect("valid circular pocket");
    let mut linear = circular_pocket();
    linear.arcs = Some("linear".to_string());
    let explicit = generate_toolpaths(&document(linear.clone())).expect("valid circular pocket");

    assert_eq!(plain, explicit);
    assert_eq!(render_gcode(&document(circular_pocket())).expect("gcode"), render_gcode(&document(linear)).expect("gcode"));
    assert!(!serde_json::to_string(&circular_pocket()).expect("serialise").contains("arcs"));
    assert_eq!(arcs(&plain.operations[0]).len(), 0, "the default keeps chord approximation");
}

#[test]
fn circular_pocket_rings_are_counter_clockwise_half_arcs() {
    let mut operation = circular_pocket();
    operation.arcs = Some("arc".to_string());
    let document = document(operation);
    let report = generate_toolpaths(&document).expect("valid arc pocket");
    let path = &report.operations[0];

    // Seven rings (37, 31, … 1 mm), two half arcs each.
    assert_eq!(path.loop_count, Some(7));
    let arcs = arcs(path);
    assert_eq!(arcs.len(), 14);
    for arc in &arcs {
        let params = arc.arc.expect("an arc move carries its centre");
        assert!(!params.clockwise, "rings run counter-clockwise and render G3");
        let radius = ((arc.from.x - params.centre.x).powi(2) + (arc.from.y - params.centre.y).powi(2)).sqrt();
        assert!(close(arc.length, std::f64::consts::PI * radius), "arc length {} is not half of radius {radius}", arc.length);
        assert!(close(arc.from.z, arc.to.z), "a ring arc stays at one depth");
    }
    assert_eq!(arcs.iter().filter(|arc| close(arc.from.x - 50.0, 37.0)).count(), 1, "the first ring starts at radius 37");
    assert!(close(arcs[13].to.x, 51.0) && close(arcs[13].to.y, 50.0), "the last ring closes on its start");

    let linear = generate_toolpaths(&document_without_arcs()).expect("valid linear pocket");
    let ratio = path.cut_length / linear.operations[0].cut_length;
    assert!(ratio > 1.0, "true arcs are longer than the inscribed 72-gon");
    assert!(ratio < 1.001, "both approximations must stay within 0.1 %, got {ratio}");

    let program = render_gcode(&document).expect("gcode");
    assert_eq!(program.lines().filter(|line| line.starts_with("G3 ")).count(), 14);
    assert_eq!(program.lines().filter(|line| line.starts_with("G2 ")).count(), 0);
    assert!(program.contains("F1200"));
}

fn document_without_arcs() -> CamDocument {
    document(circular_pocket())
}

#[test]
fn circular_contour_levels_are_two_g3_half_arcs() {
    let mut operation = circular_contour();
    operation.arcs = Some("arc".to_string());
    let document = document(operation);
    let report = generate_toolpaths(&document).expect("valid arc contour");
    let path = &report.operations[0];

    assert_eq!(path.shape.as_deref(), Some("circle"));
    let arcs = arcs(path);
    assert_eq!(arcs.len(), 4, "two levels, two half arcs each");
    for (index, arc) in arcs.iter().enumerate() {
        let params = arc.arc.expect("arc centre");
        assert!(close(params.centre.x, 40.0) && close(params.centre.y, 40.0), "arc {index} leaves the profile centre");
        let radius = ((arc.from.x - params.centre.x).powi(2) + (arc.from.y - params.centre.y).powi(2)).sqrt();
        assert!(close(radius, 20.0 + RADIUS), "the ring must sit at width / 2 + r, got {radius}");
        assert!(close(arc.length, std::f64::consts::PI * radius));
        assert!(!params.clockwise);
    }
    assert_eq!(arcs[1].to.x, 63.0);
    assert_eq!(arcs[1].to.y, 40.0);

    let program = render_gcode(&document).expect("gcode");
    assert_eq!(program.lines().filter(|line| line.starts_with("G3 ")).count(), 4);
}

#[test]
fn tabbed_arc_contour_keeps_the_lifts_and_cuts_arcs_between_them() {
    let mut operation = circular_contour();
    operation.arcs = Some("arc".to_string());
    operation.tabs = Some(TabSettings::new(3, 8.0, 1.0));
    let document = document(operation);
    let path = &generate_toolpaths(&document).expect("valid tabbed arc contour").operations[0];

    let arcs = arcs(path);
    assert_eq!(arcs.len(), 8, "four cutting intervals per level stay below a half circle");
    for arc in &arcs {
        let radius = ((arc.from.x - 40.0).powi(2) + (arc.from.y - 40.0).powi(2)).sqrt();
        assert!(arc.length <= std::f64::consts::PI * radius + 1e-9, "every arc is at most a half circle");
        assert!(!arc.arc.expect("arc centre").clockwise);
    }
    let plunges = path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).count();
    assert_eq!(plunges, 8, "one entry per level plus three tab-end plunges per level");
    let lifts = path
        .segments
        .iter()
        .filter(|segment| segment.kind == MoveKind::Rapid && close(segment.to.z - segment.from.z, 1.0))
        .count();
    assert_eq!(lifts, 6, "one lift over every tab");

    let program = render_gcode(&document).expect("gcode");
    assert_eq!(program.lines().filter(|line| line.starts_with("G3 ")).count(), 8);
}

#[test]
fn arc_mode_is_validated() {
    let mut face = Operation::new("face", OperationKind::Face, "t6", 1.0, 1.0);
    face.arcs = Some("arc".to_string());
    assert_eq!(error_code(face), "operation/invalid");

    let mut rectangle = circular_pocket();
    rectangle.shape = Some("rect".to_string());
    rectangle.arcs = Some("arc".to_string());
    assert_eq!(error_code(rectangle), "operation/invalid");

    let mut contour = circular_contour();
    contour.shape = None;
    contour.arcs = Some("arc".to_string());
    assert_eq!(error_code(contour), "operation/invalid");

    let mut unknown = circular_pocket();
    unknown.arcs = Some("spline".to_string());
    assert_eq!(error_code(unknown), "operation/invalid");

    let mut linear = circular_pocket();
    linear.arcs = Some("linear".to_string());
    assert!(validate(&document(linear)).ok);
}

#[test]
fn arc_output_is_deterministic_and_round_trips() {
    let mut operation = circular_pocket();
    operation.arcs = Some("arc".to_string());
    let document = document(operation.clone());
    let first = generate_toolpaths(&document).expect("valid");
    let second = generate_toolpaths(&document).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));

    let text = serde_json::to_string(&document).expect("serialise document");
    assert!(text.contains("\"arcs\":\"arc\""));
    let restored: CamDocument = serde_json::from_str(&text).expect("deserialise");
    assert_eq!(restored.operations[0].arcs.as_deref(), Some("arc"));
    assert_eq!(serde_json::to_string(&restored).expect("reserialise"), text);
    assert_eq!(generate_toolpaths(&restored).expect("reopened"), first);

    let point = Point::new(51.0, 50.0, 8.0);
    assert!(first.operations[0].segments.iter().any(|segment| segment.kind == MoveKind::Arc && segment.to == point));
}
