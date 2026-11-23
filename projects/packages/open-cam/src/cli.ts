#!/usr/bin/env node
/**
 * Command-line assembly only.
 *
 * Every command maps options onto one session operation of `OpenCAM`; editing,
 * validation, toolpath generation and export stay in the native core. The CLI
 * is assembled with `@wae/commander` over the `new OpenCAM(options?)` SDK and
 * owns argument parsing, help text, printing and exit codes, and nothing else.
 */

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { createCli, type CommandContext, type ParsedOptions } from '@wae/commander';

import type { CamDocument } from './document.js';
import { NotImplementedError, OpenCAMError, type OpenCAMErrorCode } from './errors.js';
import { OpenCAM } from './open-cam.js';
import type {
    ExportFormat,
    Hole,
    Island,
    LeadSettings,
    OperationKind,
    PathPoint,
    PocketShape,
    PocketStrategy,
    TabSettings,
} from './types.js';

const here = path.dirname(fileURLToPath(import.meta.url));

/** One session serves every command; it is disposed when the run finishes. */
const app = new OpenCAM();

const cli = createCli('open-cam').locales(path.join(here, '..', 'locales')).intro('cli.intro');

// Exported for contract audits that import the built entry to inspect the
// command tree. Nothing runs until the file is executed as the program entry.
export { cli };

const text = (options: ParsedOptions, key: string): string | undefined =>
    typeof options[key] === 'string' ? options[key] : undefined;

const flag = (options: ParsedOptions, key: string): boolean => options[key] === true || options[key] === 'true';

/** Values collected for one repeatable option (`--island <x,y,w,h>...`). */
const values = (options: ParsedOptions, key: string): string[] => {
    const raw = options[key];
    if (Array.isArray(raw)) {
        return raw;
    }
    return typeof raw === 'string' ? [raw] : [];
};

/** Locale-backed CLI failure with the stable code kept as the contract. */
function fail(ctx: CommandContext, code: OpenCAMErrorCode, key: string, args?: Record<string, string>): never {
    throw new OpenCAMError(code, ctx.t(key, args));
}

function positional(ctx: CommandContext, options: ParsedOptions, usage: string): string {
    const value = options._[0];
    if (value === undefined) {
        fail(ctx, 'cli/missing-argument', 'cli.error.missingArgument', { usage });
    }
    return value;
}

function required(ctx: CommandContext, options: ParsedOptions, name: string): string {
    const value = text(options, name.slice(2));
    if (value === undefined) {
        fail(ctx, 'cli/missing-option', 'cli.error.missingOption', { option: name });
    }
    return value;
}

function number(ctx: CommandContext, options: ParsedOptions, name: string): number | undefined {
    const raw = text(options, name.slice(2));
    if (raw === undefined) {
        return undefined;
    }
    const value = Number(raw);
    if (!Number.isFinite(value)) {
        fail(ctx, 'cli/invalid-number', 'cli.error.invalidNumber', { option: name, value: raw });
    }
    return value;
}

function requiredNumber(ctx: CommandContext, options: ParsedOptions, name: string): number {
    const value = number(ctx, options, name);
    if (value === undefined) {
        fail(ctx, 'cli/missing-option', 'cli.error.missingOption', { option: name });
    }
    return value;
}

function integer(ctx: CommandContext, options: ParsedOptions, name: string): number | undefined {
    const value = number(ctx, options, name);
    if (value === undefined) {
        return undefined;
    }
    if (!Number.isInteger(value) || value < 0) {
        fail(ctx, 'cli/invalid-number', 'cli.error.invalidNumber', { option: name, value: String(value) });
    }
    return value;
}

function exportFormat(ctx: CommandContext, value: string): ExportFormat {
    if (value === 'gcode' || value === 'nc' || value === 'tap') {
        return 'gcode';
    }
    if (value === 'json' || value === 'csv' || value === 'md') {
        return value;
    }
    if (value === 'step' || value === 'stp' || value === 'dxf') {
        throw new NotImplementedError(`${value} export`);
    }
    throw fail(ctx, 'cli/unknown-format', 'cli.error.unknownFormat', { format: value });
}

function operationKind(value: string): OperationKind {
    if (value === 'face' || value === 'contour' || value === 'pocket' || value === 'drill' || value === 'engrave') {
        return value;
    }
    throw new OpenCAMError('operation/unknown-kind', `unknown operation kind \`${value}\``);
}

function pocketStrategy(value: string): PocketStrategy {
    if (value === 'raster' || value === 'spiral') {
        return value;
    }
    throw new OpenCAMError('operation/invalid', `unknown pocket strategy \`${value}\`; only raster or spiral is supported`);
}

function parseHoles(raw: string): Hole[] {
    return raw
        .split(';')
        .filter((entry) => entry.trim().length > 0)
        .map((entry) => {
            const parts = entry.split(',');
            const x = Number(parts[0]);
            const y = Number(parts[1]);
            if (parts.length !== 2 || !Number.isFinite(x) || !Number.isFinite(y)) {
                throw new OpenCAMError('cli/invalid-number', `hole \`${entry}\` must be an \`x,y\` pair`);
            }
            return { x, y };
        });
}

function parsePath(raw: string): PathPoint[] {
    return raw
        .split(';')
        .filter((entry) => entry.trim().length > 0)
        .map((entry) => {
            const parts = entry.split(',');
            const x = Number(parts[0]);
            const y = Number(parts[1]);
            if (parts.length !== 2 || !Number.isFinite(x) || !Number.isFinite(y)) {
                throw new OpenCAMError('cli/invalid-number', `path point \`${entry}\` must be an \`x,y\` pair`);
            }
            return { x, y };
        });
}

function parseIslands(raw: readonly string[]): Island[] {
    return raw.map((entry) => {
        const parts = entry.split(',');
        const x = Number(parts[0]);
        const y = Number(parts[1]);
        const width = Number(parts[2]);
        const height = Number(parts[3]);
        if (parts.length !== 4 || !Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(width) || !Number.isFinite(height)) {
            throw new OpenCAMError('cli/invalid-number', `island \`${entry}\` must be an \`x,y,width,height\` quadruple`);
        }
        return { x, y, width, height };
    });
}

function parseTabs(raw: string): TabSettings {
    const parts = raw.split(',').map((entry) => entry.trim());
    const count = Number(parts[0]);
    const width = Number(parts[1]);
    const height = Number(parts[2]);
    if (parts.length !== 3 || !Number.isInteger(count) || count < 0 || !Number.isFinite(width) || !Number.isFinite(height)) {
        throw new OpenCAMError('cli/invalid-number', `tabs \`${raw}\` must be a \`count,width,height\` triple`);
    }
    return { count, width, height };
}

function parseLead(raw: string): LeadSettings {
    const parts = raw.split(',').map((entry) => entry.trim());
    const radius = Number(parts[0]);
    const direction = parts[1];
    if (parts.length !== 2 || !Number.isFinite(radius) || (direction !== 'cw' && direction !== 'ccw')) {
        throw new OpenCAMError('cli/invalid-number', `lead \`${raw}\` must be a \`radius,cw|ccw\` pair`);
    }
    return { radius, direction };
}

function print(value: unknown): void {
    console.log(JSON.stringify(value, null, 2));
}

/** Opens one document for the duration of a command. */
async function withDocument(file: string, work: (document: CamDocument) => Promise<void>): Promise<void> {
    const document = await app.open(file);
    await work(document);
}

/** Maps a failure onto the documented exit codes: `capability/*` → 2, else 1. */
async function guard(ctx: CommandContext, work: () => Promise<number>): Promise<number> {
    try {
        return await work();
    } catch (error) {
        if (error instanceof OpenCAMError) {
            ctx.emitFailure(error);
            return error.code.startsWith('capability/') ? 2 : 1;
        }
        ctx.stderr(error instanceof Error ? error.message : String(error));
        return 1;
    }
}

cli
    .command('version', 'cli.cmd.version')
    .actionCtx(async (_options, _args, ctx) =>
        guard(ctx, async () => {
            ctx.stdout('0.0.0');
            return 0;
        }),
    );

cli
    .command('capabilities', 'cli.cmd.capabilities')
    .actionCtx(async (_options, _args, ctx) =>
        guard(ctx, async () => {
            print(await app.capabilities());
            return 0;
        }),
    );

cli
    .command('create', 'cli.cmd.create')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'create <file>');
            print(await app.create(file).then((document) => document.save()));
            return 0;
        }),
    );

cli
    .command('stock', 'cli.cmd.stock')
    .option('--width <mm>', 'cli.opt.width')
    .option('--depth <mm>', 'cli.opt.depth')
    .option('--height <mm>', 'cli.opt.height')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'stock <file>');
            await withDocument(file, async (document) => {
                print(
                    await document.setStock({
                        width: requiredNumber(ctx, options, '--width'),
                        depth: requiredNumber(ctx, options, '--depth'),
                        height: requiredNumber(ctx, options, '--height'),
                    }),
                );
            });
            return 0;
        }),
    );

const toolCommand = cli.command('tool', 'cli.cmd.tool');
toolCommand
    .command('add', 'cli.cmd.tool-add')
    .option('--id <id>', 'cli.opt.id')
    .option('--name <name>', 'cli.opt.name')
    .option('--number <n>', 'cli.opt.number')
    .option('--diameter <mm>', 'cli.opt.diameter')
    .option('--flutes <n>', 'cli.opt.flutes')
    .option('--spindle <rpm>', 'cli.opt.spindle')
    .option('--feed <mm/min>', 'cli.opt.feed')
    .option('--plunge <mm/min>', 'cli.opt.plunge')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'tool add <file>');
            await withDocument(file, async (document) => {
                const feed = requiredNumber(ctx, options, '--feed');
                const id = required(ctx, options, '--id');
                print(
                    await document.addTool({
                        id,
                        name: text(options, 'name') ?? id,
                        number: integer(ctx, options, '--number') ?? 1,
                        diameter: requiredNumber(ctx, options, '--diameter'),
                        flutes: integer(ctx, options, '--flutes') ?? 2,
                        spindle: requiredNumber(ctx, options, '--spindle'),
                        feed,
                        plungeFeed: number(ctx, options, '--plunge') ?? feed / 3,
                    }),
                );
            });
            return 0;
        }),
    );
toolCommand
    .command('remove', 'cli.cmd.tool-remove')
    .option('--id <id>', 'cli.opt.id')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'tool remove <file>');
            await withDocument(file, async (document) => {
                print(await document.removeTool(required(ctx, options, '--id')));
            });
            return 0;
        }),
    );

const operationCommand = cli.command('operation', 'cli.cmd.operation');
operationCommand
    .command('add', 'cli.cmd.operation-add')
    .option('--id <id>', 'cli.opt.id')
    .option('--name <name>', 'cli.opt.name')
    .option('--kind <face|contour|pocket|drill|engrave>', 'cli.opt.kind')
    .option('--tool <id>', 'cli.opt.tool')
    .option('--depth <mm>', 'cli.opt.depth')
    .option('--stepdown <mm>', 'cli.opt.stepdown')
    .option('--stepover <mm>', 'cli.opt.stepover')
    .option('--ramp <mm>', 'cli.opt.ramp')
    .option('--strategy <raster|spiral>', 'cli.opt.strategy')
    .option('--shape <rect|circle>', 'cli.opt.shape')
    .option('--x <mm>', 'cli.opt.x')
    .option('--y <mm>', 'cli.opt.y')
    .option('--width <mm>', 'cli.opt.width')
    .option('--height <mm>', 'cli.opt.height')
    .option('--holes <x,y;x,y>', 'cli.opt.holes')
    .option('--peck <mm>', 'cli.opt.peck')
    .option('--dwell <seconds>', 'cli.opt.dwell')
    .option('--island <x,y,w,h>...', 'cli.opt.island')
    .option('--path <x,y;x,y>', 'cli.opt.path')
    .option('--tabs <count,width,height>', 'cli.opt.tabs')
    .option('--lead <radius,cw|ccw>', 'cli.opt.lead')
    .option('--closed', 'cli.opt.closed')
    .option('--finish', 'cli.opt.finish')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'operation add <file>');
            await withDocument(file, async (document) => {
                const depth = requiredNumber(ctx, options, '--depth');
                const id = required(ctx, options, '--id');
                const holes = text(options, 'holes');
                const pathValue = text(options, 'path');
                const strategy = text(options, 'strategy');
                const shape = text(options, 'shape');
                const tabs = text(options, 'tabs');
                const lead = text(options, 'lead');
                print(
                    await document.addOperation({
                        id,
                        name: text(options, 'name') ?? id,
                        kind: operationKind(required(ctx, options, '--kind')),
                        tool: required(ctx, options, '--tool'),
                        depth,
                        stepdown: number(ctx, options, '--stepdown') ?? depth,
                        stepover: number(ctx, options, '--stepover'),
                        ramp: number(ctx, options, '--ramp'),
                        strategy: strategy === undefined ? undefined : pocketStrategy(strategy),
                        shape: shape === undefined ? undefined : (shape as PocketShape),
                        x: number(ctx, options, '--x') ?? 0,
                        y: number(ctx, options, '--y') ?? 0,
                        width: number(ctx, options, '--width') ?? 0,
                        height: number(ctx, options, '--height') ?? 0,
                        holes: holes === undefined ? [] : parseHoles(holes),
                        path: pathValue === undefined ? [] : parsePath(pathValue),
                        closed: flag(options, 'closed'),
                        islands: parseIslands(values(options, 'island')),
                        peck: number(ctx, options, '--peck'),
                        dwell: number(ctx, options, '--dwell'),
                        tabs: tabs === undefined ? undefined : parseTabs(tabs),
                        lead: lead === undefined ? undefined : parseLead(lead),
                        finish: flag(options, 'finish') ? true : undefined,
                    }),
                );
            });
            return 0;
        }),
    );
operationCommand
    .command('remove', 'cli.cmd.operation-remove')
    .option('--id <id>', 'cli.opt.id')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'operation remove <file>');
            await withDocument(file, async (document) => {
                print(await document.removeOperation(required(ctx, options, '--id')));
            });
            return 0;
        }),
    );

cli
    .command('validate', 'cli.cmd.validate')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'validate <file>');
            await withDocument(file, async (document) => {
                print(await document.validate());
            });
            return 0;
        }),
    );

cli
    .command('toolpaths', 'cli.cmd.toolpaths')
    .option('--out <report.json>', 'cli.opt.out')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'toolpaths <file>');
            await withDocument(file, async (document) => {
                const report = await document.toolpaths();
                const target = text(options, 'out');
                if (target !== undefined) {
                    fs.writeFileSync(target, JSON.stringify(report, null, 2));
                }
                print(report);
            });
            return 0;
        }),
    );

cli
    .command('estimate', 'cli.cmd.estimate')
    .option('--rapid-feed <mm/min>', 'cli.opt.rapid-feed')
    .option('--tool-change <seconds>', 'cli.opt.tool-change')
    .option('--out <estimate.json>', 'cli.opt.out')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'estimate <file>');
            await withDocument(file, async (document) => {
                const rapidFeed = number(ctx, options, '--rapid-feed');
                const toolChangeSeconds = number(ctx, options, '--tool-change');
                const report = await document.estimateCycleTime({
                    ...(rapidFeed === undefined ? {} : { rapidFeed }),
                    ...(toolChangeSeconds === undefined ? {} : { toolChangeSeconds }),
                });
                const target = text(options, 'out');
                if (target !== undefined) {
                    fs.writeFileSync(target, JSON.stringify(report, null, 2));
                }
                print(report);
            });
            return 0;
        }),
    );

cli
    .command('gcode', 'cli.cmd.gcode')
    .option('--out <program.nc>', 'cli.opt.out')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'gcode <file>');
            await withDocument(file, async (document) => {
                const program = await document.gcode();
                const target = text(options, 'out');
                if (target !== undefined) {
                    fs.writeFileSync(target, program);
                }
                process.stdout.write(program);
            });
            return 0;
        }),
    );

cli
    .command('export', 'cli.cmd.export')
    .option('--format <gcode|json|csv|md>', 'cli.opt.format')
    .option('--out <path>', 'cli.opt.out')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'export <file> --format <format> --out <path>');
            await withDocument(file, async (document) => {
                const format = exportFormat(ctx, text(options, 'format') ?? 'gcode');
                print(await document.exportDocument(format, required(ctx, options, '--out')));
            });
            return 0;
        }),
    );

cli
    .command('report', 'cli.cmd.report')
    .actionCtx(async (options, _args, ctx) =>
        guard(ctx, async () => {
            const file = positional(ctx, options, 'report <file>');
            await withDocument(file, async (document) => {
                print(await document.report());
            });
            return 0;
        }),
    );

const entry = process.argv[1];
if (entry !== undefined && import.meta.url === pathToFileURL(entry).href) {
    try {
        process.exitCode = await cli.parse(process.argv);
    } finally {
        await app.dispose();
    }
}
