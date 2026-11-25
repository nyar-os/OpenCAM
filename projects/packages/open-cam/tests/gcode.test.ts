import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test, type TestContext } from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { OpenCAM, isOpenCAMError } from '../src/index.js';

const run = promisify(execFile);

const program = `
%
G21 G90 G17 G94
(Units: mm)
G0 Z5
T1 M6
S12000 M3
(operation rectangle - engrave)
G0 X0 Y0 Z5
G1 X10 Y0 Z-1 F100
G1 X10 Y5 F100
G1 X0 Y5
G0 Z5
M5
(operation holes - drill)
G0 Z5
T2 M6
S9000 M3
G81 X2 Y2 Z1 R3 F300
G81 X8 Y2 Z1 R3 F300
G80
M5
M30
%
`;

const inchProgram = `G21 G90\nG20\nG1 X10 Y10\n`;

interface SavedOperation {
    readonly id: string;
    readonly name: string;
    readonly kind: string;
    readonly path: readonly { readonly x: number; readonly y: number }[];
}

async function addonAvailable(session: OpenCAM): Promise<boolean> {
    try {
        await session.capabilities();
        return true;
    } catch (error) {
        if (isOpenCAMError(error) && error.code === 'capability/not-implemented') {
            return false;
        }
        throw error;
    }
}

function temporaryDirectory(): string {
    return fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-gcode-'));
}

function compiledCli(): string {
    const distTest = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    const packageRoot = path.resolve(distTest, '..');
    fs.cpSync(path.join(packageRoot, 'locales'), path.join(distTest, 'locales'), { recursive: true });
    return path.join(distTest, 'src', 'cli.js');
}

test('importGcode writes engrave operations and reports every loss', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = temporaryDirectory();
    try {
        if (!(await addonAvailable(session))) {
            t.skip('the native addon is not installed');
            return;
        }
        const source = path.join(directory, 'part.nc');
        const target = path.join(directory, 'part.ocam');
        fs.writeFileSync(source, program);

        const report = await session.importGcode({ path: target, source, depth: 2, stepdown: 2, tolerance: 0.05 });
        assert.equal(report.format, 'open-cam-gcode-import');
        assert.equal(report.version, 1);
        assert.equal(report.units, 'mm');
        assert.equal(report.depth, 2);
        assert.equal(report.stepdown, 2);
        assert.equal(report.moveCount, 3);
        assert.equal(report.arcCount, 0);
        assert.equal(report.operationCount, 1);
        assert.equal(report.skippedCount, 2);
        assert.ok(report.losses.every((loss) => loss.code === 'import/gcode-unsupported-gcode' && loss.word === 'G81'));
        assert.equal(report.document.revision, 0);
        assert.equal(report.document.operations, 1);
        assert.equal(report.document.stock.width, 10);
        assert.equal(report.document.stock.depth, 5);
        assert.equal(report.document.stock.height, 10);

        const saved = JSON.parse(fs.readFileSync(target, 'utf8')) as { operations: SavedOperation[] };
        const [rectangle] = saved.operations;
        assert.equal(rectangle.id, 'gcode-1');
        assert.equal(rectangle.name, 'gcode-1');
        assert.equal(rectangle.kind, 'engrave');
        assert.deepEqual(rectangle.path, [
            { x: 0, y: 0 },
            { x: 10, y: 0 },
            { x: 10, y: 5 },
            { x: 0, y: 5 },
        ]);

        await assert.rejects(session.importGcode({ path: target, source }), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'file/exists');
            return true;
        });
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('importGcode rejects inch and incremental modes without writing', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = temporaryDirectory();
    try {
        if (!(await addonAvailable(session))) {
            t.skip('the native addon is not installed');
            return;
        }
        const source = path.join(directory, 'inch.nc');
        const target = path.join(directory, 'inch.ocam');
        fs.writeFileSync(source, inchProgram);
        await assert.rejects(session.importGcode({ path: target, source }), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'import/gcode-unsupported-mode');
            assert.match(error.message, /line 2/);
            return true;
        });
        assert.equal(fs.existsSync(target), false);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('import gcode is available from the CLI and prints the report', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = temporaryDirectory();
    try {
        if (!(await addonAvailable(session))) {
            t.skip('the native addon is not installed');
            return;
        }
        const source = path.join(directory, 'part.nc');
        const target = path.join(directory, 'part.ocam');
        fs.writeFileSync(source, program);
        const cli = compiledCli();

        const { stdout } = await run(process.execPath, [cli, 'import', 'gcode', target, '--from', source, '--depth', '2']);
        const report = JSON.parse(stdout) as {
            moveCount: number;
            operationCount: number;
            losses: { code: string; line: number }[];
        };
        assert.equal(report.moveCount, 3);
        assert.equal(report.operationCount, 1);
        assert.ok(report.losses.some((loss) => loss.code === 'import/gcode-unsupported-gcode' && loss.line === 19));
        assert.ok(fs.existsSync(target));
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});
