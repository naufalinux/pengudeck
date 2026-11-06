import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const path = new URL('../src/retention.js', import.meta.url);
async function bridge(operation, payload, fetch) {
  assert.ok(fs.existsSync(path), 'retention browser bridge must exist');
  const code = fs.readFileSync(path, 'utf8');
  const result = await vm.runInNewContext(`(async () => { ${code} })()`, { fetch, AbortSignal, dioxus: { recv: async () => [operation, payload] } });
  assert.equal(typeof result, 'string', 'document::eval.join must receive the returned value');
  return JSON.parse(result);
}
test('preview uses fixed endpoint and CSRF marker, with credentials omitted', async () => {
  const result = await bridge('preview', {}, async (url, options) => {
    assert.equal(url, 'http://127.0.0.1:8788/api/settings/retention/preview');
    assert.equal(options.method, 'POST');
    assert.equal(options.credentials, 'omit');
    assert.equal(options.headers['X-Pengudeck-Request'], 'retention-v1');
    return { ok: true, json: async () => ({ dry_run: true, archive: [], purge: [] }) };
  });
  assert.equal(result.ok, true);
  assert.equal(result.data.dry_run, true);
});
test('unknown operation never reaches the network', async () => {
  const result = await bridge('../evil', {}, () => { throw new Error('network must not run'); });
  assert.equal(result.ok, false);
});
test('HTTP and malformed JSON errors are sanitized', async () => {
  for (const fetch of [async () => ({ ok: false }), async () => ({ ok: true, json: async () => { throw new Error('SECRET'); } })]) {
    const result = await bridge('get', {}, fetch);
    assert.equal(result.ok, false);
    assert.ok(!JSON.stringify(result).includes('SECRET'));
  }
});
