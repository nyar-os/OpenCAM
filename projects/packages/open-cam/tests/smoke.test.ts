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

test('a circular contour with arcs renders G3 half circles', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-arcs-'));
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

        assert.equal((await session.capabilities()).operations.circleArcs, 'supported');

        const folder = fs.mkdtempSync(path.join(directory, 'job-'));
        const document = await session.create(path.join(folder, 'part.ocam'));
        await document.setStock({ width: 120, depth: 100, height: 10 });
        await document.addTool({ id: 't6', diameter: 6, spindle: 12000, feed: 1200 });
        await document.addOperation({
            id: 'boss',
            kind: 'contour',
            tool: 't6',
            depth: 2,
            stepdown: 2,
            x: 20,
            y: 20,
            width: 40,
            height: 40,
            shape: 'circle',
            arcs: 'arc',
        });

        const report = await document.toolpaths();
        const path0 = report.operations[0] ?? assert.fail('missing operation');
        const arcs = path0.segments.filter((segment) => segment.kind === 'arc');
        assert.equal(arcs.length, 2);
        assert.equal(arcs[0]?.arc?.clockwise, false);

        const program = await document.gcode();
        assert.ok(program.includes('G3 X17 Y40 Z8 I-23 J0 F1200'));
        assert.ok(program.includes('G3 X63 Y40 Z8 I23 J0 F1200'));

        await assert.rejects(
            document.addOperation({ id: 'face', kind: 'face', tool: 't6', depth: 1, stepdown: 1, arcs: 'arc' }),
            (error: unknown) => {
                assert.ok(isOpenCAMError(error));
                assert.equal(error.code, 'operation/invalid');
                return true;
            },
        );
        assert.equal((await document.validate()).ok, true);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('the tool table exports and imports a portable library', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-tools-'));
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

        assert.equal((await session.capabilities()).io.toolLibrary, 'supported');

        const folder = fs.mkdtempSync(path.join(directory, 'shop-'));
        const document = await session.create(path.join(folder, 'part.ocam'));
        await document.setStock({ width: 120, depth: 100, height: 10 });
        await document.addTool({ id: 't6', diameter: 6, spindle: 12000, feed: 1200 });
        await document.addTool({ id: 't3', diameter: 3, spindle: 18000, feed: 800 });
        await document.addOperation({ id: 'face', kind: 'face', tool: 't6', depth: 1, stepdown: 1 });

        const table = await document.listTools();
        assert.equal(table.toolCount, 2);
        assert.equal(table.unusedCount, 1);
        assert.deepEqual(table.tools[0]?.usedBy, ['face']);
        assert.equal(table.tools[1]?.used, false);

        const library = path.join(folder, 'shop-tools.json');
        const exported = await document.exportTools(library);
        assert.equal(exported.format, 'tools');
        assert.equal(exported.bytes > 0, true);
        assert.equal(fs.existsSync(library), true);

        const target = await session.create(path.join(folder, 'second.ocam'));
        await target.setStock({ width: 120, depth: 100, height: 10 });
        const imported = await target.importTools(library);
        assert.deepEqual(imported.imported, ['t6', 't3']);
        assert.equal(imported.toolCount, 2);
        assert.equal(target.tools, 2);

        await assert.rejects(target.importTools(library), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'tool/duplicate-id');
            return true;
        });
        await assert.rejects(target.importTools(path.join(folder, 'missing.json')), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'io/read');
            return true;
        });
        assert.equal((await target.validate()).ok, true);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('a contour with lead arcs reports arc moves and renders G2', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-lead-'));
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

        const capabilities = await session.capabilities();
        assert.equal(capabilities.operations.contourLead, 'supported');

        const file = path.join(fs.mkdtempSync(path.join(directory, 'job-')), 'part.ocam');
        const document = await session.create(file);
        await document.setStock({ width: 120, depth: 100, height: 10 });
        await document.addTool({ id: 't6', diameter: 6, spindle: 12000, feed: 1200 });
        await document.addOperation({
            id: 'profile',
            kind: 'contour',
            tool: 't6',
            depth: 2,
            stepdown: 2,
            x: 20,
            y: 20,
            width: 40,
            height: 20,
            lead: { radius: 5, direction: 'cw' },
        });

        const report = await document.toolpaths();
        const arcs = (report.operations[0] ?? assert.fail('missing operation')).segments.filter((segment) => segment.kind === 'arc');
        assert.equal(arcs.length, 2);
        const leadIn = arcs[0] ?? assert.fail('missing lead-in arc');
        assert.ok(leadIn.arc);
        assert.equal(leadIn.arc.clockwise, true);
        assert.equal(leadIn.to.x, 17);
        assert.equal(leadIn.to.y, 17);
        assert.equal(leadIn.arc.centre.x, 17);
        assert.equal(leadIn.arc.centre.y, 12);

        const program = await document.gcode();
        assert.ok(program.includes('G2 X17 Y17 Z8 I0 J5 F1200'));
        assert.ok(program.includes('G2 X7 Y17 Z8 I-5 J0 F1200'));

        await assert.rejects(
            document.addOperation({
                id: 'bad',
                kind: 'contour',
                tool: 't6',
                depth: 2,
                stepdown: 2,
                x: 20,
                y: 20,
                width: 40,
                height: 20,
                lead: { radius: 0, direction: 'cw' },
            }),
            (error: unknown) => {
                assert.ok(isOpenCAMError(error));
                assert.equal(error.code, 'operation/invalid');
                return true;
            },
        );
        assert.equal((await document.validate()).ok, true);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('gcode post options add a program number, line numbers and coolant', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-post-'));
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

        assert.equal((await session.capabilities()).io.gcodePost, 'supported');

        const folder = fs.mkdtempSync(path.join(directory, 'job-'));
        const document = await session.create(path.join(folder, 'part.ocam'));
        await document.setStock({ width: 120, depth: 100, height: 10 });
        await document.addTool({ id: 't6', diameter: 6, spindle: 12000, feed: 1200 });
        await document.addOperation({ id: 'face', kind: 'face', tool: 't6', depth: 1, stepdown: 1, stepover: 4 });

        const plain = await document.gcode();
        assert.equal(plain.includes('M8'), false);

        const program = await document.gcode({ programNumber: 1001, lineNumbers: true, coolant: 'flood' });
        const lines = program.split('\n');
        assert.equal(lines[0], '%');
        assert.equal(lines[1], 'N10 O1001');
        assert.ok(lines[2]?.startsWith('N20 '), `third line was \`${lines[2]}\``);
        assert.ok(lines[3]?.startsWith('N30 '), `fourth line was \`${lines[3]}\``);
        assert.equal(program.split('M8').length - 1, 1);
        assert.equal(program.split('M9').length - 1, 1);
        assert.ok(program.indexOf('S12000 M3') < program.indexOf('M8'));

        await assert.rejects(document.gcode({ coolant: 'high-pressure' as never }), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'post/invalid');
            return true;
        });
        await assert.rejects(document.gcode({ lineIncrement: 0 }), (error: unknown) => {
            assert.ok(isOpenCAMError(error));
            assert.equal(error.code, 'post/invalid');
            return true;
        });
        assert.equal((await document.validate()).ok, true);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});

test('sequencing groups operations by tool and applies the plan', async (t: TestContext) => {
    const session = new OpenCAM();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'open-cam-sequence-'));
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

        assert.equal((await session.capabilities()).operations.sequence, 'supported');

        const folder = fs.mkdtempSync(path.join(directory, 'job-'));
        const document = await session.create(path.join(folder, 'part.ocam'));
        await document.setStock({ width: 120, depth: 100, height: 10 });
        await document.addTool({ id: 't6', number: 1, diameter: 6, spindle: 12000, feed: 1200 });
        await document.addTool({ id: 't3', number: 2, diameter: 3, spindle: 18000, feed: 800 });
        await document.addOperation({ id: 'face', kind: 'face', tool: 't6', depth: 1, stepdown: 1, stepover: 4 });
        await document.addOperation({
            id: 'holes',
            kind: 'drill',
            tool: 't3',
            depth: 3,
            stepdown: 3,
            holes: [{ x: 20, y: 20 }],
        });
        await document.addOperation({
            id: 'floor',
            kind: 'pocket',
            tool: 't6',
            depth: 1,
            stepdown: 1,
            stepover: 3,
            x: 10,
            y: 10,
            width: 40,
            height: 30,
        });

        const plan = await document.sequence();
        assert.equal(plan.format, 'open-cam-sequence');
        assert.equal(plan.operationCount, 3);
        assert.equal(plan.beforeToolChanges, 2);
        assert.equal(plan.afterToolChanges, 1);
        assert.equal(plan.unchanged, false);
        assert.equal(plan.applied, false);
        assert.deepEqual(
            plan.operations.map((entry) => entry.operation),
            ['face', 'floor', 'holes'],
        );
        assert.deepEqual(plan.groups[0]?.operations, ['face', 'floor']);
        assert.deepEqual(plan.groups[1]?.operations, ['holes']);

        const revision = document.revision;
        const applied = await document.applySequence();
        assert.equal(applied.applied, true);
        assert.equal(applied.revision, revision + 1);
        assert.equal(document.revision, revision + 1);

        const settled = await document.sequence();
        assert.equal(settled.unchanged, true);

        const program = await document.gcode();
        assert.equal(program.split('T1 M6').length - 1, 2);
        assert.ok(program.lastIndexOf('T1 M6') < program.indexOf('T2 M6'));

        assert.equal((await document.validate()).ok, true);
    } finally {
        await session.dispose();
        fs.rmSync(directory, { recursive: true, force: true });
    }
});
