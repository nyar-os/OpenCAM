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

const drawing = `0
SECTION
2
HEADER
9
$INSUNITS
70
4
0
ENDSEC
0
SECTION
2
ENTITIES
0
LINE
8
outline
10
0
20
0
11
10
21
0
0
CIRCLE
8
holes
10
5
20
5
40
2
0
SPLINE
8
bad
0
ENDSEC
0
SECTION
2
OBJECTS
0
ENDSEC
0
EOF
`;

interface SavedOperation {
    readonly id: string;
    readonly name: string;
    readonly kind: string;
    readonly closed?: boolean;
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
    return fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-dxf-'));
}

test('importDxf writes engrave operations and reports every loss', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = temporaryDirectory();
    try {
        if (!(await addonAvailable(session))) {
            t.skip('the native addon is not installed');
            return;
        }
        const source = path.join(directory, 'part.dxf');
        const target = path.join(directory, 'part.ocam');
        fs.writeFileSync(source, drawing);

        const report = await session.importDxf({ path: target, source, tolerance: 0.05 });
        assert.equal(report.format, 'open-cam-dxf-import');
        assert.equal(report.version, 1);
        assert.equal(report.insUnits, 4);
        assert.equal(report.sourceUnits, 'millimetres');
        assert.equal(report.entityCount, 2);
        assert.equal(report.operationCount, 2);
        assert.equal(report.skippedCount, 1);
        assert.equal(report.tolerance, 0.05);
        assert.equal(report.depth, 1);
        assert.equal(report.stepdown, 1);
        assert.equal(report.document.revision, 0);
        assert.equal(report.document.operations, 2);
        assert.equal(report.document.stock.width, 10);
        assert.ok(Math.abs(report.document.stock.depth - 7) < 0.05);
        assert.ok(Math.abs(report.document.stock.height - (report.document.stock.depth + 5)) < 1e-9);

        assert.ok(report.losses.some((loss) => loss.code === 'import/dxf-unsupported-entity' && loss.entity === 'SPLINE'));
        assert.ok(report.losses.some((loss) => loss.code === 'import/dxf-unsupported-section'));

        const saved = JSON.parse(fs.readFileSync(target, 'utf8')) as { operations: SavedOperation[] };
        const [line, circle] = saved.operations;
        assert.equal(line.id, 'outline-1');
        assert.equal(line.name, 'outline');
        assert.equal(line.kind, 'engrave');
        assert.equal(line.closed ?? false, false);
        assert.deepEqual(line.path, [
            { x: 0, y: 0 },
            { x: 10, y: 0 },
        ]);
        assert.equal(circle.id, 'holes-2');
        assert.equal(circle.name, 'holes');
        assert.equal(circle.closed, true);
        assert.ok(circle.path.length >= 8);
        assert.ok(circle.path.every((point) => Math.abs((point.x - 5) ** 2 + (point.y - 5) ** 2 - 4) < 1e-9));

        await assert.rejects(session.importDxf({ path: target, source }), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'file/exists');
            return true;
        });
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('import dxf is available from the CLI and prints the report', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = temporaryDirectory();
    try {
        if (!(await addonAvailable(session))) {
            t.skip('the native addon is not installed');
            return;
        }
        const source = path.join(directory, 'part.dxf');
        const target = path.join(directory, 'part.ocam');
        fs.writeFileSync(source, drawing);
        const cli = compiledCli();

        const { stdout } = await run(process.execPath, [cli, 'import', 'dxf', target, '--from', source, '--tolerance', '0.01']);
        const report = JSON.parse(stdout) as { entityCount: number; tolerance: number; losses: { code: string }[] };
        assert.equal(report.entityCount, 2);
        assert.equal(report.tolerance, 0.01);
        assert.ok(report.losses.some((loss) => loss.code === 'import/dxf-unsupported-entity'));
        assert.ok(fs.existsSync(target));
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('import dxf fails with the malformed-group code and line number', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = temporaryDirectory();
    try {
        if (!(await addonAvailable(session))) {
            t.skip('the native addon is not installed');
            return;
        }
        const source = path.join(directory, 'broken.dxf');
        const target = path.join(directory, 'broken.ocam');
        fs.writeFileSync(source, '0\nSECTION\n2\nENTITIES\nnot-a-code\n10\n');
        const cli = compiledCli();

        await assert.rejects(
            run(process.execPath, [cli, 'import', 'dxf', target, '--from', source]),
            (error: unknown) => {
                const failure = error as { code?: number; stderr?: string };
                assert.equal(failure.code, 1);
                assert.match(failure.stderr ?? '', /import\/dxf-malformed-group/);
                assert.match(failure.stderr ?? '', /line 5/);
                return true;
            },
        );
        assert.equal(fs.existsSync(target), false);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

function compiledCli(): string {
    const distTest = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    const packageRoot = path.resolve(distTest, '..');
    fs.cpSync(path.join(packageRoot, 'locales'), path.join(distTest, 'locales'), { recursive: true });
    return path.join(distTest, 'src', 'cli.js');
}
