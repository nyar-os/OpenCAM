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

async function addonAvailable(context: TestContext, session: OpenCAM): Promise<boolean> {
    try {
        await session.capabilities();
        return true;
    } catch (error) {
        if (isOpenCAMError(error) && error.code === 'capability/not-implemented') {
            context.skip('the native addon is not installed');
            return false;
        }
        throw error;
    }
}

async function sample(session: OpenCAM, file: string): Promise<void> {
    const document = await session.create(file);
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
    await document.addOperation({
        id: 'profile',
        kind: 'contour',
        tool: 't6',
        depth: 3,
        stepdown: 1.5,
        x: 20,
        y: 20,
        width: 40,
        height: 30,
    });
    await document.addOperation({
        id: '槽',
        kind: 'engrave',
        tool: 't6',
        depth: 0.5,
        path: [{ x: 10, y: 10 }, { x: 30, y: 10 }],
    });
}

test('the SDK renders SVG and PDF toolpath drawings', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-drawing-'));
    try {
        if (!(await addonAvailable(t, session))) {
            return;
        }
        const file = path.join(directory, 'job.ocam');
        await sample(session, file);

        const document = await session.open(file);
        const vector = await document.renderDrawing('svg');
        assert.equal(vector.format, 'svg');
        assert.equal(vector.dropped, 0);
        assert.ok(vector.text.startsWith('<?xml version="1.0" encoding="UTF-8"?>'));
        assert.ok(vector.text.includes('data-stock="true"'));
        assert.ok(vector.text.includes('data-operation="profile"'));
        assert.ok(vector.text.includes('data-legend="true"'));
        assert.ok(vector.text.includes('data-title-block="true"'));
        assert.ok(vector.text.includes('<polyline'));
        assert.ok(vector.text.includes('stroke-dasharray="1.5 1"'));
        assert.ok(vector.text.includes('槽'), 'SVG keeps Unicode');

        const sheet = await document.renderDrawing('pdf');
        assert.equal(sheet.format, 'pdf');
        assert.ok(sheet.dropped > 0, 'the non-ASCII id is reported as a loss');
        assert.ok(sheet.text.startsWith('%PDF-1.4'));
        assert.ok(sheet.text.includes('/MediaBox [0 0 842 595]'));
        assert.ok(sheet.text.includes('/Count 1'));
        assert.ok(sheet.text.endsWith('%%EOF\n'));

        const target = path.join(directory, 'part.svg');
        const report = await document.exportDocument('svg', target);
        assert.equal(report.format, 'svg');
        assert.equal(report.dropped, 0);
        assert.equal(fs.readFileSync(target, 'utf8'), vector.text);
    } finally {
        await session.dispose();
    }
});

test('the CLI prints a drawing to stdout or writes --out', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-drawing-cli-'));
    try {
        if (!(await addonAvailable(t, session))) {
            return;
        }
        const file = path.join(directory, 'job.ocam');
        await sample(session, file);
        const cli = compiledCli();

        const { stdout } = await run(process.execPath, [cli, 'export', file, '--format', 'svg']);
        assert.ok(stdout.startsWith('<?xml'));
        assert.ok(stdout.includes('data-title-block="true"'));
        assert.ok(stdout.includes('data-operation="profile"'));

        const target = path.join(directory, 'sheet.pdf');
        const { stdout: summary } = await run(process.execPath, [
            cli,
            'export',
            file,
            '--format',
            'pdf',
            '--out',
            target,
        ]);
        const report = JSON.parse(summary) as { format: string; bytes: number; dropped: number };
        assert.equal(report.format, 'pdf');
        assert.ok(report.bytes > 0);
        assert.ok(report.dropped > 0);
        assert.ok(fs.readFileSync(target, 'utf8').startsWith('%PDF-1.4'));
    } finally {
        await session.dispose();
    }
});

function compiledCli(): string {
    const distTest = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    const packageRoot = path.resolve(distTest, '..');
    fs.cpSync(path.join(packageRoot, 'locales'), path.join(distTest, 'locales'), { recursive: true });
    return path.join(distTest, 'src', 'cli.js');
}
