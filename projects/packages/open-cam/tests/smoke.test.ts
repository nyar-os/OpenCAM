import assert from 'node:assert/strict';
import { test } from 'node:test';

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
});
