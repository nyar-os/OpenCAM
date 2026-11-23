# @wae/open-cam

Deterministic 2.5-axis toolpaths and G-code for a rectangular stock, exposed as a
CLI and a typed SDK over one native core.

## Install

```shell
pnpm add @wae/open-cam
```

The package resolves a per-platform native addon. If the addon is not installed,
calls report `capability/not-implemented` instead of returning a fabricated
result.

## CLI

```text
open-cam create <file.ocam>
open-cam stock <file> --width <w> --depth <d> --height <h>
open-cam tool add <file> --id <id> --diameter <d> --spindle <rpm> --feed <f>
                     [--flutes <n>] [--plunge <f>] [--number <n>] [--name <name>]
open-cam tool remove <file> --id <id>
open-cam operation add <file> --id <id> --kind face|contour|pocket|drill
                     --tool <id> --depth <d> [--stepdown <d>] [--stepover <d>]
                     [--strategy raster|spiral]
                     [--shape rect|circle]
                     [--x <x>] [--y <y>] [--width <w>] [--height <h>]
                     [--holes <x,y;x,y>] [--peck <d>] [--dwell <seconds>]
                     [--island <x,y,w,h>]...
open-cam operation list <file>
open-cam operation update <file> --id <id> [--tool <id>] [--depth <mm>]
                     [--stepdown <mm>] [--stepover <mm>] [--name <name>]
                     [--peck <mm>] [--dwell <seconds>]
open-cam operation duplicate <file> --id <id> [--new-id <id>]
open-cam operation delete <file> --id <id>
open-cam validate <file>
open-cam toolpaths <file> [--out report.json]
open-cam gcode <file> [--out program.nc]
open-cam export <file> --format gcode|json|csv|md --out <path>
open-cam report <file>
open-cam capabilities
```

A complete job:

```shell
open-cam create bracket.ocam
open-cam stock bracket.ocam --width 120 --depth 80 --height 12
open-cam tool add bracket.ocam --id t6 --diameter 6 --spindle 12000 --feed 1200 --plunge 300
open-cam operation add bracket.ocam --id face --kind face --tool t6 --depth 1 --stepover 4
open-cam operation add bracket.ocam --id holes --kind drill --tool t6 --depth 6 --holes "20,20;80,40"
open-cam validate bracket.ocam
open-cam gcode bracket.ocam --out bracket.nc
```

`gcode` prints the program to stdout and optionally writes it. `export` writes a
file and returns a JSON report with the byte count. Exit code `0` is success, `1`
is invalid input or an unmachineable document, and `2` is an unimplemented
capability.

## SDK

```ts
import { OpenCAM } from '@wae/open-cam';

const cam = new OpenCAM();
const document = await cam.create('bracket.ocam');

await document.setStock({ width: 120, depth: 80, height: 12 });
await document.addTool({
    id: 't6',
    name: '6 mm flat',
    number: 1,
    diameter: 6,
    flutes: 2,
    spindle: 12000,
    feed: 1200,
    plungeFeed: 300,
});
await document.addOperation({ id: 'face', kind: 'face', tool: 't6', depth: 1, stepdown: 0.5, stepover: 4 });
await document.addOperation({
    id: 'pocket',
    kind: 'pocket',
    tool: 't6',
    depth: 2,
    x: 10,
    y: 10,
    width: 100,
    height: 70,
    islands: [{ x: 40, y: 35, width: 20, height: 20 }],
});

const report = await document.toolpaths();
console.log(report.estimatedSeconds);

const program = await document.gcode();
const validation = await document.validate();
console.log(validation.ok, program.split('\n').length);

await cam.dispose();
```

Open an existing file with `await cam.open('bracket.ocam')`. Editing methods
return the document summary as written to disk, so `revision`, `tools` and
`operations` always match the file.

## Document model

| Field | Meaning |
|-------|---------|
| `stock` | rectangular block; the top face is `z = height` |
| `tools` | diameter, flute count, spindle speed, cutting feed, plunge feed |
| `operations` | `face`, `contour`, `pocket` or `drill` |

Contour and pocket use `x`, `y`, `width`, `height`; drill uses `holes` and an
optional `peck` or `dwell`. `dwell` is a bottom-of-hole pause in seconds emitted
as `G82 P<seconds>`, cannot be combined with `peck`, and does not change the
toolpath or cycle-time estimate. A pocket may also carry `islands`, an array of
`{ x, y, width, height }` rectangles that the raster clears around. A pocket
takes an optional `strategy`: `raster` (the default) or `spiral`, which walks
concentric closed rectangles inward and does not support islands. A pocket also
takes an optional `shape`: `rect` (the default) or `circle`, where `width` is the
diameter, `height` must equal `width`, and the cutter walks concentric rings to
the centre; a circular pocket accepts neither an explicit strategy nor islands.
A contour also takes `shape`: `circle` cuts a closed outside profile of radius
`width / 2 + r` as 72 linear segments. Both a rectangular and a circular contour
accept `tabs`: `count` equal angular or perimeter shares with each tab centred in
its share and `width` millimetres long along the tool-centre path, leaving
`height` millimetres of material above the cut floor (capped at the clearance
plane).
`stepdown` is the maximum cut per Z pass and `stepover` defaults to 70 % of the
tool diameter for facing and 60 % for pocketing. The toolpath report reports
`islandCount`, `skippedLength` and `skippedSegments` per operation, all `0`
without islands, plus `strategy` and `loopCount` for a spiral pocket, `shape`
and `loopCount` for a circular pocket, and `shape` for a circular contour.

## Constraints

- Units are millimetres; the engine does not convert.
- The G-code writer is a generic absolute-millimetre mill post
  (`G21 G90 G17 G94`), not a vendor post-processor.
- 3D surfacing, adaptive clearing, turning, four-axis work and STEP/DXF import
  are not implemented and are reported as such.
- `validate()` is the support claim: only a document that reports `ok: true` can
  be machined.

## License

MPL-2.0.
