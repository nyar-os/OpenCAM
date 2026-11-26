---
name: open-cam
description: Prepare 2.5-axis milling jobs with OpenCAM. Use when a task involves an `.ocam` document, stock and tools, face, contour, pocket, drill or engrave operations, toolpaths, cycle-time estimates, G-code posts, DXF or G-code import, or toolpath and operation drawings.
---

# open-cam

## Status

The 2.5-axis core is executable. The CLI and SDK create, import, edit and reopen
one `.ocam` document, generate deterministic toolpaths, estimate cycle time,
write G-code for four posts, and export tables and drawings. Units are
millimetres and never converted.

Operations are `face`, `contour`, `pocket`, `drill` and `engrave`; each carries
its own geometry, depth, stepdown and tool. Operation order is execution order.
`export --format step` is declared but unimplemented and exits with code `2`.
There is no four-axis or simultaneous five-axis motion, no collision or material
simulation, no rest machining and no automatic feature recognition. `import`
reads only the documented ASCII DXF and G-code subsets.

## Executable commands

```bash
open-cam capabilities
open-cam create <file.ocam>
open-cam import dxf <file.ocam> --from <drawing.dxf> [--depth <mm>] [--stepdown <mm>] [--tolerance <mm>]
open-cam import gcode <file.ocam> --from <program.nc> [--depth <mm>] [--stepdown <mm>] [--tolerance <mm>]
open-cam validate <file>
open-cam report <file>

open-cam stock <file> --width <w> --depth <d> --height <h>
open-cam tool add <file> --id <id> --diameter <d> --spindle <rpm> --feed <f> [--flutes <n>] [--plunge <f>] [--number <n>] [--name <name>]
open-cam tool remove <file> --id <id>
open-cam tool list <file>
open-cam tool export <file> --out <library.json>
open-cam tool import <file> --from <library.json>

open-cam operation add <file> --id <id> --kind <kind> --tool <id> --depth <d> [--stepdown <d>] [--stepover <d>] [--ramp <mm>] [--strategy raster|spiral] [--shape rect|circle] [--arcs linear|arc] [--x <x>] [--y <y>] [--width <w>] [--height <h>] [--holes <holes>] [--pattern <layout>] [--peck <d>] [--dwell <seconds>] [--island <island>] [--path <path>] [--tabs <count,width,height>] [--lead <radius,cw|ccw>] [--closed] [--finish]
open-cam operation list <file>
open-cam operation update <file> --id <id> [--tool <id>] [--depth <mm>] [--stepdown <mm>] [--stepover <mm>] [--name <name>] [--peck <mm>] [--dwell <seconds>]
open-cam operation delete <file> --id <id>
open-cam operation duplicate <file> --id <id> [--new-id <id>]
open-cam operation reorder <file> --id <id> --to <index>

open-cam toolpaths <file> [--out <file>]
open-cam estimate <file> [--rapid-feed <mm/min>] [--tool-change <seconds>] [--out <file>]
open-cam feeds --material <name> --diameter <mm> [--flutes <n>]
open-cam gcode <file> [--out <file>] [--post generic|grbl|mach3|fanuc] [--program-number <n>] [--line-numbers] [--line-increment <n>] [--coolant off|flood|mist]
open-cam sequence <file> [--apply]
open-cam export <file> --format gcode|json|csv|md|svg|pdf|dxf [--kind <kind>] [--ops <list>] [--out <path>]
```

Exit codes: `0` success, `1` invalid input or an unmachineable document, `2` the
capability is not implemented.

## Preflight

1. Run `open-cam capabilities` and confirm the operation kind, post or export the
   task needs is supported. A command name or a file extension is not a
   capability.
2. Run `open-cam validate <file>` and `open-cam report <file>` before generating
   G-code.
3. Confirm the stock and every tool actually exist; an operation is validated
   against the stock when it is added, but a later `stock` change can invalidate
   a document, and `validate` reports it.

## Routes

| Route | Intent | Minimum evidence |
|-------|--------|------------------|
| set up | Create a document, stock and tool table | `open-cam validate <file>` and `open-cam tool list <file>` |
| machine | Add and order operations | `open-cam toolpaths <file> --out <file>` |
| deliver | Write G-code and a drawing | the G-code file, its line count and the export report |
| review | Check depth, feeds and timing | the validate, toolpath and estimate reports |

## Geometry and stock

- `create` writes a 100 × 100 × 10 mm stock; `stock` replaces it. All three
  values must be positive.
- `operation add` validates the operation against the stock before saving, so a
  geometry outside the stock reports `path/outside-stock` and writes nothing.
- `contour` mills outside the outline by the cutter radius; `pocket` clears
  inside it; `engrave` follows the polyline with no cutter-radius compensation;
  `drill` services an explicit `--holes` list or a generated `--pattern`.
- `--pattern grid,rows,columns,spacingX,spacingY` and
  `--pattern circular,count,radius` replace `--holes`. Two layouts exist and no
  other value is accepted.
- `--stepdown` defaults to `--depth`. `--stepover` defaults to 70 % of the tool
  diameter for facing and 60 % for pocketing.
- `--ramp` is a horizontal length in millimetres and applies to `contour`,
  `pocket` and `engrave` only. `--lead` applies to `contour` and excludes
  `--ramp`. `--tabs` and `--closed` apply to `contour` and `engrave`
  respectively.
- Spiral pockets reject islands (`operation/strategy-island`) and circular
  pockets reject both `--strategy` and `--island`.

## Tools and feeds

- `tool add` requires a positive diameter, spindle speed and feed; `--plunge`
  defaults to `feed / 3` and flutes to `2`.
- A tool referenced by an operation cannot be removed (`tool/in-use`).
- `open-cam feeds --material <name> --diameter <mm>` prints a recommendation for
  `aluminium`, `brass`, `steel`, `stainless`, `plastic` or `wood`; it reads no
  document. The values are a table lookup, not a machine calibration.

## Toolpaths, time and G-code

- `open-cam toolpaths <file> --out <file>` reports every move, the cut and rapid
  length and the kinematic estimate for one operation. Arc moves carry a centre
  and a direction; linear moves do not.
- `estimate` splits cutting, plunge, retract, traverse and tool-change seconds;
  the default rapid feed is 8000 mm/min and the default tool change 3 seconds.
- `open-cam gcode <file> --out <file>` writes the generic program. `--post`
  changes only the header, program number, tool-change wording and canned-cycle
  spelling; coordinates, feeds and moves never change.
- `--program-number` is required by `fanuc` and rejected by `grbl` and `mach3`;
  `--line-numbers` is rejected by `grbl`. Units `G21` and absolute mode `G90`
  are fixed in every post.
- `sequence` groups operations by tool; `open-cam sequence <file> --apply` writes
  only when the order actually changes.

## Exports

- `export --format gcode|json|csv|md|svg|pdf|dxf` with `--out <path>`. SVG and
  PDF may be printed to stdout instead.
- `--kind` selects the sheet for SVG/PDF: `toolpath`, `tool-list`,
  `stock-setup`, `depth-map`, `operation-sheet` or `stock-removal`.
- `--ops` draws only the listed 1-based operations and the legend follows the
  selection.
- SVG is deterministic and keeps Unicode; PDF drops non-ASCII characters and
  reports the count as `dropped`. Two exports of one document are byte-identical.
- The `dxf` export writes nominal geometry, not the toolpath: stock on `STOCK`,
  engrave paths, contour and pocket outlines, islands and drill circles.

## Hard rules

- Never claim collision checking, material simulation, rest machining or a
  step export; none exist in this build.
- Never present a feeds recommendation or cycle-time estimate as a measured
  machine result.
- Never present G-code from the `generic`, `grbl`, `mach3` or `fanuc` post as a
  verified machine program for another controller.
- Keep one model: toolpaths, G-code, drawings and estimates must come from the
  same document revision.

## Safety

- Confirm the target path with the user before any command that creates or
  rewrites a document or a G-code file; every editing command rewrites the
  document in place.
- Never overwrite an input document unless the task asks for an in-place edit.
- G-code drives a physical machine: state the tool, depth, stock origin, post and
  export path, and say explicitly which checks were not performed.
