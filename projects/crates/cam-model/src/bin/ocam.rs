//! Thin command line over the [`cam_model::OpenCam`] session API.
//!
//! This binary only parses arguments, calls one session operation, prints the
//! result and maps errors to exit codes. Editing, validation, toolpath
//! generation and export live in the library so the native addon and the
//! TypeScript SDK share them.

use std::{collections::HashMap, env, path::Path, process};

use cam_model::{CamError, ExportFormat, Hole, Island, OpenCam, Operation, OperationKind, PathPoint, Stock, TabSettings, Tool};

const USAGE: &str = "\
ocam - OpenCAM 2.5-axis toolpath and G-code generator (millimetres)

Usage:
  ocam create <file.ocam>
  ocam stock <file> --width <w> --depth <d> --height <h>
  ocam tool add <file> --id <id> --diameter <d> --flutes <n> --spindle <rpm> --feed <f>
                     [--plunge <f>] [--number <n>] [--name <name>]
  ocam tool remove <file> --id <id>
  ocam operation add <file> --id <id> --kind face|contour|pocket|drill|engrave --tool <id>
                     --depth <d> [--stepdown <d>] [--stepover <d>] [--name <name>]
                     [--strategy raster|spiral]
                     [--shape rect|circle]
                     [--x <x>] [--y <y>] [--width <w>] [--height <h>]
                     [--holes <x,y;x,y>] [--peck <d>] [--island <x,y,w,h>]...
                     [--path <x,y;x,y>] [--tabs <count,width,height>] [--closed]
  ocam operation remove <file> --id <id>
  ocam validate <file>
  ocam toolpaths <file> [--out report.json]
  ocam gcode <file> [--out program.nc]
  ocam export <file> --format gcode|json|csv|md --out <path>
  ocam report <file>
  ocam capabilities
  ocam version

Exit codes: 0 success, 1 input/validation error, 2 capability not implemented
";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            process::exit(error.exit_code());
        }
    }
}

fn run(args: &[String]) -> Result<i32, CamError> {
    let Some(command) = args.first().map(String::as_str)
    else {
        println!("{USAGE}");
        return Ok(0);
    };
    match command {
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(0)
        }
        "version" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        "capabilities" => {
            println!("{}", OpenCam::new().capabilities());
            Ok(0)
        }
        "create" => {
            let file = positional(args, 1, "create <file>")?;
            let mut session = OpenCam::new();
            print_json(&session.create(Path::new(file))?)
        }
        "stock" => {
            let file = positional(args, 1, "stock <file>")?;
            let values = Options::parse(&args[2..], &["--width", "--depth", "--height"], &[])?;
            let stock = Stock::new(values.number("--width")?, values.number("--depth")?, values.number("--height")?);
            print_json(&session(file)?.set_stock(stock)?)
        }
        "tool" => tool_command(args),
        "operation" => operation_command(args),
        "validate" => {
            let file = positional(args, 1, "validate <file>")?;
            print_json(&session(file)?.validate()?)
        }
        "toolpaths" => {
            let file = positional(args, 1, "toolpaths <file>")?;
            let values = Options::parse(&args[2..], &["--out"], &[])?;
            let session = session(file)?;
            let report = session.toolpaths()?;
            if let Some(target) = values.value("--out") {
                cam_model::write_atomic(
                    Path::new(target),
                    &serde_json::to_string_pretty(&report)
                        .map_err(|error| CamError::new("document/serialize", error.to_string()))?,
                )?;
            }
            print_json(&report)
        }
        "gcode" => {
            let file = positional(args, 1, "gcode <file>")?;
            let values = Options::parse(&args[2..], &["--out"], &[])?;
            let session = session(file)?;
            let program = session.gcode()?;
            if let Some(target) = values.value("--out") {
                cam_model::write_atomic(Path::new(target), &program)?;
            }
            print!("{program}");
            Ok(0)
        }
        "export" => {
            let file = positional(args, 1, "export <file> --format <format> --out <path>")?;
            let values = Options::parse(&args[2..], &["--format", "--out"], &[])?;
            let format: ExportFormat = values.value("--format").unwrap_or("gcode").parse()?;
            let target = values.require("--out")?;
            let session = session(file)?;
            print_json(&session.export(format, Path::new(target))?)
        }
        "report" => {
            let file = positional(args, 1, "report <file>")?;
            let session = session(file)?;
            print_json(&session.report()?)
        }
        other => Err(CamError::new("cli/unknown-command", format!("unknown command `{other}`"))),
    }
}

fn tool_command(args: &[String]) -> Result<i32, CamError> {
    let action = args.get(1).map(String::as_str).unwrap_or("");
    match action {
        "add" => {
            let file = positional(args, 2, "tool add <file>")?;
            let values = Options::parse(
                &args[3..],
                &["--id", "--diameter", "--flutes", "--spindle", "--feed", "--plunge", "--number", "--name"],
                &[],
            )?;
            let id = values.require("--id")?;
            let tool = Tool {
                name: values.value("--name").unwrap_or(id).to_string(),
                id: id.to_string(),
                number: values.integer("--number")?.unwrap_or(1),
                diameter: values.number("--diameter")?,
                flutes: values.integer("--flutes")?.unwrap_or(2),
                spindle: values.number("--spindle")?,
                feed: values.number("--feed")?,
                plunge_feed: values.number("--plunge").unwrap_or(values.number("--feed")? / 3.0),
            };
            print_json(&session(file)?.add_tool(tool)?)
        }
        "remove" => {
            let file = positional(args, 2, "tool remove <file>")?;
            let values = Options::parse(&args[3..], &["--id"], &[])?;
            print_json(&session(file)?.remove_tool(values.require("--id")?)?)
        }
        _ => Err(CamError::new("cli/missing-argument", "usage: ocam tool add|remove <file> ...")),
    }
}

fn operation_command(args: &[String]) -> Result<i32, CamError> {
    let action = args.get(1).map(String::as_str).unwrap_or("");
    match action {
        "add" => {
            let file = positional(args, 2, "operation add <file>")?;
            let values = Options::parse(
                &args[3..],
                &[
                    "--id",
                    "--kind",
                    "--tool",
                    "--depth",
                    "--stepdown",
                    "--stepover",
                    "--name",
                    "--strategy",
                    "--shape",
                    "--x",
                    "--y",
                    "--width",
                    "--height",
                    "--holes",
                    "--peck",
                    "--island",
                    "--path",
                    "--tabs",
                ],
                &["--closed"],
            )?;
            let id = values.require("--id")?;
            let kind: OperationKind = values.require("--kind")?.parse()?;
            let tool = values.require("--tool")?;
            let depth = values.number("--depth")?;
            let stepdown = values.number("--stepdown").unwrap_or(depth);
            let mut operation = Operation::new(id, kind, tool, depth, stepdown);
            operation.name = values.value("--name").unwrap_or(id).to_string();
            operation.stepover = values.optional_number("--stepover")?;
            operation.strategy = values.value("--strategy").map(str::to_string);
            operation.shape = values.value("--shape").map(str::to_string);
            operation.x = values.number_or("--x", 0.0)?;
            operation.y = values.number_or("--y", 0.0)?;
            operation.width = values.number_or("--width", 0.0)?;
            operation.height = values.number_or("--height", 0.0)?;
            operation.peck = values.optional_number("--peck")?;
            if let Some(raw) = values.value("--holes") {
                operation.holes = parse_holes(raw)?;
            }
            if let Some(raw) = values.value("--path") {
                operation.path = parse_path(raw)?;
            }
            if let Some(raw) = values.value("--tabs") {
                operation.tabs = Some(parse_tabs(raw)?);
            }
            operation.closed = values.value("--closed").is_some();
            operation.islands = values.values("--island").iter().map(|raw| parse_island(raw)).collect::<Result<_, _>>()?;
            print_json(&session(file)?.add_operation(operation)?)
        }
        "remove" => {
            let file = positional(args, 2, "operation remove <file>")?;
            let values = Options::parse(&args[3..], &["--id"], &[])?;
            print_json(&session(file)?.remove_operation(values.require("--id")?)?)
        }
        _ => Err(CamError::new("cli/missing-argument", "usage: ocam operation add|remove <file> ...")),
    }
}

fn parse_holes(raw: &str) -> Result<Vec<Hole>, CamError> {
    let mut holes = Vec::new();
    for entry in raw.split(';').filter(|entry| !entry.trim().is_empty()) {
        let mut parts = entry.split(',');
        let x = parts.next().and_then(|value| value.trim().parse::<f64>().ok());
        let y = parts.next().and_then(|value| value.trim().parse::<f64>().ok());
        match (x, y) {
            (Some(x), Some(y)) if parts.next().is_none() => holes.push(Hole::new(x, y)),
            _ => {
                return Err(CamError::new("cli/invalid-option", format!("hole `{entry}` must be a `x,y` pair")));
            }
        }
    }
    Ok(holes)
}

fn parse_path(raw: &str) -> Result<Vec<PathPoint>, CamError> {
    let mut points = Vec::new();
    for entry in raw.split(';').filter(|entry| !entry.trim().is_empty()) {
        let mut parts = entry.split(',');
        let x = parts.next().and_then(|value| value.trim().parse::<f64>().ok());
        let y = parts.next().and_then(|value| value.trim().parse::<f64>().ok());
        match (x, y) {
            (Some(x), Some(y)) if parts.next().is_none() => points.push(PathPoint::new(x, y)),
            _ => {
                return Err(CamError::new("cli/invalid-option", format!("path point `{entry}` must be an `x,y` pair")));
            }
        }
    }
    Ok(points)
}

fn parse_island(raw: &str) -> Result<Island, CamError> {
    let parts: Vec<f64> =
        raw.split(',').map(|value| value.trim().parse::<f64>()).collect::<Result<Vec<_>, _>>().map_err(|_| {
            CamError::new("cli/invalid-option", format!("island `{raw}` must be an `x,y,width,height` quadruple"))
        })?;
    match parts.as_slice() {
        [x, y, width, height] => Ok(Island::new(*x, *y, *width, *height)),
        _ => Err(CamError::new("cli/invalid-option", format!("island `{raw}` must be an `x,y,width,height` quadruple"))),
    }
}

fn parse_tabs(raw: &str) -> Result<TabSettings, CamError> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(CamError::new("cli/invalid-option", format!("tabs `{raw}` must be a `count,width,height` triple")));
    }
    let count = parts[0].parse::<u32>().map_err(|_| {
        CamError::new("cli/invalid-option", format!("tabs count `{}` must be a non-negative integer", parts[0]))
    })?;
    let width = parse_number("--tabs width", parts[1])?;
    let height = parse_number("--tabs height", parts[2])?;
    Ok(TabSettings::new(count, width, height))
}

fn session(file: &str) -> Result<OpenCam, CamError> {
    let mut session = OpenCam::new();
    session.open(Path::new(file))?;
    Ok(session)
}

fn positional<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str, CamError> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| CamError::new("cli/missing-argument", format!("missing argument; usage: ocam {usage}")))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<i32, CamError> {
    let text = serde_json::to_string_pretty(value).map_err(|error| CamError::new("document/serialize", error.to_string()))?;
    println!("{text}");
    Ok(0)
}

struct Options {
    map: HashMap<String, Vec<String>>,
}

impl Options {
    fn parse(args: &[String], values: &[&str], flags: &[&str]) -> Result<Self, CamError> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut index = 0;
        while index < args.len() {
            let argument = args[index].as_str();
            if flags.contains(&argument) {
                map.entry(argument.to_string()).or_default().push(String::new());
                index += 1;
            }
            else if values.contains(&argument) {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| CamError::new("cli/missing-value", format!("option `{argument}` needs a value")))?;
                map.entry(argument.to_string()).or_default().push(value.clone());
                index += 2;
            }
            else {
                return Err(CamError::new("cli/unknown-option", format!("unknown option `{argument}`")));
            }
        }
        Ok(Self { map })
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.map.get(name).and_then(|entries| entries.last()).map(String::as_str)
    }

    fn values(&self, name: &str) -> &[String] {
        self.map.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    fn require(&self, name: &str) -> Result<&str, CamError> {
        self.value(name).ok_or_else(|| CamError::new("cli/missing-option", format!("option `{name}` is required")))
    }

    fn number(&self, name: &str) -> Result<f64, CamError> {
        let raw = self.require(name)?;
        parse_number(name, raw)
    }

    fn optional_number(&self, name: &str) -> Result<Option<f64>, CamError> {
        self.value(name).map(|raw| parse_number(name, raw)).transpose()
    }

    fn number_or(&self, name: &str, fallback: f64) -> Result<f64, CamError> {
        Ok(self.optional_number(name)?.unwrap_or(fallback))
    }

    fn integer(&self, name: &str) -> Result<Option<u32>, CamError> {
        match self.value(name) {
            None => Ok(None),
            Some(raw) => raw
                .parse::<u32>()
                .map(Some)
                .map_err(|_| CamError::new("cli/invalid-number", format!("option `{name}` must be a non-negative integer"))),
        }
    }
}

fn parse_number(name: &str, raw: &str) -> Result<f64, CamError> {
    raw.parse::<f64>().map_err(|_| CamError::new("cli/invalid-number", format!("option `{name}` is not a number: `{raw}`")))
}
