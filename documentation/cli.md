# Command line

`open-cam` is the packaged CLI. The Rust binary `ocam` accepts the same commands
for development and prints the same JSON.

```text
open-cam <command> [options]
```

Exit codes: `0` success, `1` invalid input or an unmachineable document, `2` the
capability is not implemented.

## create

```shell
open-cam create <file.ocam>
```

Writes a new document with `open-cam` format version 1, units `mm`, revision 0,
a 100 × 100 × 10 mm stock and no tools. An existing file is rejected with
`file/exists`.

## stock

```shell
open-cam stock <file> --width <w> --depth <d> --height <h>
```

Replaces the stock; all three values are required and must be positive.

## tool add

```shell
open-cam tool add <file> --id <id> --diameter <d> --spindle <rpm> --feed <f>
                     [--flutes <n>] [--plunge <f>] [--number <n>] [--name <name>]
```

`--flutes` defaults to 2, `--number` to 1, `--name` to the id and `--plunge` to
`feed / 3`. Adding a duplicate id reports `tool/duplicate-id`.

## tool remove

```shell
open-cam tool remove <file> --id <id>
```

A tool referenced by an operation reports `tool/in-use`; an unknown id reports
`tool/unknown-id`.

## operation add

```shell
open-cam operation add <file> --id <id> --kind <kind> --tool <id> --depth <d>
                      [--stepdown <d>] [--stepover <d>] [--ramp <mm>] [--name <name>]
                      [--strategy raster|spiral]
                      [--shape rect|circle]
                      [--x <x>] [--y <y>] [--width <w>] [--height <h>]
                      [--holes <x,y;x,y>] [--peck <d>] [--dwell <seconds>]
                      [--island <x,y,w,h>]...
                      [--path <x,y;x,y>]
                      [--tabs <count,width,height>]
                      [--closed] [--finish]
```

| Kind | Geometry |
|------|----------|
| `face` | whole stock top; optional `--stepover` |
| `contour` | `--x --y --width --height`, milled outside by the cutter radius; optional `--tabs` on a rectangle or circle; `--shape circle` cuts a circular outside profile |
| `pocket` | `--x --y --width --height`, raster or spiral inside the cutter radius, or `--shape circle` for concentric rings, repeatable `--island` for rectangles, optional `--finish` wall loop |
| `drill` | `--holes "x,y;x,y"`, optional `--peck` or `--dwell` |
| `engrave` | `--path "x,y;x,y;..."`, a polyline followed by the cutter centre line; optional `--closed` |

`--stepdown` defaults to `--depth` (one Z pass). `--stepover` defaults to 70 % of
the tool diameter for facing and 60 % for pocketing. The operation is validated
against the stock before it is saved.

`--dwell <seconds>` is an optional bottom-of-hole pause for `drill`. Each hole
then emits `G82 X.. Y.. Z.. R.. P<seconds> F..` instead of `G81 ...` and the
cycle is still cancelled with `G80`. The value must be finite and greater than
zero and may not be combined with `--peck`, because `G83` has no dwell and the
writer does not approximate it; both cases report `operation/invalid`.
`--dwell` on any other kind also reports `operation/invalid`. A dwell adds no
move, so the toolpath and its kinematic cycle time are unchanged; the pause is
machine time and is not included in the estimate.

`--ramp <mm>` is an optional horizontal length in millimetres, not an angle.
Without it every Z level starts with a vertical plunge. With it the cutter
starts at the clearance plane above the first cutting move of the level and
descends to the level depth while covering exactly `<mm>` millimetres
horizontally along that first cutting direction, folding back inside that first
segment when `<mm>` is longer than it and returning to the entry point at depth;
the rest of the level path keeps its full length. `ramp` applies only to
`contour`, `pocket` and `engrave`. A value that is not finite and greater than
zero reports `operation/invalid`, as does `--ramp` on `face` or `drill`.

`--strategy raster|spiral` selects the pocket clearing pattern and defaults to
`raster`. `spiral` walks concentric closed rectangles from the pocket shrunk by
the cutter radius inward, one `stepover` per ring, and stops once the uncut core
inside a ring is no wider or taller than the tool diameter (opposite passes then
overlap). Rings advance at the cutting feed without retracting. `spiral` does
not support islands: a pocket with `--island` and `--strategy spiral` reports
`operation/strategy-island`. An unknown value reports `operation/invalid`.

`--island x,y,w,h` is repeatable and only valid for `pocket`. Each island is a
rectangle in the pocket's coordinate frame; the tool path skips every raster
portion that falls inside an island expanded by the cutter radius and crosses
the gap at the clearance plane. An island must lie inside the pocket, far enough
from the wall that its expanded rectangle fits inside the pocket shrunk by the
cutter radius, and expanded islands may not overlap.

`--shape rect|circle` selects the `contour` or `pocket` outline and defaults to
`rect`. An unknown value, or `circle` on another kind, reports
`operation/invalid`. `circle` treats `--width` as the diameter and requires
`--height` equal to `--width`; a mismatch reports `operation/circle-size` for
both kinds. A circular pocket walks concentric rings from radius `width / 2 − r`
inward, one `stepover` per ring, closes every ring at the cutting feed, advances
between rings without retracting and finishes at the centre. A circular contour
cuts one closed outside profile of radius `width / 2 + r` per depth level,
starting at angle 0 (+X) and running counter-clockwise as 72 linear segments
that close at the cutting feed; every level enters with a plunge or `--ramp` and
retracts to the clearance plane. `--tabs` is supported on a circular contour
with the same meaning as on a rectangle. `circle` on a pocket accepts no explicit
`--strategy` (`operation/strategy-circle`) and no `--island`
(`operation/strategy-island`); with `rect` both kinds behave exactly as before.

`--path x,y;x,y;...` is only used by `engrave` and lists the polyline vertices in
order. Every depth level repeats the whole polyline at its own Z. The cutter
follows the polyline itself, so no cutter-radius compensation is applied. An
engrave operation needs at least two points, every point must lie on the stock
and adjacent points may not coincide; violations report `operation/invalid` or
`path/outside-stock`. `engrave` accepts neither `--holes` nor `--island`
(`operation/invalid`).

`--closed` is a flag for `engrave` that adds one cutting move from the last path
point back to the first at every depth level, so the polyline forms a closed
loop. The path must not already end on its first point; that reports
`operation/invalid` instead of emitting a zero-length move. `--closed` on any
other kind reports `operation/invalid`. Without the flag the engraving is exactly
the open polyline as before.

`--tabs count,width,height` is only used by a `contour` and leaves holding tabs
in the profile. `count` tabs are spaced evenly around the path; each tab keeps
`height` millimetres of material above the cut floor over a `width` millimetre
length of the path. A rectangle measures tab widths along its perimeter; a circle
measures them along the tool-centre circle of radius `width / 2 + r` and centres
each tab at angle `2 × pi × (k + 0.5) / count`. Over a tab the cutter lifts to the
level plus `height` (capped at the clearance plane), rapids along the path to the
far edge of the tab and plunges back. The operation reports `operation/invalid`
when another kind uses tabs, when `count` is zero, when `width` or `height` is
not positive, when a rectangle's `height` exceeds `--depth`, when a rectangle's
`count × width` is not below the profile perimeter `2 × (width + height)`, or when
a circle's `count × width` is not below the tool-centre circumference
`2 × pi × (width / 2 + r)`.

`--finish` is a flag for `pocket` that adds one extra closed loop along the
final wall after the clearing moves of every depth level. The loop follows the
nominal cutter radius: the rectangle shrunk by `r` for raster and spiral
pockets, or the ring of radius `width / 2 − r` for a circular pocket. It runs at
the level depth and the cutting feed and counts toward the reported cut length
and cycle time. `--finish` cannot be combined with `--island`
(`operation/invalid`) and does not change the clearing moves when omitted.
`--finish` on any kind other than `pocket` reports `operation/invalid`.

## operation remove

```shell
open-cam operation remove <file> --id <id>
```

## validate

```shell
open-cam validate <file>
```

Prints `{ "ok": true|false, "issues": [...] }`. Each issue has `code`,
`severity` and `message`.

## toolpaths

```shell
open-cam toolpaths <file> [--out report.json]
```

Prints the toolpath report and optionally writes the same JSON. The report
contains `safeZ`, `rapidFeed`, total `cutLength`, `rapidLength`,
`estimatedSeconds` and one entry per operation with every move. Each operation
also reports `islandCount`, `skippedLength` and `skippedSegments`; they are `0`
for operations without islands. A spiral pocket additionally reports `strategy`
(`"spiral"`) and `loopCount`, the number of closed clearing loops emitted over
every depth level; a circular pocket reports `shape` (`"circle"`) and
`loopCount`, the number of closed clearing rings; a circular contour reports
`shape` (`"circle"`) and no `loopCount`. A wall finish loop is not
counted in `loopCount`, but its moves are included in `segments`, `cutLength`
and the cycle time. Raster operations keep the previous shape and add no
fields. A drill `dwell` is a bottom-of-hole pause and does not add a move or
cycle time.

## gcode

```shell
open-cam gcode <file> [--out program.nc]
```

Prints the G-code program and optionally writes it. The program is
deterministic; repeated runs are byte-identical.

## export

```shell
open-cam export <file> --format gcode|json|csv|md --out <path>
```

| Format | Content |
|--------|---------|
| `gcode` | the complete program |
| `json` | the toolpath report |
| `csv` | one row per move: operation, kind, from/to, length, feed |
| `md` | a Markdown stock/tool/operation summary |

`step` and `dxf` are declared but unimplemented and exit with code `2`.

## report

```shell
open-cam report <file>
```

Prints the document summary, validation, toolpaths and the G-code line count.

## capabilities

```shell
open-cam capabilities
```

Prints the machine-readable capability matrix.

## Examples

```shell
open-cam create bracket.ocam
open-cam stock bracket.ocam --width 120 --depth 80 --height 12
open-cam tool add bracket.ocam --id t6 --diameter 6 --spindle 12000 --feed 1200 --plunge 300
open-cam operation add bracket.ocam --id pocket --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --ramp 8 --x 20 --y 20 --width 60 --height 40 \
    --island 40,26,20,20
open-cam operation add bracket.ocam --id spiral --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --strategy spiral --x 20 --y 20 --width 60 --height 40
open-cam operation add bracket.ocam --id bore --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --shape circle --x 20 --y 20 --width 40 --height 40
open-cam operation add bracket.ocam --id boss --kind contour --tool t6 \
    --depth 8 --stepdown 2 --shape circle --x 20 --y 20 --width 40 --height 40
open-cam operation add bracket.ocam --id logo --kind engrave --tool t2 \
    --depth 0.5 --stepdown 0.25 --path "10,10;40,10;40,30;70,30"
open-cam operation add bracket.ocam --id border --kind engrave --tool t2 \
    --depth 0.5 --closed --path "10,10;40,10;40,30;10,30"
open-cam operation add bracket.ocam --id bore --kind drill --tool t6 \
    --depth 6 --holes "20,20;80,40" --dwell 0.5
open-cam operation add bracket.ocam --id floor --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --x 20 --y 20 --width 60 --height 40 --finish
open-cam operation add bracket.ocam --id profile --kind contour --tool t6 \
    --depth 8 --stepdown 2 --x 20 --y 20 --width 60 --height 40 \
    --tabs 4,6,1
open-cam validate bracket.ocam
open-cam toolpaths bracket.ocam
open-cam gcode bracket.ocam --out bracket.nc
```
