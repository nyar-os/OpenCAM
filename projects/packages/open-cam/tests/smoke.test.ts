import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test, type TestContext } from 'node:test';

import { OpenCAM, OpenCAMError, errorCodes, isOpenCAMError } from '../src/index.js';

test('the session stores its locale and starts empty', () => {
    const session = new OpenCAM({ locale: 'zh-CN' });
    assert.equal(session.locale, 'zh-CN');
    assert.equal(session.disposed, false);
});

test('an empty path is rejected before the addon is touched', async () => {
    const session = new OpenCAM();
    await assert.rejects(session.create(''), (error: unknown) => {
        assert.ok(isOpenCAMError(error));
        assert.equal(error.code, 'session/argument-invalid');
        return true;
    });
    await assert.rejects(session.open(''), (error: unknown) => {
        assert.ok(error instanceof OpenCAMError);
        assert.equal(error.code, 'session/argument-invalid');
        return true;
    });
    await session.dispose();
});

test('a disposed session refuses new documents', async () => {
    const session = new OpenCAM();
    await session.dispose();
    await assert.rejects(session.create('part.ocam'), (error: unknown) => {
        assert.ok(isOpenCAMError(error));
        assert.equal(error.code, 'session/disposed');
        return true;
    });
});

test('the error surface includes the unimplemented capability code', () => {
    assert.ok(errorCodes.includes('capability/not-implemented'));
    assert.ok(errorCodes.includes('tool/in-use'));
    assert.ok(errorCodes.includes('path/outside-stock'));
    assert.ok(errorCodes.includes('operation/island-overlap'));
});

test('a pocket clears around islands and rejects a rejected island without saving', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-islands-'));
    try {
        try {
            await session.capabilities();
        } catch (error) {
            if (isOpenCAMError(error) && error.code === 'capability/not-implemented') {
                t.skip('the native addon is not installed');
                return;
            }
            throw error;
        }

        const folder = fs.mkdtempSync(path.join(directory, 'job-'));
        const file = path.join(folder, 'part.ocam');
        const document = await session.create(file);
        await document.setStock({ width: 120, depth: 100, height: 10 });
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
            id: 'pocket',
            kind: 'pocket',
            tool: 't6',
            depth: 1,
            stepdown: 1,
            stepover: 3,
            x: 10,
            y: 10,
            width: 100,
            height: 70,
            islands: [{ x: 40, y: 35, width: 20, height: 20 }],
        });

        const report = await document.toolpaths();
        const path0 = report.operations[0];
        assert.ok(path0);
        assert.equal(path0.islandCount, 1);
        assert.ok(path0.skippedLength > 0);
        assert.ok(path0.skippedSegments > 0);

        const program = await document.gcode();
        assert.ok(program.includes('G0 X'));

        await assert.rejects(
            document.addOperation({
                id: 'bad',
                kind: 'pocket',
                tool: 't6',
                depth: 1,
                stepdown: 1,
                x: 10,
                y: 10,
                width: 100,
                height: 70,
                islands: [{ x: 5, y: 20, width: 10, height: 10 }],
            }),
            (error: unknown) => {
                assert.ok(isOpenCAMError(error));
                assert.equal(error.code, 'operation/island-outside-pocket');
                return true;
            },
        );
        assert.equal((await document.validate()).ok, true);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});
