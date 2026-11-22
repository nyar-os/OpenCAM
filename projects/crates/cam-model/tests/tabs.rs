use cam_model::{
    CamDocument, MoveKind, Operation, OperationKind, Stock, TabSettings, Tool, generate_toolpaths, render_gcode,
    validate::validate,
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

/// Offset profile `(7, 7) … (53, 33)`: width 46, height 26, perimeter 144.
fn contour() -> Operation {
    let mut operation = Operation::new("profile", OperationKind::Contour, "t6", 3.0, 1.5);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 20.0;
    operation
}

fn with_tabs() -> Operation {
    let mut operation = contour();
    operation.tabs = Some(TabSettings::new(4, 6.0, 1.0));
    operation
}

#[test]
fn every_level_lifts_once_per_tab() {
    let report = generate_toolpaths(&document(with_tabs())).expect("valid contour tabs");
    let path = &report.operations[0];

    // Two levels (1.5 and 3.0); each tab lift goes up by the tab height.
    let lifts: Vec<_> = path
        .segments
        .iter()
        .filter(|segment| segment.kind == MoveKind::Rapid && (segment.to.z - segment.from.z - 1.0).abs() < 1e-9)
        .collect();
    assert_eq!(lifts.len(), 8);

    for level in [8.5, 7.0] {
        assert_eq!(
            lifts.iter().filter(|segment| (segment.from.z - level).abs() < 1e-9).count(),
            4,
            "one lift per tab at level {level}"
        );
    }

    // One initial plunge plus one plunge back down after every tab, per level.
    let plunges = path.segments.iter().filter(|segment| segment.kind == MoveKind::Plunge).count();
    assert_eq!(plunges, 10);
}

#[test]
fn tabs_are_centred_on_each_perimeter_share() {
    let report = generate_toolpaths(&document(with_tabs())).expect("valid contour tabs");
    let path = &report.operations[0];

    // First tab: perimeter 144 / 4 = 36 spacing, centred at 18, width 6.
    let lift = path
        .segments
        .iter()
        .find(|segment| {
            segment.kind == MoveKind::Rapid && (segment.from.z - 8.5).abs() < 1e-9 && (segment.to.z - 9.5).abs() < 1e-9
        })
        .expect("first tab lift");
    assert!((lift.from.x - 22.0).abs() < 1e-9);
    assert!((lift.from.y - 7.0).abs() < 1e-9);

    let cross = path
        .segments
        .iter()
        .find(|segment| {
            segment.kind == MoveKind::Rapid
                && (segment.from.x - 22.0).abs() < 1e-9
                && (segment.from.y - 7.0).abs() < 1e-9
                && (segment.from.z - 9.5).abs() < 1e-9
                && (segment.to.z - 9.5).abs() < 1e-9
        })
        .expect("first tab crossing");
    assert!((cross.to.x - 28.0).abs() < 1e-9);
    assert!((cross.to.y - 7.0).abs() < 1e-9);

    // Second tab starts 12 mm up the right edge.
    assert!(
        path.segments.iter().any(|segment| {
            segment.kind == MoveKind::Rapid
                && (segment.from.x - 53.0).abs() < 1e-9
                && (segment.from.y - 12.0).abs() < 1e-9
                && (segment.to.z - 9.5).abs() < 1e-9
        }),
        "second tab lift"
    );

    // The deeper level leaves the same tab height above its own floor.
    assert!(
        path.segments.iter().any(|segment| segment.kind == MoveKind::Rapid && (segment.to.z - 8.0).abs() < 1e-9),
        "second level tab height"
    );
}

#[test]
fn tab_footprint_must_fit_the_perimeter() {
    let mut operation = contour();
    // 6 × 20 = 120, which equals the 2 × (40 + 20) perimeter.
    operation.tabs = Some(TabSettings::new(6, 20.0, 1.0));
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("less than the profile perimeter"))
    );
}

#[test]
fn tab_height_must_not_exceed_the_depth() {
    let mut operation = contour();
    operation.tabs = Some(TabSettings::new(4, 6.0, 4.0));
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report.issues.iter().any(|issue| issue.code == "operation/invalid" && issue.message.contains("exceeds the cut depth"))
    );
}

#[test]
fn tab_count_and_width_must_be_positive() {
    for tabs in [TabSettings::new(0, 6.0, 1.0), TabSettings::new(4, 0.0, 1.0), TabSettings::new(4, 6.0, 0.0)] {
        let mut operation = contour();
        operation.tabs = Some(tabs);
        let report = validate(&document(operation));
        assert!(!report.ok, "tabs {tabs:?} must be rejected");
        assert!(report.issues.iter().any(|issue| issue.code == "operation/invalid"));
    }
}

#[test]
fn tabs_are_contour_only() {
    let mut operation = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    operation.x = 10.0;
    operation.y = 10.0;
    operation.width = 40.0;
    operation.height = 30.0;
    operation.tabs = Some(TabSettings::new(2, 5.0, 1.0));
    let report = validate(&document(operation));
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "operation/invalid" && issue.message.contains("only `contour` supports them"))
    );
}

#[test]
fn a_contour_without_tabs_is_unchanged() {
    let plain = contour();
    let json = serde_json::to_value(&plain).expect("serialise");
    assert!(json.get("tabs").is_none());

    let text = serde_json::to_string(&document(plain)).expect("serialise document");
    assert!(!text.contains("tabs"));
    let restored: CamDocument = serde_json::from_str(&text).expect("deserialise");
    assert!(restored.operations[0].tabs.is_none());
    assert_eq!(serde_json::to_string(&restored).expect("reserialise"), text);

    let report = generate_toolpaths(&restored).expect("valid plain contour");
    let cuts = report.operations[0].segments.iter().filter(|segment| segment.kind == MoveKind::Cut).count();
    assert_eq!(cuts, 8);
}

#[test]
fn tabs_round_trip_as_camel_case() {
    let operation = with_tabs();
    let json = serde_json::to_value(&operation).expect("serialise");
    assert_eq!(json["tabs"]["count"].as_u64(), Some(4));
    assert_eq!(json["tabs"]["width"].as_f64(), Some(6.0));
    assert_eq!(json["tabs"]["height"].as_f64(), Some(1.0));

    let restored: Operation = serde_json::from_value(json).expect("deserialise");
    assert_eq!(restored.tabs, operation.tabs);
}

#[test]
fn tabs_render_deterministically_in_gcode() {
    let document = document(with_tabs());
    let first = render_gcode(&document).expect("gcode");
    let second = render_gcode(&document).expect("gcode");
    assert_eq!(first, second);
    assert!(first.contains("(tabs 4 x 6 mm wide, 1 mm high)"));
    assert!(first.contains("G0 X22 Y7 Z9.5"));
    assert!(first.contains("G1 X28 Y7 Z8.5 F300"));
}
