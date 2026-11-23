use cam_model::{
    CamDocument, LeadSettings, MoveKind, Operation, OperationKind, Point, Severity, Stock, TabSettings, Tool,
    generate_toolpaths, render_gcode, validate::validate,
};

const RADIUS: f64 = 5.0;

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

/// Offset profile `(17, 17) … (63, 43)`; one level at `Z8`.
fn contour() -> Operation {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 2.0, 2.0);
    operation.x = 20.0;
    operation.y = 20.0;
    operation.width = 40.0;
    operation.height = 20.0;
    operation
}

fn led_contour(direction: &str) -> Operation {
    let mut operation = contour();
    operation.lead = Some(LeadSettings::new(RADIUS, direction));
    operation
}

/// Circular contour centred at (50, 50); tool-centre start `(93, 50)`.
fn circle() -> Operation {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 2.0, 2.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 80.0;
    operation.height = 80.0;
    operation.shape = Some("circle".to_string());
    operation.lead = Some(LeadSettings::new(RADIUS, "cw"));
    operation
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn on_circle(move_: &cam_model::Move, centre: Point) -> bool {
    let radius = ((move_.from.x - centre.x).powi(2) + (move_.from.y - centre.y).powi(2)).sqrt();
    let to_radius = ((move_.to.x - centre.x).powi(2) + (move_.to.y - centre.y).powi(2)).sqrt();
    close(radius, RADIUS) && close(to_radius, RADIUS)
}

fn error_messages(operation: Operation) -> Vec<String> {
    validate(&document(operation))
        .issues
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .map(|issue| issue.message.clone())
        .collect()
}

#[test]
fn a_contour_without_lead_is_byte_identical_to_the_previous_output() {
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
        "G0 X17 Y17 Z15",
        "G1 X17 Y17 Z8 F300",
        "G1 X63 Y17 Z8 F1200",
        "G1 X63 Y43 Z8 F1200",
        "G1 X17 Y43 Z8 F1200",
        "G1 X17 Y17 Z8 F1200",
        "G0 X17 Y17 Z15",
        "G0 Z15",
        "M5",
        "G0 Z15",
        "M30",
        "%",
    ]
    .join("\n")
        + "\n";

    assert_eq!(render_gcode(&document).expect("gcode"), expected);
    assert!(!serde_json::to_string(&document).expect("serialise").contains("lead"));
    let path = &generate_toolpaths(&document).expect("toolpaths").operations[0];
    assert_eq!(path.segments[1].kind, MoveKind::Plunge);
    assert_eq!(path.segments.iter().filter(|segment| segment.kind == MoveKind::Arc).count(), 0);
}

#[test]
fn a_lead_in_arc_joins_the_profile_start_tangentially() {
    let report = generate_toolpaths(&document(led_contour("cw"))).expect("valid lead contour");
    let path = &report.operations[0];

    assert_eq!(path.segments[0].kind, MoveKind::Rapid);
    assert!(close(path.segments[0].to.x, 17.0) && close(path.segments[0].to.y, 7.0) && close(path.segments[0].to.z, 15.0));
    let plunge = &path.segments[1];
    assert_eq!(plunge.kind, MoveKind::Plunge);
    assert!(close(plunge.to.x, 17.0) && close(plunge.to.y, 7.0) && close(plunge.to.z, 8.0));

    let lead_in = &path.segments[2];
    assert_eq!(lead_in.kind, MoveKind::Arc);
    assert!(close(lead_in.to.x, 17.0) && close(lead_in.to.y, 17.0), "the arc must end exactly at the profile start");
    let arc = lead_in.arc.expect("arc parameters");
    assert!(close(arc.centre.x, 17.0) && close(arc.centre.y, 12.0), "the centre lies on the outward normal of the bottom edge");
    assert!(arc.clockwise);
    assert!(on_circle(lead_in, arc.centre), "both ends are one radius from the centre");
    assert!(close(lead_in.length, RADIUS * std::f64::consts::PI), "a half circle spans pi times the radius");
    assert_eq!(lead_in.feed, 1200.0);

    let first_cut = &path.segments[3];
    assert_eq!(first_cut.kind, MoveKind::Cut);
    assert!(close(first_cut.from.x, 17.0) && close(first_cut.from.y, 17.0) && close(first_cut.to.x, 63.0));
}

#[test]
fn a_lead_out_arc_leaves_the_closing_point_tangentially() {
    let report = generate_toolpaths(&document(led_contour("cw"))).expect("valid lead contour");
    let path = &report.operations[0];

    let lead_out = &path.segments[7];
    assert_eq!(lead_out.kind, MoveKind::Arc);
    assert!(close(lead_out.from.x, 17.0) && close(lead_out.from.y, 17.0), "the arc starts at the closing point");
    assert!(close(lead_out.to.x, 7.0) && close(lead_out.to.y, 17.0) && close(lead_out.to.z, 8.0));
    let arc = lead_out.arc.expect("arc parameters");
    assert!(close(arc.centre.x, 12.0) && close(arc.centre.y, 17.0), "the centre lies on the outward normal of the left edge");
    assert!(arc.clockwise);
    assert!(on_circle(lead_out, arc.centre));
    assert!(close(lead_out.length, RADIUS * std::f64::consts::PI));

    let retract = &path.segments[8];
    assert_eq!(retract.kind, MoveKind::Rapid);
    assert!(close(retract.from.x, 7.0) && close(retract.from.y, 17.0) && close(retract.to.z, 15.0));
    assert_eq!(path.segments.len(), 9);
}

#[test]
fn a_counter_clockwise_lead_mirrors_both_arcs_and_renders_g3() {
    let report = generate_toolpaths(&document(led_contour("ccw"))).expect("valid lead contour");
    let path = &report.operations[0];

    let lead_in = &path.segments[2];
    let lead_out = &path.segments[7];
    assert_eq!(lead_in.kind, MoveKind::Arc);
    assert_eq!(lead_out.kind, MoveKind::Arc);
    let in_arc = lead_in.arc.expect("lead-in parameters");
    let out_arc = lead_out.arc.expect("lead-out parameters");
    assert!(!in_arc.clockwise && !out_arc.clockwise);
    assert!(close(in_arc.centre.x, 17.0) && close(in_arc.centre.y, 22.0));
    assert!(close(lead_in.from.x, 17.0) && close(lead_in.from.y, 27.0));
    assert!(close(out_arc.centre.x, 22.0) && close(out_arc.centre.y, 17.0));
    assert!(close(lead_out.to.x, 27.0) && close(lead_out.to.y, 17.0));

    let program = render_gcode(&document(led_contour("ccw"))).expect("gcode");
    assert!(program.contains("G3 X17 Y17 Z8 I0 J-5 F1200"));
    assert!(program.contains("G3 X27 Y17 Z8 I5 J0 F1200"));
    assert!(!program.contains("G2 X"), "no clockwise arc in the ccw program");
}

#[test]
fn a_clockwise_lead_renders_g2_lines_with_centre_offsets() {
    let program = render_gcode(&document(led_contour("cw"))).expect("gcode");
    assert!(program.contains("G2 X17 Y17 Z8 I0 J5 F1200"), "lead-in: centre offset from the plunge point");
    assert!(program.contains("G2 X7 Y17 Z8 I-5 J0 F1200"), "lead-out: centre offset from the closing point");
    let lines: Vec<&str> = program.lines().filter(|line| line.starts_with("G2 X")).collect();
    assert_eq!(lines.len(), 2, "one lead-in and one lead-out arc per level");
}

#[test]
fn circular_lead_arcs_share_the_circle_without_retracing() {
    let report = generate_toolpaths(&document(circle())).expect("valid circular lead contour");
    let path = &report.operations[0];

    let lead_in = &path.segments[2];
    assert_eq!(lead_in.kind, MoveKind::Arc);
    assert!(close(lead_in.from.x, 103.0) && close(lead_in.from.y, 50.0));
    assert!(close(lead_in.to.x, 93.0) && close(lead_in.to.y, 50.0), "the arc joins the ring at angle zero");
    let in_arc = lead_in.arc.expect("lead-in parameters");
    assert!(close(in_arc.centre.x, 98.0) && close(in_arc.centre.y, 50.0));
    assert!(in_arc.clockwise);

    let lead_out = &path.segments[path.segments.len() - 2];
    assert_eq!(lead_out.kind, MoveKind::Arc);
    assert!(close(lead_out.from.x, 93.0) && close(lead_out.from.y, 50.0));
    assert!(close(lead_out.to.x, 103.0) && close(lead_out.to.y, 50.0));
    let out_arc = lead_out.arc.expect("lead-out parameters");
    assert!(
        close(out_arc.centre.x, 98.0) && close(out_arc.centre.y, 50.0),
        "the ring has one tangent, so both arcs share the circle"
    );
    assert!(on_circle(lead_in, in_arc.centre) && on_circle(lead_out, out_arc.centre));

    // The two half circles cover opposite halves: their midpoints differ.
    let midpoint = |segment: &cam_model::Move| {
        let centre = segment.arc.expect("arc parameters").centre;
        let start = (segment.from.y - centre.y).atan2(segment.from.x - centre.x);
        let sweep = if segment.arc.expect("arc parameters").clockwise { -std::f64::consts::PI } else { std::f64::consts::PI };
        let angle = start + sweep / 2.0;
        Point::new(centre.x + RADIUS * angle.cos(), centre.y + RADIUS * angle.sin(), 0.0)
    };
    let in_mid = midpoint(lead_in);
    let out_mid = midpoint(lead_out);
    assert!(!close(in_mid.y, out_mid.y), "the lead-in and lead-out halves must not overlap");
    assert!((in_mid.x - 98.0).abs() < 1e-9 && (in_mid.y - 45.0).abs() < 1e-9);
    assert!((out_mid.x - 98.0).abs() < 1e-9 && (out_mid.y - 55.0).abs() < 1e-9);
    assert!((lead_in.to.z - lead_in.from.z).abs() < 1e-9, "lead arcs stay at the level depth");
}

#[test]
fn lead_arcs_count_as_cutting_length() {
    let plain = generate_toolpaths(&document(contour())).expect("valid contour");
    let led = generate_toolpaths(&document(led_contour("cw"))).expect("valid lead contour");
    let difference = led.cut_length - plain.cut_length;
    // One level replaces nothing but adds two half circles of radius 5.
    assert!(close(difference, 2.0 * RADIUS * std::f64::consts::PI), "exactly the two lead arcs, got {difference}");
}

#[test]
fn a_lead_combines_with_tabs_and_multi_level_stepdown() {
    let mut operation = led_contour("cw");
    operation.depth = 3.0;
    operation.stepdown = 1.5;
    operation.tabs = Some(TabSettings::new(4, 5.0, 1.0));
    let report = generate_toolpaths(&document(operation)).expect("valid tabbed lead contour");
    let path = &report.operations[0];

    let arcs: Vec<&cam_model::Move> = path.segments.iter().filter(|segment| segment.kind == MoveKind::Arc).collect();
    assert_eq!(arcs.len(), 4, "two lead arcs per level");
    for (index, expected_z) in [(0, 8.5), (1, 8.5), (2, 7.0), (3, 7.0)] {
        assert!(close(arcs[index].from.z, expected_z) && close(arcs[index].to.z, expected_z));
    }
    for arc in &arcs {
        assert!(close(arc.length, RADIUS * std::f64::consts::PI));
    }
}

#[test]
fn invalid_lead_parameters_are_rejected() {
    let messages = error_messages({
        let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
        operation.x = 20.0;
        operation.y = 20.0;
        operation.width = 40.0;
        operation.height = 20.0;
        operation.stepover = Some(3.0);
        operation.lead = Some(LeadSettings::new(RADIUS, "cw"));
        operation
    });
    assert!(messages.iter().any(|message| message.contains("uses lead but only `contour` supports it")), "{messages:?}");

    for radius in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        let mut operation = contour();
        operation.lead = Some(LeadSettings::new(radius, "cw"));
        let messages = error_messages(operation);
        assert!(
            messages.iter().any(|message| message.contains("lead radius must be finite and greater than zero")),
            "{messages:?}"
        );
    }

    for direction in ["sideways", "", "CW"] {
        let mut operation = contour();
        operation.lead = Some(LeadSettings::new(RADIUS, direction));
        let messages = error_messages(operation);
        assert!(
            messages.iter().any(|message| message.contains("lead direction") && message.contains("is unknown")),
            "{messages:?}"
        );
    }

    let mut operation = led_contour("cw");
    operation.ramp = Some(10.0);
    let messages = error_messages(operation);
    assert!(messages.iter().any(|message| message.contains("combines lead with ramp")), "{messages:?}");

    let valid = validate(&document(led_contour("cw")));
    assert!(valid.ok, "{:?}", valid.issues);
}

#[test]
fn lead_output_is_deterministic_and_round_trips() {
    let first = generate_toolpaths(&document(led_contour("cw"))).expect("valid");
    let second = generate_toolpaths(&document(led_contour("cw"))).expect("valid");
    assert_eq!(first, second);
    assert_eq!(serde_json::to_string(&first).expect("serialise"), serde_json::to_string(&second).expect("serialise"));

    let document = document(led_contour("cw"));
    let text = serde_json::to_string(&document).expect("serialise document");
    assert!(text.contains("\"lead\":{\"radius\":5.0,\"direction\":\"cw\"}"), "{text}");
    let restored: CamDocument = serde_json::from_str(&text).expect("deserialise");
    assert_eq!(restored.operations[0].lead.as_ref().expect("lead").radius, RADIUS);
    assert_eq!(serde_json::to_string(&restored).expect("reserialise"), text);

    let report = serde_json::to_string(&first).expect("serialise report");
    assert!(report.contains("\"kind\":\"arc\""));
    assert!(report.contains("\"arc\":{\"centre\":{\"x\":17"));
}
