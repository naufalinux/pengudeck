import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const css = await readFile(new URL('../src/style.css', import.meta.url), 'utf8');
const load = async name => new AsyncFunction('dioxus', 'localStorage', await readFile(new URL(`../src/${name}`, import.meta.url), 'utf8'));

const hex = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16) / 255)
    .map(c => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
const lum = h => { const [r, g, b] = hex(h); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
const ratio = (a, b) => { const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };

// Extract the declaration block of one selector, e.g. ':root, [data-theme="light"]'.
const block = selector => {
    const start = css.indexOf(selector + ' {');
    assert.notEqual(start, -1, `missing block ${selector}`);
    return css.slice(start, css.indexOf('}', start));
};
const tokens = selector => Object.fromEntries(
    [...block(selector).matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})/g)].map(m => [m[1], m[2]]));

const LIGHT = tokens(':root, [data-theme="light"]');
const DARK = tokens('[data-theme="dark"]');

for (const [name, t] of [['light', LIGHT], ['dark', DARK]]) {
    test(`${name} theme defines every token`, () => {
        for (const key of ['bg', 'surface', 'card', 'text', 'text-2', 'accent', 'accent-text', 'ring']) {
            assert.ok(t[key], `${name} missing --${key}`);
        }
    });
    test(`${name} theme: body text meets WCAG AA 4.5:1 on every surface`, () => {
        for (const surface of ['bg', 'surface', 'card']) {
            assert.ok(ratio(t.text, t[surface]) >= 4.5, `text on ${surface}`);
            assert.ok(ratio(t['text-2'], t[surface]) >= 4.5, `text-2 on ${surface}`);
        }
    });
    test(`${name} theme: button label on accent and focus ring meet contrast`, () => {
        assert.ok(ratio(t['accent-text'], t.accent) >= 4.5, 'button label on accent');
        assert.ok(ratio(t.ring, t.bg) >= 3, 'focus ring on page background');
    });
}

test('no external resources: fonts and images are local only', () => {
    assert.doesNotMatch(css, /url\(\s*["']?https?:/i);
    assert.doesNotMatch(css, /@import/i);
    assert.match(css, /url\("\/fonts\/fraunces-latin-wght\.woff2"\)/);
});

test('theme storage saves only light or dark and returns "" otherwise', async () => {
    const get = await load('theme_get.js');
    const set = await load('theme_set.js');
    const store = (v = {}) => ({ getItem: k => (k in v ? v[k] : null), setItem: (k, x) => { v[k] = x; } });
    const s = store({ 'hermes-dash.theme': 'dark' });
    assert.equal(await get(null, s), 'dark');
    assert.equal(await get(null, store({ 'hermes-dash.theme': 'neon' })), '');
    assert.equal(await get(null, store({})), '');
    const w = store();
    await set({ recv: async () => 'dark' }, w);
    assert.equal(w.getItem('hermes-dash.theme'), 'dark');
    await set({ recv: async () => 'javascript:alert(1)' }, w);
    assert.equal(w.getItem('hermes-dash.theme'), 'dark');
    const blocked = { getItem() { throw new Error('denied'); }, setItem() { throw new Error('denied'); } };
    assert.equal(await get(null, blocked), '');
    assert.equal(await set({ recv: async () => 'light' }, blocked), true);
});
