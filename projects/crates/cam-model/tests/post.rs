use cam_model::{CamDocument, Hole, Operation, OperationKind, PostOptions, Stock, Tool, render_gcode, render_gcode_with_post};

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
fn default_post_options_keep_the_program_byte_identical() {
    let expected = render_gcode(&document()).expect("gcode");
    let actual = render_gcode_with_post(&document(), &PostOptions::default()).expect("gcode");
    assert_eq!(expected, actual);
}

#[test]
fn an_empty_request_uses_the_defaults() {
    let post: PostOptions = serde_json::from_str("{}").expect("post");
    assert_eq!(post, PostOptions::default());
    assert_eq!(post.line_increment, 10);
    assert_eq!(post.coolant, "off");
    assert!(!post.line_numbers);
    assert_eq!(post.program_number, None);
}

#[test]
fn program_number_is_emitted_right_after_the_header() {
    let post = PostOptions { program_number: Some(1001), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    let lines: Vec<&str> = program.lines().collect();
    assert_eq!(lines[0], "%");
    assert_eq!(lines[1], "O1001");
    assert_eq!(lines.last().copied(), Some("%"));
}

#[test]
fn line_numbers_prefix_every_interior_line_only() {
    let post = PostOptions { line_numbers: true, ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    let lines: Vec<&str> = program.lines().collect();
    assert_eq!(lines[0], "%");
    assert_eq!(lines.last().copied(), Some("%"));
    assert!(lines[1].starts_with("N10 "), "first interior line was `{}`", lines[1]);
    assert!(lines[2].starts_with("N20 "), "second interior line was `{}`", lines[2]);
    for line in &lines[1..lines.len() - 1] {
        assert!(line.starts_with('N'), "`{line}` is not numbered");
    }
    for line in &lines[1..lines.len() - 1] {
        assert!(!line.starts_with("%"), "a `%` line was numbered");
    }
}

#[test]
fn a_custom_increment_renumbers_every_line() {
    let post = PostOptions { line_numbers: true, line_increment: 5, program_number: Some(7), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    let lines: Vec<&str> = program.lines().collect();
    assert_eq!(lines[0], "%");
    assert_eq!(lines[1], "N5 O7");
    assert!(lines[2].starts_with("N10 "), "second interior line was `{}`", lines[2]);
}

#[test]
fn flood_coolant_wraps_every_operation() {
    let post = PostOptions { coolant: "flood".to_string(), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    assert_eq!(program.matches("M8").count(), 2);
    assert_eq!(program.matches("M9").count(), 2);
    let lines: Vec<&str> = program.lines().collect();
    let spindle = lines.iter().position(|line| line.starts_with("S12000 M3")).expect("spindle line");
    assert_eq!(lines[spindle + 1], "M8");
    let stop = lines.iter().position(|line| *line == "M5").expect("spindle stop");
    assert_eq!(lines[stop - 1], "M9");
}

#[test]
fn mist_coolant_emits_m7() {
    let post = PostOptions { coolant: "mist".to_string(), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    assert_eq!(program.matches("M7").count(), 2);
    assert!(!program.contains("M8"));
    assert_eq!(program.matches("M9").count(), 2);
}

#[test]
fn coolant_off_emits_no_coolant_line() {
    let program = render_gcode(&document()).expect("gcode");
    assert!(!program.contains("M8"));
    assert!(!program.contains("M7"));
    assert!(!program.contains("M9"));
}

#[test]
fn invalid_post_options_report_post_invalid() {
    for post in [
        PostOptions { line_increment: 0, ..PostOptions::default() },
        PostOptions { program_number: Some(0), ..PostOptions::default() },
        PostOptions { coolant: "high-pressure".to_string(), ..PostOptions::default() },
    ] {
        let error = render_gcode_with_post(&document(), &post).expect_err("invalid options");
        assert_eq!(error.code, "post/invalid");
    }
}

#[test]
fn post_options_do_not_change_a_single_move() {
    let plain = render_gcode(&document()).expect("gcode");
    let post = PostOptions {
        post: "generic".to_string(),
        program_number: Some(12),
        line_numbers: true,
        line_increment: 10,
        coolant: "flood".to_string(),
    };
    let decorated = render_gcode_with_post(&document(), &post).expect("gcode");
    let body = |text: &str| -> Vec<String> {
        text.lines()
            .map(|line| line.trim_start_matches(|character: char| character == 'N' || character.is_ascii_digit()).trim_start())
            .filter(|line| !line.starts_with('O'))
            .filter(|line| !matches!(*line, "M8" | "M9"))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(body(&plain), body(&decorated));
}

#[test]
fn post_output_is_deterministic() {
    let post = PostOptions {
        post: "generic".to_string(),
        program_number: Some(42),
        line_numbers: true,
        line_increment: 2,
        coolant: "mist".to_string(),
    };
    let first = render_gcode_with_post(&document(), &post).expect("gcode");
    let second = render_gcode_with_post(&document(), &post).expect("gcode");
    assert_eq!(first, second);
}

#[test]
fn grbl_omits_the_program_wrappers_and_tool_change() {
    let post = PostOptions { post: "grbl".to_string(), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    assert!(!program.starts_with('%'), "GRBL has no `%` delimiter");
    assert!(!program.ends_with("%\n"), "GRBL has no trailing `%`");
    assert!(!program.contains("\nO"), "GRBL has no program number");
    assert!(!program.contains("M6"), "GRBL has no automatic tool change");
    assert!(program.contains("(tool t6 T3 diameter 6)"));
    assert!(program.contains("G21 G90 G17\n"));
    assert!(!program.contains("G94"));
    assert_eq!(program.matches("S12000 M3").count(), 2, "every operation still starts the spindle");
    assert!(program.ends_with("M30\n"));
}

#[test]
fn grbl_expands_canned_cycles_into_moves() {
    let post = PostOptions { post: "grbl".to_string(), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    assert!(!program.contains("G81"), "no canned cycle survives");
    assert!(!program.contains("G80"), "no cycle is cancelled");
    assert!(program.contains("G0 X10 Y10 Z13"), "rapid to the hole at the clearance plane");
    assert!(program.contains("G1 Z4 F300"), "plunge to the hole bottom at the plunge feed");
}

#[test]
fn grbl_expands_peck_and_dwell() {
    let mut peck = document();
    peck.operations[0].peck = Some(2.0);
    let post = PostOptions { post: "grbl".to_string(), ..PostOptions::default() };
    let program = render_gcode_with_post(&peck, &post).expect("gcode");
    assert_eq!(program.matches("G1 Z").count(), 5, "13 → 11 → 9 → 7 → 5 → 4 mm");
    assert!(program.contains("G1 Z11 F300"));
    assert!(program.contains("G1 Z9 F300"));
    assert!(program.contains("G1 Z4 F300"));
    assert!(program.contains("G0 Z13"));

    let mut dwell = document();
    dwell.operations[0].dwell = Some(1.5);
    let program = render_gcode_with_post(&dwell, &post).expect("gcode");
    assert!(program.contains("G1 Z4 F300"));
    assert!(program.contains("G4 P1.5"), "the dwell is kept as a pause at the bottom");
    assert!(!program.contains("G82"));
}

#[test]
fn mach3_keeps_cycles_and_adds_a_safety_block() {
    let post = PostOptions { post: "mach3".to_string(), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    assert!(program.starts_with("%\n"));
    assert!(program.ends_with("%\n"));
    assert!(program.contains("(Post: Mach3, absolute, XY plane)"));
    assert!(program.contains("G21 G90 G17 G94 G40 G49 G80"));
    assert!(program.contains("T3 M6"));
    assert!(program.contains("G81 X10 Y10 Z4 R13 F300"));
    assert!(program.contains("G80"));
}

#[test]
fn fanuc_pads_the_program_number_and_requests_initial_level_retract() {
    let post = PostOptions { post: "fanuc".to_string(), program_number: Some(7), ..PostOptions::default() };
    let program = render_gcode_with_post(&document(), &post).expect("gcode");
    assert_eq!(program.lines().nth(1), Some("O0007"), "a Fanuc program number is four digits");
    assert!(program.contains("(Post: Fanuc, absolute, XY plane)"));
    assert!(program.contains("G21 G90 G17 G94 G40 G49 G80"));
    assert!(program.contains("G98 G81 X10 Y10 Z4 R13 F300"));
    assert!(program.contains("T3 M6"));

    let error = render_gcode_with_post(&document(), &PostOptions { post: "fanuc".to_string(), ..PostOptions::default() })
        .expect_err("a Fanuc program needs an O number");
    assert_eq!(error.code, "post/invalid");
}

#[test]
fn dialects_reject_options_they_cannot_express() {
    for post in [
        PostOptions { post: "grbl".to_string(), program_number: Some(1), ..PostOptions::default() },
        PostOptions { post: "grbl".to_string(), line_numbers: true, ..PostOptions::default() },
        PostOptions { post: "mach3".to_string(), program_number: Some(1), ..PostOptions::default() },
        PostOptions { post: "fanuc".to_string(), program_number: Some(10000), ..PostOptions::default() },
        PostOptions { post: "haas".to_string(), ..PostOptions::default() },
    ] {
        let error = render_gcode_with_post(&document(), &post).expect_err("invalid post");
        assert_eq!(error.code, "post/invalid", "post `{}` should be rejected", post.post);
    }
}

#[test]
fn mach3_changes_no_coordinate() {
    let plain = render_gcode(&document()).expect("gcode");
    let mach3 = render_gcode_with_post(&document(), &PostOptions { post: "mach3".to_string(), ..PostOptions::default() })
        .expect("gcode");
    let motion = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("G0 ") || line.starts_with("G1 ") || line.starts_with("G8"))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(motion(&plain), motion(&mach3));
}

#[test]
fn each_dialect_is_deterministic() {
    for post in ["generic", "grbl", "mach3", "fanuc"] {
        let options = PostOptions {
            post: post.to_string(),
            program_number: if post == "fanuc" { Some(1) } else { None },
            ..PostOptions::default()
        };
        let first = render_gcode_with_post(&document(), &options).expect("gcode");
        let second = render_gcode_with_post(&document(), &options).expect("gcode");
        assert_eq!(first, second, "post `{post}` is not deterministic");
    }
}
