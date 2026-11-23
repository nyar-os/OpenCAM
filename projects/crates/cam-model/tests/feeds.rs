use cam_model::{FeedsRequest, MATERIALS, OpenCam, suggest_feeds};

fn request(material: &str, diameter: f64, flutes: Option<u32>) -> FeedsRequest {
    FeedsRequest { material: material.to_string(), diameter, flutes }
}

#[test]
fn aluminium_six_millimetre_two_flute_uses_the_documented_formula() {
    let report = suggest_feeds(&request("aluminium", 6.0, Some(2))).expect("feeds");
    assert_eq!(report.format, "open-cam-feeds");
    assert_eq!(report.version, 1);
    assert_eq!(report.units, "mm");
    assert_eq!(report.material, "aluminium");
    assert_eq!(report.diameter, 6.0);
    assert_eq!(report.flutes, 2);
    assert_eq!(report.surface_speed, 300.0);
    assert_eq!(report.chip_load, 0.02);
    assert_eq!(report.spindle, 15900.0);
    assert_eq!(report.feed, 640.0);
    assert_eq!(report.plunge_feed, 190.0);
}

#[test]
fn the_american_spelling_is_an_alias_of_the_canonical_name() {
    let canonical = suggest_feeds(&request("aluminium", 6.0, Some(2))).expect("feeds");
    let alias = suggest_feeds(&request("aluminum", 6.0, Some(2))).expect("feeds");
    assert_eq!(canonical, alias);
    assert_eq!(alias.material, "aluminium");
}

#[test]
fn steel_selects_the_diameter_band_and_the_flute_count() {
    let report = suggest_feeds(&request("steel", 10.0, Some(4))).expect("feeds");
    assert_eq!(report.surface_speed, 120.0);
    assert_eq!(report.chip_load, 0.025);
    assert_eq!(report.spindle, 3800.0);
    assert_eq!(report.feed, 380.0);
    assert_eq!(report.plunge_feed, 110.0);
}

#[test]
fn an_omitted_flute_count_defaults_to_two() {
    let report = suggest_feeds(&request("brass", 5.0, None)).expect("feeds");
    assert_eq!(report.flutes, 2);
    assert_eq!(report.chip_load, 0.016);
    assert_eq!(report.spindle, 12700.0);
    assert_eq!(report.feed, 410.0);
}

#[test]
fn the_reported_rates_are_floored_for_a_tiny_tool() {
    let report = suggest_feeds(&request("stainless", 0.1, Some(1))).expect("feeds");
    assert_eq!(report.spindle, 24000.0);
    assert_eq!(report.chip_load, 0.004);
    assert_eq!(report.feed, 100.0);
    assert_eq!(report.plunge_feed, 30.0);
}

#[test]
fn an_unknown_material_lists_every_supported_name() {
    let error = suggest_feeds(&request("titanium", 6.0, Some(2))).expect_err("unknown material");
    assert_eq!(error.code, "feeds/unknown-material");
    for name in MATERIALS {
        assert!(error.message.contains(name), "message must list `{name}`: {}", error.message);
    }
}

#[test]
fn an_invalid_diameter_or_flute_count_reports_feeds_invalid() {
    for diameter in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let error = suggest_feeds(&request("steel", diameter, Some(2))).expect_err("diameter");
        assert_eq!(error.code, "feeds/invalid", "diameter {diameter}");
    }
    let error = suggest_feeds(&request("steel", 6.0, Some(0))).expect_err("flutes");
    assert_eq!(error.code, "feeds/invalid");
}

#[test]
fn the_session_reports_the_same_recommendation() {
    let session = OpenCam::new();
    let report = session.suggest_feeds(&request("wood", 3.0, Some(1))).expect("feeds");
    assert_eq!(report.surface_speed, 500.0);
    assert_eq!(report.chip_load, 0.02);
    assert_eq!(report.spindle, 24000.0);
    assert_eq!(report.feed, 480.0);
    assert_eq!(report.plunge_feed, 140.0);
}

#[test]
fn the_request_deserializes_with_an_absent_flute_count_and_round_trips() {
    let parsed: FeedsRequest = serde_json::from_str(r#"{"material":"plastic","diameter":8}"#).expect("parse");
    assert_eq!(parsed.flutes, None);
    let report = suggest_feeds(&parsed).expect("feeds");
    assert_eq!(report.flutes, 2);

    let text = serde_json::to_string(&parsed).expect("json");
    assert!(!text.contains("flutes"), "absent flutes must be omitted: {text}");
    let again: FeedsRequest = serde_json::from_str(&text).expect("reparse");
    assert_eq!(again, parsed);
}

#[test]
fn the_recommendation_is_deterministic() {
    let first = suggest_feeds(&request("aluminium", 8.0, Some(3))).expect("feeds");
    let second = suggest_feeds(&request("aluminium", 8.0, Some(3))).expect("feeds");
    assert_eq!(serde_json::to_string(&first).expect("json"), serde_json::to_string(&second).expect("json"));
}
