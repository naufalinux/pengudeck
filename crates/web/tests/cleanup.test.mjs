import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const path = new URL('../src/cleanup.js', import.meta.url);
async function bridge(operation, payload, fetch) {
  const code = fs.readFileSync(path, 'utf8');
  const result = await vm.runInNewContext(`(async () => { ${code} })()`, { fetch, AbortSignal, dioxus: { recv: async () => [operation, payload] } });
  assert.equal(typeof result, 'string', 'document::eval.join must receive the returned value');
  return JSON.parse(result);
}
test('each operation uses its fixed endpoint, method and CSRF marker', async () => {
  const expected = {
    status: ['GET', 'http://127.0.0.1:8788/api/settings/cleanup'],
    review: ['POST', 'http://127.0.0.1:8788/api/settings/cleanup/review'],
    save: ['PUT', 'http://127.0.0.1:8788/api/settings/cleanup'],
    run: ['POST', 'http://127.0.0.1:8788/api/settings/cleanup/run'],
  };
  for (const [operation, [method, url]] of Object.entries(expected)) {
    const result = await bridge(operation, { a: 1 }, async (seen, options) => {
      assert.equal(seen, url);
      assert.equal(options.method, method);
      assert.equal(options.credentials, 'omit');
      assert.equal(options.redirect, 'error');
      assert.equal(options.headers['X-Pengudeck-Request'], 'retention-v1');
      assert.equal(options.body, method === 'GET' ? undefined : '{"a":1}');
      return { ok: true, json: async () => ({ ok: 1 }) };
    });
    assert.deepEqual(result, { ok: true, data: { ok: 1 } });
  }
});
test('unknown operation never reaches the network', async () => {
  for (const operation of ['../evil', 'tick', 'track', 'apply']) {
    const result = await bridge(operation, {}, () => { throw new Error('network must not run'); });
    assert.deepEqual(result, { ok: false, code: 'cleanup_failed' });
  }
});
test('only fixed lowercase error categories are forwarded', async () => {
  const fail = body => async () => ({ ok: false, json: async () => body });
  assert.deepEqual(await bridge('save', {}, fail({ error: 'confirmation_required' })), { ok: false, code: 'confirmation_required' });
  for (const body of [{ error: 'SECRET /home/gnu' }, { error: 42 }, {}, null]) {
    const result = await bridge('save', {}, fail(body));
    assert.deepEqual(result, { ok: false, code: 'cleanup_failed' });
  }
  const broken = await bridge('status', {}, async () => ({ ok: false, json: async () => { throw new Error('SECRET'); } }));
  assert.deepEqual(broken, { ok: false, code: 'cleanup_failed' });
});
test('malformed success bodies and network failures are sanitized', async () => {
  for (const fetch of [async () => ({ ok: true, json: async () => [] }), async () => ({ ok: true, json: async () => { throw new Error('SECRET'); } }), async () => { throw new Error('SECRET'); }]) {
    const result = await bridge('status', {}, fetch);
    assert.deepEqual(result, { ok: false, code: 'cleanup_failed' });
  }
});
