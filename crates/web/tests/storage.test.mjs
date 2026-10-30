import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const load = async name => new AsyncFunction('dioxus', 'localStorage', await readFile(new URL(`../src/${name}`, import.meta.url), 'utf8'));
const store = (initial = {}) => {
    const data = { ...initial };
    return { data, getItem: k => (k in data ? data[k] : null), setItem: (k, v) => { data[k] = String(v); } };
};

test('get returns a saved valid slug, otherwise empty string', async () => {
    const get = await load('storage_get.js');
    assert.equal(await get(null, store({ 'hermes-dash.board': 'zaha' })), 'zaha');
    assert.equal(await get(null, store({})), '');
    assert.equal(await get(null, store({ 'hermes-dash.board': '../etc' })), '');
    assert.equal(await get(null, store({ 'hermes-dash.board': 'a'.repeat(65) })), '');
});

test('get survives blocked storage', async () => {
    const get = await load('storage_get.js');
    const blocked = { getItem() { throw new Error('denied'); } };
    assert.equal(await get(null, blocked), '');
});

test('set saves only valid slugs and never throws', async () => {
    const set = await load('storage_set.js');
    const s = store();
    const send = slug => set({ recv: async () => slug }, s);
    assert.equal(await send('reporty'), true);
    assert.equal(s.data['hermes-dash.board'], 'reporty');
    await send('bad slug;alert(1)');
    assert.equal(s.data['hermes-dash.board'], 'reporty');
    const blocked = { setItem() { throw new Error('denied'); } };
    assert.equal(await set({ recv: async () => 'zaha' }, blocked), true);
});
