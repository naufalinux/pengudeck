import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { test } from 'node:test';

// Dioxus executes eval bodies in an AsyncFunction; exercise that exact program.
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const program = new AsyncFunction('dioxus', 'fetch', await readFile(new URL('../src/fetch.js', import.meta.url), 'utf8'));
const statuses = ['todo', 'ready', 'running', 'blocked', 'review', 'done'];
const emptyBoard = () => ({ columns: statuses.map(status => ({ status, tasks: [] })) });
const run = (kind, response) => program({ recv: async () => ['http://127.0.0.1:8787/api/boards', kind] }, async (url, options) => {
    assert.equal(url, 'http://127.0.0.1:8787/api/boards');
    assert.equal(options.method, 'GET');
    assert.equal(options.credentials, 'omit');
    assert.equal(options.redirect, 'error');
    assert.ok(options.signal instanceof AbortSignal);
    return response;
});
const ok = body => ({ status: 200, json: async () => body });

test('board list preserves order, accepts only slugs, removes duplicates', async () => {
    assert.deepEqual(await run('boards', ok(['team-a', 'team_b', 'team-a'])), [200, ['team-a', 'team_b'], []]);
});
test('empty board list is successful', async () => {
    assert.deepEqual(await run('boards', ok([])), [200, [], []]);
});
test('bad board payloads are not displayed', async () => {
    for (const data of [{ boards: [] }, null, [''], ['../secret'], ['a/b'], ['é'], [3]]) {
        assert.deepEqual(await run('boards', ok(data)), [1, [], []]);
    }
});
test('all six empty columns are valid', async () => {
    assert.deepEqual(await run('tasks', ok(emptyBoard())), [200, [], []]);
});
test('card values and assignee null cross the bridge as data, not executable code', async () => {
    const data = emptyBoard();
    const task = { id: 't_card', title: '<script>secret</script>', status: 'running', assignee: null };
    data.columns[2].tasks.push(task);
    assert.deepEqual(await run('tasks', ok(data)), [200, [], [task]]);
});
test('malformed columns and mismatched tasks are rejected', async () => {
    const wrongOrder = emptyBoard(); wrongOrder.columns.reverse();
    const badTasks = emptyBoard(); badTasks.columns[0].tasks = {};
    const badCard = emptyBoard(); badCard.columns[0].tasks.push({ id: 'x', title: 'x', status: 'done' });
    for (const data of [null, [], {}, { columns: [] }, wrongOrder, badTasks, badCard]) {
        assert.deepEqual(await run('tasks', ok(data)), [1, [], []]);
    }
});
test('every non-200 is an error and error bodies are never read', async () => {
    for (const status of [201, 204, 301, 400, 404, 500, 502]) {
        assert.deepEqual(await run('boards', { status, json: () => assert.fail('sensitive error body read') }), [status, [], []]);
    }
});
test('network errors and timeouts do not expose exception messages', async () => {
    assert.deepEqual(await program({ recv: async () => ['x', 'boards'] }, async () => { throw Error('SENTINEL_SECRET'); }), [0, [], []]);
});
test('invalid JSON does not expose parse errors', async () => {
    assert.deepEqual(await run('boards', { status: 200, json: async () => { throw Error('SENTINEL_SECRET'); } }), [1, [], []]);
});
test('unknown request kinds cannot be interpreted as task responses', async () => {
    assert.deepEqual(await run('invalid', ok(emptyBoard())), [1, [], []]);
});
