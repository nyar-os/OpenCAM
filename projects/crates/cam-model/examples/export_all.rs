//! Temporary driver: builds a demo part and exports every supported format.

use std::path::Path;

use cam_model::{ExportFormat, Hole, OpenCam, Operation, OperationKind, Stock, Tool};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "export-all".into());
    let out = Path::new(&out);
    std::fs::create_dir_all(out)?;

    let path = out.join("demo.ocam");
    let _ = std::fs::remove_file(&path);
    let mut session = OpenCam::new();
    session.create(&path)?;
    session.set_stock(Stock::new(120.0, 80.0, 12.0))?;
    session.add_tool(Tool {
        id: "t6".into(),
        name: "6 mm flat".into(),
        number: 1,
        diameter: 6.0,
        flutes: 2,
        spindle: 12000.0,
        feed: 1200.0,
        plunge_feed: 300.0,
    })?;
    session.add_tool(Tool {
        id: "t3".into(),
        name: "3 mm flat".into(),
        number: 2,
        diameter: 3.0,
        flutes: 2,
        spindle: 18000.0,
        feed: 900.0,
        plunge_feed: 250.0,
    })?;

    let mut facing = Operation::new("face", OperationKind::Face, "t6", 1.0, 0.5);
    facing.x = 0.0;
    facing.y = 0.0;
    facing.width = 120.0;
    facing.height = 80.0;
    session.add_operation(facing)?;

    let mut pocket = Operation::new("pocket", OperationKind::Pocket, "t6", 2.0, 1.0);
    pocket.x = 10.0;
    pocket.y = 10.0;
    pocket.width = 40.0;
    pocket.height = 30.0;
    session.add_operation(pocket)?;

    let mut contour = Operation::new("outline", OperationKind::Contour, "t3", 12.0, 4.0);
    contour.x = 70.0;
    contour.y = 20.0;
    contour.width = 40.0;
    contour.height = 40.0;
    session.add_operation(contour)?;

    let mut drill = Operation::new("holes", OperationKind::Drill, "t6", 12.0, 6.0);
    drill.holes = vec![Hole::new(25.0, 25.0), Hole::new(45.0, 25.0), Hole::new(25.0, 45.0), Hole::new(45.0, 45.0)];
    session.add_operation(drill)?;

    let mut engrave = Operation::new("logo", OperationKind::Engrave, "t3", 0.5, 0.5);
    engrave.path = vec![(80.0, 30.0), (100.0, 30.0), (90.0, 50.0), (80.0, 30.0)]
        .into_iter()
        .map(|(x, y)| cam_model::PathPoint { x, y })
        .collect();
    session.add_operation(engrave)?;

    for format in
        [ExportFormat::Gcode, ExportFormat::Csv, ExportFormat::Md, ExportFormat::Json, ExportFormat::Svg, ExportFormat::Pdf]
    {
        let extension = format.as_str();
        let target = out.join(format!("demo.{extension}"));
        match session.export(format, &target) {
            Ok(report) => println!("OK {}: {} bytes -> {}", report.format, report.bytes, report.path),
            Err(err) => println!("ERR {extension}: {}", err.message),
        }
    }
    Ok(())
}
