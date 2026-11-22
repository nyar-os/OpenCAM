#!/usr/bin/env node
/**
 * Command-line assembly only.
 *
 * Every command maps options onto one session operation of `OpenCAM`; editing,
 * validation, toolpath generation and export stay in the native core. The CLI
 * owns argument parsing, help text, printing and exit codes, and nothing else.
 */

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import type { CamDocument } from './document.js';
import { NotImplementedError, OpenCAMError, type OpenCAMErrorCode } from './errors.js';
import { OpenCAM } from './open-cam.js';
import type {
    ExportFormat,
    Hole,
    Island,
    OperationKind,
    PathPoint,
    PocketShape,
    PocketStrategy,
    TabSettings,
} from './types.js';

interface CliLocale {
    readonly cli: {
        readonly intro: string;
        readonly usage: readonly string[];
        readonly exit: string;
        readonly message: Readonly<Record<string, string>>;
        readonly error: Readonly<Record<string, string>>;
    };
}

const here = path.dirname(fileURLToPath(import.meta.url));
const locale = JSON.parse(
    fs.readFileSync(path.join(here, '..', 'locales', 'en-US', 'cli.json'), 'utf8'),
) as CliLocale;

function fill(template: string, values: Readonly<Record<string, string>> = {}): string {
    return template.replace(/\{(\w+)\}/g, (_match, key: string) => values[key] ?? `{${key}}`);
}

function failure(code: OpenCAMErrorCode, key: string, values?: Readonly<Record<string, string>>): OpenCAMError {
    return new OpenCAMError(code, fill(locale.cli.error[key] ?? key, values));
}

function printUsage(): void {
    process.stdout.write(`${locale.cli.intro}\n\n${locale.cli.usage.join('\n')}\n\n${locale.cli.exit}\n`);
}

interface Parsed {
    readonly positionals: readonly string[];
    readonly flags: ReadonlySet<string>;
    readonly options: ReadonlyMap<string, string>;
    readonly repeated: ReadonlyMap<string, readonly string[]>;
}

function parse(args: readonly string[], valueOptions: readonly string[], flagOptions: readonly string[]): Parsed {
    const positionals: string[] = [];
    const flags = new Set<string>();
    const options = new Map<string, string>();
    const repeated = new Map<string, string[]>();
    let index = 0;
    while (index < args.length) {
        const argument = args[index] as string;
        if (argument.startsWith('--')) {
            if (flagOptions.includes(argument)) {
                flags.add(argument);
                index += 1;
            } else if (valueOptions.includes(argument)) {
                const value = args[index + 1];
                if (value === undefined || value.startsWith('--')) {
                    throw failure('cli/missing-value', 'missingValue', { option: argument });
                }
                options.set(argument, value);
                const entries = repeated.get(argument);
                if (entries === undefined) {
                    repeated.set(argument, [value]);
                } else {
                    entries.push(value);
                }
                index += 2;
            } else {
                throw failure('cli/unknown-option', 'unknownOption', { option: argument });
            }
        } else {
            positionals.push(argument);
            index += 1;
        }
    }
    return { positionals, flags, options, repeated };
}

function positional(parsed: Parsed, index: number, usage: string): string {
    const value = parsed.positionals[index];
    if (value === undefined) {
        throw failure('cli/missing-argument', 'missingArgument', { usage });
    }
    return value;
}

function required(parsed: Parsed, name: string): string {
    const value = parsed.options.get(name);
    if (value === undefined) {
        throw failure('cli/missing-option', 'missingOption', { option: name });
    }
    return value;
}

function number(parsed: Parsed, name: string): number | undefined {
    const raw = parsed.options.get(name);
    if (raw === undefined) {
        return undefined;
    }
    const value = Number(raw);
    if (!Number.isFinite(value)) {
        throw failure('cli/invalid-number', 'invalidNumber', { option: name, value: raw });
    }
    return value;
}

function requiredNumber(parsed: Parsed, name: string): number {
    const value = number(parsed, name);
    if (value === undefined) {
        throw failure('cli/missing-option', 'missingOption', { option: name });
    }
    return value;
}

function integer(parsed: Parsed, name: string): number | undefined {
    const value = number(parsed, name);
    if (value === undefined) {
        return undefined;
    }
    if (!Number.isInteger(value) || value < 0) {
        throw failure('cli/invalid-number', 'invalidNumber', { option: name, value: String(value) });
    }
    return value;
}

function exportFormat(value: string): ExportFormat {
    if (value === 'gcode' || value === 'nc' || value === 'tap') {
        return 'gcode';
    }
    if (value === 'json' || value === 'csv' || value === 'md') {
        return value;
    }
    if (value === 'step' || value === 'stp' || value === 'dxf') {
        throw new NotImplementedError(`${value} export`);
    }
    throw failure('cli/unknown-format', 'unknownFormat', { format: value });
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

function print(value: unknown): void {
    console.log(JSON.stringify(value, null, 2));
}

async function withSession<T>(work: (session: OpenCAM) => Promise<T>): Promise<T> {
    const session = new OpenCAM();
    try {
        return await work(session);
    } finally {
        await session.dispose();
    }
}

async function dispatch(args: readonly string[]): Promise<number> {
    const command = args[0];
    if (command === undefined) {
        printUsage();
        return 0;
    }
    switch (command) {
        case '-h':
        case '--help':
        case 'help':
            printUsage();
            return 0;
        case 'version':
            console.log('0.0.0');
            return 0;
        case 'capabilities':
            return withSession(async (session) => {
                print(await session.capabilities());
                return 0;
            });
        case 'create':
            return withSession(async (session) => {
                const file = positional(parse(args.slice(1), [], []), 0, 'create <file>');
                print(await session.create(file).then((document) => document.save()));
                return 0;
            });
        case 'stock':
            return editDocument(args.slice(1), 'stock <file>', ['--width', '--depth', '--height'], [], async (document, parsed) => {
                print(
                    await document.setStock({
                        width: requiredNumber(parsed, '--width'),
                        depth: requiredNumber(parsed, '--depth'),
                        height: requiredNumber(parsed, '--height'),
                    }),
                );
            });
        case 'tool':
            return toolCommand(args.slice(1));
        case 'operation':
            return operationCommand(args.slice(1));
        case 'validate':
            return readDocument(args.slice(1), 'validate <file>', [], [], async (document) => {
                print(await document.validate());
            });
        case 'toolpaths':
            return readDocument(args.slice(1), 'toolpaths <file>', ['--out'], [], async (document, parsed) => {
                const report = await document.toolpaths();
                const target = parsed.options.get('--out');
                if (target !== undefined) {
                    fs.writeFileSync(target, JSON.stringify(report, null, 2));
                }
                print(report);
            });
        case 'gcode':
            return readDocument(args.slice(1), 'gcode <file>', ['--out'], [], async (document, parsed) => {
                const program = await document.gcode();
                const target = parsed.options.get('--out');
                if (target !== undefined) {
                    fs.writeFileSync(target, program);
                }
                process.stdout.write(program);
            });
        case 'export':
            return readDocument(args.slice(1), 'export <file> --format <format> --out <path>', ['--format', '--out'], [], async (document, parsed) => {
                const format = exportFormat(parsed.options.get('--format') ?? 'gcode');
                print(await document.exportDocument(format, required(parsed, '--out')));
            });
        case 'report':
            return readDocument(args.slice(1), 'report <file>', [], [], async (document) => {
                print(await document.report());
            });
        default:
            throw new OpenCAMError('cli/unknown-command', `unknown command \`${command}\``);
    }
}

async function toolCommand(args: readonly string[]): Promise<number> {
    const action = args[0] ?? '';
    if (action === 'add') {
        return editDocument(
            args.slice(1),
            'tool add <file>',
            ['--id', '--name', '--number', '--diameter', '--flutes', '--spindle', '--feed', '--plunge'],
            [],
            async (document, parsed) => {
                const feed = requiredNumber(parsed, '--feed');
                const id = required(parsed, '--id');
                print(
                    await document.addTool({
                        id,
                        name: parsed.options.get('--name') ?? id,
                        number: integer(parsed, '--number') ?? 1,
                        diameter: requiredNumber(parsed, '--diameter'),
                        flutes: integer(parsed, '--flutes') ?? 2,
                        spindle: requiredNumber(parsed, '--spindle'),
                        feed,
                        plungeFeed: number(parsed, '--plunge') ?? feed / 3,
                    }),
                );
            },
        );
    }
    if (action === 'remove') {
        return editDocument(args.slice(1), 'tool remove <file>', ['--id'], [], async (document, parsed) => {
            print(await document.removeTool(required(parsed, '--id')));
        });
    }
    throw failure('cli/missing-argument', 'missingArgument', { usage: 'tool add|remove <file> ...' });
}

async function operationCommand(args: readonly string[]): Promise<number> {
    const action = args[0] ?? '';
    if (action === 'add') {
        return editDocument(
            args.slice(1),
            'operation add <file>',
            [
                '--id', '--name', '--kind', '--tool', '--depth', '--stepdown', '--stepover',
                '--strategy', '--shape', '--x', '--y', '--width', '--height', '--holes', '--peck', '--island', '--path',
                '--tabs',
            ],
            ['--closed'],
            async (document, parsed) => {
                const depth = requiredNumber(parsed, '--depth');
                const id = required(parsed, '--id');
                const holes = parsed.options.get('--holes');
                const path = parsed.options.get('--path');
                const strategy = parsed.options.get('--strategy');
                const shape = parsed.options.get('--shape');
                const tabs = parsed.options.get('--tabs');
                print(
                    await document.addOperation({
                        id,
                        name: parsed.options.get('--name') ?? id,
                        kind: operationKind(required(parsed, '--kind')),
                        tool: required(parsed, '--tool'),
                        depth,
                        stepdown: number(parsed, '--stepdown') ?? depth,
                        stepover: number(parsed, '--stepover'),
                        strategy: strategy === undefined ? undefined : pocketStrategy(strategy),
                        shape: shape === undefined ? undefined : (shape as PocketShape),
                        x: number(parsed, '--x') ?? 0,
                        y: number(parsed, '--y') ?? 0,
                        width: number(parsed, '--width') ?? 0,
                        height: number(parsed, '--height') ?? 0,
                        holes: holes === undefined ? [] : parseHoles(holes),
                        path: path === undefined ? [] : parsePath(path),
                        closed: parsed.flags.has('--closed'),
                        islands: parseIslands(parsed.repeated.get('--island') ?? []),
                        peck: number(parsed, '--peck'),
                        tabs: tabs === undefined ? undefined : parseTabs(tabs),
                    }),
                );
            },
        );
    }
    if (action === 'remove') {
        return editDocument(args.slice(1), 'operation remove <file>', ['--id'], [], async (document, parsed) => {
            print(await document.removeOperation(required(parsed, '--id')));
        });
    }
    throw failure('cli/missing-argument', 'missingArgument', { usage: 'operation add|remove <file> ...' });
}

async function editDocument(
    args: readonly string[],
    usage: string,
    valueOptions: readonly string[],
    flagOptions: readonly string[],
    work: (document: CamDocument, parsed: Parsed) => Promise<void>,
): Promise<number> {
    const parsed = parse(args, valueOptions, flagOptions);
    const file = positional(parsed, 0, usage);
    return withSession(async (session) => {
        const document = await session.open(file);
        await work(document, parsed);
        return 0;
    });
}

async function readDocument(
    args: readonly string[],
    usage: string,
    valueOptions: readonly string[],
    flagOptions: readonly string[],
    work: (document: CamDocument, parsed: Parsed) => Promise<void>,
): Promise<number> {
    return editDocument(args, usage, valueOptions, flagOptions, work);
}

const args = process.argv.slice(2);
dispatch(args)
    .then((code) => {
        process.exitCode = code;
    })
    .catch((error: unknown) => {
        if (error instanceof OpenCAMError) {
            process.stderr.write(`${error.code}: ${error.message}\n`);
            process.exitCode = error.code.startsWith('capability/') ? 2 : 1;
        } else {
            process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
            process.exitCode = 1;
        }
    });
