# Command line

`open-cam` is the packaged CLI: a TypeScript assembly over the `@wae/open-cam`
SDK, built with `@wae/commander`. There is no Rust CLI binary; every command
maps onto an orthogonal Node-API export and prints the same JSON.

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

## import dxf

```shell
open-cam import dxf <file.ocam> --from <drawing.dxf>
                     [--depth <mm>] [--stepdown <mm>] [--tolerance <mm>]
```

Creates a new document from an ASCII DXF drawing; the target must not exist.
Supported 2D entities are `LINE`, `LWPOLYLINE`, `POLYLINE` with its `VERTEX`
records, `ARC` and `CIRCLE`; each becomes one `engrave` operation named after
its DXF layer with id `<layer slug>-<n>`. The import uses one fixed `engrave`
tool (0.2 mm, 12 000 rpm, 300 mm/min feed, 100 mm/min plunge). Arcs and circles
are flattened with a deterministic chord sagitta tolerance: `--tolerance`
defaults to the documented constant `0.05` mm. `--depth` defaults to `1` mm and
`--stepdown` to `--depth`. The drawing is translated so the minimum X and Y are
`0`; stock width and depth follow its bounding box (minimum 1 mm) and height is
`max(bounding-box depth, depth) + 5` mm. The new document keeps revision `0`.

The JSON report carries `format` (`open-cam-dxf-import`), `version`, `units`,
`source`, `path`, `insUnits`, `sourceUnits`, `tolerance`, `depth`, `stepdown`,
`entityCount`, `operationCount`, `skippedCount`, `losses` and the written
`document` summary. `$INSUNITS` is reported, never converted: `4` is
`millimetres`, `0` is `unitless`, a missing header is `unspecified`, and any
other value is read as millimetres and reported as
`import/dxf-unsupported-units`.

Unsupported entity types (`SPLINE`, `ELLIPSE`, `HATCH`, `INSERT`/blocks, …),
every section other than `HEADER` and `ENTITIES`, a non-zero `LWPOLYLINE` bulge
and an entity with no usable geometry are skipped into `losses` with
`import/dxf-unsupported-entity`, `import/dxf-unsupported-section`,
`import/dxf-bulge-ignored` or `import/dxf-empty-entity`; they never fail the
import. A malformed group code reports `import/dxf-malformed-group` and a
supported entity with missing or invalid values reports
`import/dxf-malformed-entity`, both with the 1-based source line. A failed
import writes nothing.

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

## tool list

```shell
open-cam tool list <file>
```

Prints the `open-cam-tool-table` report: `toolCount`, `unusedCount` and one
entry per declared tool in document order with its `usedBy` operation ids and a
`used` flag. A tool no operation references is visible without opening the
document.

## tool export

```shell
open-cam tool export <file> --out <library.json>
```

Writes the document tool table as a portable `open-cam-tools` library
(`format`, `version` 1, `units` `mm`, `tools`). The file is deterministic and
every declared tool is written in document order, used or not.

## tool import

```shell
open-cam tool import <file> --from <library.json>
```

Validates the whole library, merges its tools into the document and saves it
atomically, then prints `{ "format": "open-cam-tool-import", "imported": [...],
"toolCount": <n>, ... }`. A malformed file, an unsupported format, version or
unit set reports `tool/library-invalid`; a malformed tool reports `tool/invalid`;
an id declared twice inside the file or already in the document reports
`tool/duplicate-id`. A rejected import leaves the document and its revision
untouched.

## operation add

```shell
open-cam operation add <file> --id <id> --kind <kind> --tool <id> --depth <d>
                      [--stepdown <d>] [--stepover <d>] [--ramp <mm>] [--name <name>]
                      [--strategy raster|spiral]
                      [--shape rect|circle] [--arcs linear|arc]
                      [--x <x>] [--y <y>] [--width <w>] [--height <h>]
                      [--holes <x,y;x,y>] [--pattern <layout>] [--peck <d>] [--dwell <seconds>]
                      [--island <x,y,w,h|circle,x,y,d>]...
                      [--path <x,y;x,y>]
                      [--tabs <count,width,height>]
                      [--lead <radius,cw|ccw>]
                      [--closed] [--finish]
```

| Kind | Geometry |
|------|----------|
| `face` | whole stock top; optional `--stepover` |
| `contour` | `--x --y --width --height`, milled outside by the cutter radius; optional `--tabs` on a rectangle or circle; optional `--lead` tangential entry and exit arcs; `--shape circle` cuts a circular outside profile, optionally with `--arcs arc`; |
| `pocket` | `--x --y --width --height`, raster or spiral inside the cutter radius, or `--shape circle` for concentric rings with optional `--arcs arc`, repeatable `--island` for rectangles and circles, optional `--finish` wall loop |
| `drill` | `--holes "x,y;x,y"` or `--pattern`, optional `--peck` or `--dwell` |
| `engrave` | `--path "x,y;x,y;..."`, a polyline followed by the cutter centre line; optional `--closed` |

`--stepdown` defaults to `--depth` (one Z pass). `--stepover` defaults to 70 % of
the tool diameter for facing and 60 % for pocketing. The operation is validated
against the stock before it is saved.

`--pattern` is only used by `drill` and replaces `--holes`. Two layouts exist:

| Value | Holes |
|-------|-------|
| `grid,rows,columns,spacingX,spacingY` | `columns × rows` holes in row-major order, starting at `--x,--y`: `columns` along X spaced `spacingX`, `rows` along Y spaced `spacingY` |
| `circular,count,radius` | `count` holes on the circle of `radius` around `--x,--y`, the first at angle 0 (+X), running counter-clockwise |

Every generated hole uses the same stock check, `--peck` and `--dwell` rules and
G-code cycle as an explicit hole list, and the cycle is cancelled once with
`G80`. A drill that declares both `--holes` and `--pattern`, a pattern on another
kind, an unknown layout, a grid with zero rows or columns or a non-positive
spacing, or a circular layout with zero holes or a non-positive radius reports
`operation/invalid`; a generated hole outside the stock reports
`path/outside-stock`. Without `--pattern` the explicit hole list is unchanged.

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

`--island` is repeatable and only valid for `pocket`. `x,y,w,h` is a rectangular
island and `circle,x,y,d` a disc of diameter `d` centred in the bounding box
starting at `x,y`, both in the pocket's coordinate frame; the tool path skips
every raster portion that falls inside an island expanded by the cutter radius
and crosses the gap at the clearance plane, a circle island blocking the chord
of its keep-out circle on each line. An island must lie inside the pocket, far
enough from the wall that its expanded outline fits inside the pocket shrunk by
the cutter radius, and expanded islands may not overlap. An unknown shape or a
non-positive diameter reports `operation/island-invalid`.

`--shape rect|circle` selects the `contour` or `pocket` outline and defaults to
`rect`. An unknown value, or `circle` on another kind, reports
`operation/invalid`. `circle` treats `--width` as the diameter and requires
`--height` equal to `--width`; a mismatch reports `operation/circle-size` for
both kinds. A circular pocket walks concentric rings from radius `width / 2 − r`
inward, one `stepover` per ring, closes every ring at the cutting feed, advances
between rings without retracting and finishes at the centre. A circular contour
cuts one closed outside profile of radius `width / 2 + r` per depth level,
starting at angle 0 (+X) and running counter-clockwise as 72 linear segments by
default or as `G3` arcs with `--arcs arc`, closing at the cutting feed; every
level enters with a plunge, `--ramp` or `--lead` arc and retracts to the
clearance plane. `--tabs` is supported on a circular contour with the same
meaning as on a rectangle. `circle` on a pocket accepts no explicit
`--strategy` (`operation/strategy-circle`) and no `--island`
(`operation/strategy-island`); with `rect` both kinds behave exactly as before.

`--arcs linear|arc` selects the interpolation of the closed rings of a `circle`
contour or pocket and defaults to `linear`. `linear` keeps the 72-chord
approximation; `arc` emits every ring as counter-clockwise `G3` arcs of at most
half a circle, so a tab-free turn becomes two half arcs and an interval between
two tabs becomes as many arcs as its angular span needs. Ring radii, `--stepover`,
depth levels, feeds, ring transitions, tab lifts, `--ramp` entries and the
reported loop count are unchanged, and each arc contributes `radius × sweep` to
the cut length and the cycle time. `--arcs arc` is valid only together with
`--shape circle` on `contour` or `pocket`; another kind or shape and an unknown
value report `operation/invalid`. Without the option the output stays
byte-identical.

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

`--lead radius,cw|ccw` is only used by a `contour` and replaces the vertical
entry plunge and the direct closing retract of every Z level with tangential
half-circle arcs. The cutter rapids to the point `2 × radius` away from the
profile start along the normal of the first edge (outward for `cw`, profile
side for `ccw`), plunges there and cuts one half circle that joins the start
exactly along the profile tangent;
after the level loop closes it leaves through the mirror half circle at the
closing point before retracting. `cw` renders `G2` and `ccw` renders `G3`; for
the counter-clockwise contour `cw` keeps both arcs outside the profile. Each
arc runs at the cutting feed, spans `pi × radius` and counts toward the
reported cut length and cycle time. The value must be a `radius,cw|ccw` pair;
a radius that is not finite and greater than zero or an unknown direction
reports `operation/invalid`, as does `--lead` on any other kind or together
with `--ramp`, which replaces the same entry plunge. Without `--lead` the
vertical plunge and direct retract are emitted exactly as before.

`--finish` is a flag for `pocket` that adds one extra closed loop along the
final wall after the clearing moves of every depth level. The loop follows the
nominal cutter radius: the rectangle shrunk by `r` for raster and spiral
pockets, or the ring of radius `width / 2 − r` for a circular pocket. It runs at
the level depth and the cutting feed and counts toward the reported cut length
and cycle time. `--finish` cannot be combined with `--island`
(`operation/invalid`) and does not change the clearing moves when omitted.
`--finish` on any kind other than `pocket` reports `operation/invalid`.

## operation list

```shell
open-cam operation list <file>
```

Prints the `open-cam-operation-list` report: `operationCount`, `usedTools` and
one entry per operation in document order. Each entry carries `id`, `kind`,
`tool`, `name`, `depth`, `stepdown`, `stepover`, `strategy` (`raster` or
`spiral` for a pocket and `null` for every other kind), `shape` (`rect` or
`circle` for a contour or pocket and `null` otherwise), `holes` (including a
generated pattern, `0` for other kinds), `islandCount`, `finish` (`false` when
the kind has none) and `estimatedSeconds`. The time is the kinematic motion
time of that operation alone: the cycle-time model at the default rapid feed
without tool-change overhead. The command only reads the document and writes
nothing.

## operation update

```shell
open-cam operation update <file> --id <id> [--tool <id>] [--depth <mm>]
                          [--stepdown <mm>] [--stepover <mm>] [--name <name>]
                          [--peck <mm>] [--dwell <seconds>]
```

Partial edit: only the supplied fields change and at least one field is
required, otherwise the command reports `operation/invalid`. `--depth`,
`--stepdown`, `--stepover`, `--peck` and `--dwell` must be finite and greater
than zero. `--peck` and `--dwell` are valid only on `drill` and may not be
combined, mirroring the add-time rule. `--tool` must name a declared tool, so an
unknown id reports `operation/unknown-tool`. An unknown `--id` reports
`operation/unknown-id`.

The whole candidate document is validated with the same rules as
`operation add` and `save` before it is written: on any failure nothing is
written and the revision stays the same. On success the document is saved
atomically, the revision rises by one and the usual edit summary is printed.
This command changes only `--tool`, `--name`, `--depth`, `--stepdown`,
`--stepover`, `--peck` and `--dwell`; it cannot change the kind, geometry
coordinates, holes, islands, tabs, `--lead`, `--ramp` or `--arcs`, clear an
optional field, reorder operations or merge two operations.

## operation delete

```shell
open-cam operation delete <file> --id <id>
```

Removes one operation and saves the document atomically. `operation remove` is
accepted as an alias. An unknown `--id` reports `operation/unknown-id`, and the
remaining operations keep their document order. Removing the last operation is
allowed and leaves a document with zero operations, which is still valid
(`operation/none` is only a warning). An operation is self-contained: its
geometry, holes, islands, tabs, lead, ramp and arcs live inside it, and the tool
table, sequence plan and operation list are derived from the operation array on
demand, so a delete never leaves a dangling reference and no `operation/in-use`
guard is required. The whole candidate document is validated with the same rules
as `operation add`, `operation update` and `save` before it is written: on any
failure nothing is written and the revision stays the same. On success the
document is saved atomically, the revision rises by one and the usual edit
summary is printed.

## operation duplicate

```shell
open-cam operation duplicate <file> --id <id> [--new-id <id>]
```

Copies one operation and inserts the copy immediately after its source. The copy
keeps every field of the source — kind, tool, geometry, holes, islands, tabs,
lead, ramp, arcs and name — and only the identifier changes; the name is never
suffixed automatically. An unknown `--id` reports `operation/unknown-id`.
`--new-id` must be non-empty and not already declared, otherwise the command
reports `operation/invalid` (empty) or `operation/duplicate-id` (already used).
Without `--new-id` the copy takes the smallest free `op-<n>` identifier,
starting at `op-1`. The whole candidate document is validated with the same
rules as `operation add`, `operation update` and `save` before it is written: on
any failure nothing is written and the revision stays the same, while a
successful duplicate is saved atomically, bumps the revision once and prints the
usual edit summary.

## operation reorder

```shell
open-cam operation reorder <file> --id <id> --to <index>
```

Moves one operation to a final 0-based position and saves the document
atomically. `--to 0` puts it first and `--to <operationCount - 1>` puts it last:
the operation is removed from its current position and reinserted at `--to`, so
`--to` is the position the operation occupies after the move. An unknown
`--id` reports `operation/unknown-id`; a `--to` at or beyond the operation count
reports `operation/invalid`, and a negative or non-integer `--to` reports
`cli/invalid-number`.

Moving an operation to its current index is a no-op: the command succeeds with
`changed: false`, does not rewrite the file and leaves the revision unchanged.
A real move keeps every field of every operation, changes only the order and
reports `changed: true` with the saved revision. Operation order is the
execution order, so toolpaths, G-code, the cycle-time estimate, the operation
list and the sequence plan all follow the new order; no document structure is
stored to rebuild and ids never change. The whole candidate document is
validated with the same rules as `operation add`, `operation update`,
`operation delete` and `save` before it is written: on any failure nothing is
written and the revision stays the same, while a successful move is saved
atomically, bumps the revision once and prints an
`open-cam-operation-reorder` report with `id`, `from`, `to`, `changed`,
`operationCount` and the saved `revision`.

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
and the cycle time. A contour lead arc and every ring of an `--arcs arc` contour
or pocket are moves of kind `arc` carrying an `arc` object with its `centre` and
`clockwise` flag; linear moves omit the
field. Raster operations keep the previous shape and add no
fields. A drill `dwell` is a bottom-of-hole pause and does not add a move or
cycle time.

## estimate

```shell
open-cam estimate <file> [--rapid-feed <mm/min>] [--tool-change <seconds>] [--out estimate.json]
```

Prints the cycle-time report and optionally writes the same JSON. The report
reuses the generated toolpath moves and splits every operation into
`cuttingSeconds`, `plungeSeconds`, `retractSeconds`, `traverseSeconds` and
`toolchangeSeconds`. Cutting and plunge time use the move feeds; vertical
rapid lifts count as `retractSeconds` and every other rapid move as
`traverseSeconds`, both at `--rapid-feed` (default `8000` mm/min). One fixed
`--tool-change` overhead (default `3` seconds) is charged to the first
operation that uses each distinct tool and counted in `toolChanges`.
`totalSeconds` sums the five components per operation and over the whole
program; with the default request the motion time equals the toolpath
report's `estimatedSeconds` plus `toolChanges × 3`.

## feeds

```shell
open-cam feeds --material <name> --diameter <mm> [--flutes <n>]
```

Prints the `open-cam-feeds` recommendation for one tool in one material.
`--flutes` defaults to `2`. The report carries the canonical `material`, the
`diameter` and `flutes` used, the table `surfaceSpeed` (m/min), the selected
`chipLoad` per tooth (mm), and the rounded `spindle` (rpm), `feed` and
`plungeFeed` (mm/min).

Supported materials are `aluminium` (alias `aluminum`), `brass`, `steel`,
`stainless`, `plastic` and `wood`. The spindle is
`1000 × surfaceSpeed / (π × diameter)`, rounded to 100 rpm and clamped to
`100 … 24000`; the feed is `spindle × chipLoad × flutes` and the plunge feed 30 %
of it, both rounded to 10 mm/min with a floor of 10. An unknown material reports
`feeds/unknown-material`; a diameter that is not finite and positive or a flute
count of zero reports `feeds/invalid`. The recommendation reads no document and
writes nothing.

## gcode

```shell
open-cam gcode <file> [--out program.nc]
                [--program-number <n>] [--line-numbers] [--line-increment <n>]
                [--coolant <off|flood|mist>]
```

Prints the G-code program and optionally writes it. The program is
deterministic; repeated runs are byte-identical. Only contour lead arcs and the
rings of an `--arcs arc` contour or pocket are interpolated: they render as
`G2`/`G3` with absolute end coordinates and `I`/`J` centre offsets; every other
move stays `G0` or `G1`.

The post options change only the program text, never a move, feed or cycle:

| Option | Effect |
|--------|--------|
| `--program-number <n>` | emits `O<n>` immediately after the leading `%` |
| `--line-numbers` | prefixes every line between the two `%` lines with `N10`, `N20`, … |
| `--line-increment <n>` | distance between two line numbers, at least `1` (default `10`) |
| `--coolant flood` | emits `M8` after the spindle start of each operation and `M9` before `M5` |
| `--coolant mist` | emits `M7` after the spindle start of each operation and `M9` before `M5` |

`--coolant off` and every omitted option render the plain program byte for byte.
Units `G21`, absolute mode `G90`, the `G17` plane and `G94` feed mode stay
fixed. A program number of zero, a line increment of zero or an unknown coolant
reports `post/invalid` with exit code `1`. `export --format gcode` always uses
the defaults.

## sequence

```shell
open-cam sequence <file> [--apply]
```

Prints the tool-grouped execution plan as `open-cam-sequence`. The plan keeps
every operation of one tool together: tools keep the order of their first
appearance and the operations inside a tool keep their document order. The
report carries `operationCount`, `beforeToolChanges`, `afterToolChanges`,
`unchanged`, `applied`, `revision`, one `groups` entry per tool and every
`operations` entry in its proposed position.

`--apply` rewrites the document only when the order changes. An already grouped
document is not written and reports `applied: false` with its current revision;
otherwise the reordered document is validated and saved atomically and the
report carries the new revision. Sequencing changes no operation field, move or
feed.

## export

```shell
open-cam export <file> --format gcode|json|csv|md|svg|pdf|dxf [--out <path>]
```

| Format | Content |
|--------|---------|
| `gcode` | the complete program |
| `json` | the toolpath report |
| `csv` | one row per move: operation, kind, from/to, length, feed |
| `md` | a Markdown stock/tool/operation summary |
| `svg` | a vector toolpath drawing with a millimetre `viewBox` at 8 px/mm |
| `pdf` | the same drawing as a one-page A4 landscape PDF 1.4 |
| `dxf` | the document geometry as ASCII DXF `LINE`, `LWPOLYLINE` and `CIRCLE` entities |

`--out` is required for `gcode`, `json`, `csv`, `md` and `dxf`. An `svg` or `pdf`
drawing is written to `--out` when given and printed to stdout otherwise, so it
can be piped or redirected. With `--out`, the command prints the usual report
(`format`, `path`, `files`, `bytes` and `dropped`); a drawing sent to stdout is
printed unchanged. `dropped` counts non-ASCII characters the PDF font replaced
with `?`; the CLI also prints that count to stderr. SVG keeps Unicode.

The drawing is rendered from the toolpath report, never recomputed:

- the stock rectangle with its width × depth × height label;
- one cutting polyline per operation, in a stable palette by operation index
  (`face`/`contour`/… colours follow the list position, not the clock);
- rapid `G0` moves as light dashed lines in one underlay below the cutting
  polylines;
- pocket island outlines (rect polygon, circle) in a slate colour;
- a thick burnt-orange marker segment per contour holding tab;
- a ring at the operation start and a dot at every plunge point;
- a legend that names cutting moves, rapid moves, plunge/start points, the
  island and tab markers when present, and one row per operation with its kind,
  id and tool diameter/radius;
- a title block with the document id, revision, units, operation count,
  estimated time and scale (SVG is 1:1 at 8 px/mm; PDF is fitted to the page and
  prints the resulting percentage).

Arc moves are drawn as polylines sampled at the engine's ring resolution
(72 chords per turn) with the exact move endpoint, so the drawing matches the
linear ring output. Coordinates are rounded to three decimals, operations keep
document order and no timestamp is written, so two runs are byte-identical. An
empty document still produces a valid empty sheet with the stock and title
block. PDF text is ASCII-only and uses Helvetica.

`export --format step` is declared but unimplemented and exits with code `2`.

The `dxf` export writes nominal geometry, not the toolpath: the stock
rectangle on layer `STOCK`, one `LINE` or `LWPOLYLINE` per engrave path, one
closed rectangle `LWPOLYLINE` or `CIRCLE` per contour and pocket outline and
per pocket island, and one `CIRCLE` of the tool radius per drill hole.
Operation layers are the sanitized operation ids and coordinates are rounded
to three decimals. An engrave drawing round-trips through `import dxf`.

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
open-cam import dxf logo.ocam --from logo.dxf --depth 0.5 --tolerance 0.02
open-cam stock bracket.ocam --width 120 --depth 80 --height 12
open-cam tool add bracket.ocam --id t6 --diameter 6 --spindle 12000 --feed 1200 --plunge 300
open-cam operation add bracket.ocam --id pocket --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --ramp 8 --x 20 --y 20 --width 60 --height 40 \
    --island 40,26,20,20 --island circle,66,36,8
open-cam operation add bracket.ocam --id spiral --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --strategy spiral --x 20 --y 20 --width 60 --height 40
open-cam operation add bracket.ocam --id bore --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --shape circle --x 20 --y 20 --width 40 --height 40
open-cam operation add bracket.ocam --id boss --kind contour --tool t6 \
    --depth 8 --stepdown 2 --shape circle --x 20 --y 20 --width 40 --height 40
open-cam operation add bracket.ocam --id bore-arc --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --shape circle --arcs arc --x 20 --y 20 --width 40 --height 40
open-cam tool list bracket.ocam
open-cam tool export bracket.ocam --out shop-tools.json
open-cam tool import bracket.ocam --from shop-tools.json
open-cam operation add bracket.ocam --id logo --kind engrave --tool t2 \
    --depth 0.5 --stepdown 0.25 --path "10,10;40,10;40,30;70,30"
open-cam operation add bracket.ocam --id border --kind engrave --tool t2 \
    --depth 0.5 --closed --path "10,10;40,10;40,30;10,30"
open-cam operation add bracket.ocam --id bore --kind drill --tool t6 \
    --depth 6 --holes "20,20;80,40" --dwell 0.5
open-cam operation add bracket.ocam --id grid --kind drill --tool t6 \
    --depth 6 --x 20 --y 20 --pattern grid,3,2,20,15
open-cam operation add bracket.ocam --id ring --kind drill --tool t6 \
    --depth 6 --x 60 --y 40 --pattern circular,8,20 --peck 2
open-cam feeds --material aluminium --diameter 6 --flutes 2
open-cam operation add bracket.ocam --id floor --kind pocket --tool t6 \
    --depth 3 --stepdown 1 --x 20 --y 20 --width 60 --height 40 --finish
open-cam operation add bracket.ocam --id profile --kind contour --tool t6 \
    --depth 8 --stepdown 2 --x 20 --y 20 --width 60 --height 40 \
    --tabs 4,6,1
open-cam operation add bracket.ocam --id edge --kind contour --tool t6 \
    --depth 2 --stepdown 2 --x 20 --y 20 --width 60 --height 40 \
    --lead 5,cw
open-cam operation list bracket.ocam
open-cam operation update bracket.ocam --id floor --depth 4 --stepdown 2
open-cam operation update bracket.ocam --id bore --peck 2
open-cam operation duplicate bracket.ocam --id floor --new-id floor-finish
open-cam operation reorder bracket.ocam --id logo --to 0
open-cam operation delete bracket.ocam --id edge
open-cam validate bracket.ocam
open-cam toolpaths bracket.ocam
open-cam sequence bracket.ocam --apply
open-cam gcode bracket.ocam --out bracket.nc
open-cam gcode bracket.ocam --program-number 1001 --line-numbers --coolant flood
open-cam export bracket.ocam --format svg --out bracket.svg
open-cam export bracket.ocam --format pdf --out bracket.pdf
open-cam export bracket.ocam --format svg > bracket.svg
open-cam export bracket.ocam --format dxf --out bracket.dxf
```
