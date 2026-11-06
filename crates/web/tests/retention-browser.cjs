// Browser contract test. Uses an existing Playwright installation; no downloads.
// Every browser HTTP request is routed to fixture files/the isolated loopback API.
const { spawn, execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const readline = require('node:readline');
const pw = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const root = path.resolve(__dirname, '../../..');
const publicDir = path.join(root, 'target/dx/hermes-dash-web/debug/web/public');
const output = process.env.QA_OUT || path.join(os.tmpdir(), 'pengudeck-retention-browser');
fs.mkdirSync(output, { recursive: true });

async function auditContrast(page) {
  return page.evaluate(() => {
    const rgb = value => { const numbers = value.match(/[\d.]+/g)?.map(Number); return numbers?.length >= 3 ? numbers : null; };
    const lum = color => color.slice(0, 3).map(value => { const c = value / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }).reduce((sum, value, i) => sum + value * [0.2126, 0.7152, 0.0722][i], 0);
    const failures = []; let checked = 0;
    for (const element of document.querySelectorAll('body, body *')) {
      if (element.closest(':disabled') || !element.getClientRects().length) continue;
      if (element.tagName !== 'BODY' && ![...element.childNodes].some(n => n.nodeType === Node.TEXT_NODE && n.textContent.trim())) continue;
      const style = getComputedStyle(element), fg = rgb(style.color);
      let bg = null;
      for (let ancestor = element; ancestor && !bg; ancestor = ancestor.parentElement) {
        const color = rgb(getComputedStyle(ancestor).backgroundColor);
        if (color && (color.length === 3 || color[3] >= 0.99)) bg = color;
      }
      if (!fg || !bg || style.visibility === 'hidden') continue;
      const a = lum(fg), b = lum(bg), ratio = (Math.max(a,b) + 0.05) / (Math.min(a,b) + 0.05);
      const size = parseFloat(style.fontSize), large = size >= 24 || (size >= 18.66 && Number(style.fontWeight) >= 700);
      checked++;
      if (ratio < (large ? 3 : 4.5)) failures.push({ text: element.innerText.slice(0,80), ratio });
    }
    return { checked, failures };
  });
}

async function run(engine) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'pengudeck-browser-'));
  const env = { ...process.env, HERMES_KANBAN_HOME: home, HERMES_KANBAN_DB: path.join(home, 'kanban.db') };
  const cli = (...args) => execFileSync('hermes', ['kanban', '--board', 'default', ...args], { env, encoding: 'utf8', timeout: 15000 });
  let server, browser;
  try {
    cli('init');
    const done = JSON.parse(cli('create', 'browser archive fixture', '--initial-status', 'blocked', '--json')).id;
    const purge = JSON.parse(cli('create', 'browser purge fixture', '--initial-status', 'blocked', '--json')).id;
    cli('complete', done, '--result', 'fixture only');
    cli('archive', purge);
    execFileSync('python3', ['-c', `import sqlite3,time,sys; db=sqlite3.connect(sys.argv[1]); now=int(time.time()); db.execute('UPDATE tasks SET completed_at=? WHERE id=?',(now-31*86400,sys.argv[2])); db.execute('UPDATE task_events SET created_at=? WHERE task_id=?',(now-31*86400,sys.argv[2])); db.execute('UPDATE task_events SET created_at=? WHERE task_id=?',(now-181*86400,sys.argv[3])); db.commit(); db.close()`, env.HERMES_KANBAN_DB, done, purge]);
    fs.writeFileSync(path.join(home, '.pengudeck-test-fixture'), 'private browser fixture');
    server = spawn(path.join(root, 'target/debug/examples/retention_fixture'), [], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let serverErrors = '';
    server.stderr.on('data', data => { serverErrors += data.toString(); });
    const addr = await Promise.race([
      new Promise((resolve, reject) => { readline.createInterface({ input: server.stdout }).once('line', resolve); server.once('exit', code => reject(new Error(`fixture server exited ${code}: ${serverErrors}`))); }),
      new Promise((_, reject) => setTimeout(() => reject(new Error('fixture startup timed out')), 10000).unref()),
    ]);
    browser = engine === 'webkit' && process.env.PW_WEBKIT_WS
      ? await pw.webkit.connect(process.env.PW_WEBKIT_WS)
      : await pw[engine].launch({ headless: true });
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    const errors = [], failures = [], external = [];
    await context.routeWebSocket('**', socket => socket.close());
    await context.route('**/*', async route => {
      const request = route.request(), url = new URL(request.url());
      if (url.origin === 'http://127.0.0.1:8788' && url.pathname.startsWith('/api/')) {
        const headers = { ...request.headers(), host: '127.0.0.1:8788' };
        delete headers['content-length'];
        try {
          const result = await context.request.fetch(`http://${addr}${url.pathname}`, { method: request.method(), headers, data: request.postDataBuffer() || undefined, timeout: 70000 });
          await route.fulfill({ response: result });
        } catch (error) { failures.push(String(error)); await route.abort(); }
      } else if (url.origin === 'http://127.0.0.1:8081') {
        const local = path.resolve(publicDir, '.' + decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname));
        if (!local.startsWith(publicDir + path.sep) || !fs.existsSync(local) || !fs.statSync(local).isFile()) return route.fulfill({ status: 404, body: 'not found' });
        const contentType = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.woff2': 'font/woff2' }[path.extname(local)] || 'application/octet-stream';
        await route.fulfill({ status: 200, contentType, body: fs.readFileSync(local) });
      } else {
        external.push(request.url());
        await route.abort();
      }
    });
    const page = await context.newPage();
    page.on('pageerror', error => errors.push(String(error)));
    page.on('console', msg => { if (msg.type() === 'error' && !msg.text().includes('_dioxus')) errors.push(msg.text()); });
    await page.goto('http://127.0.0.1:8081');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.locator('#archive-days').waitFor();
    assert.equal(await page.locator('#archive-days').inputValue(), '30');
    assert.equal(await page.locator('#purge-days').inputValue(), '180');
    assert.equal(await page.locator('#retention-enabled').isChecked(), false);
    await page.locator('#archive-days').fill('0');
    assert.equal(await page.getByRole('button', { name: 'Simpan kebijakan' }).isDisabled(), true);
    await page.locator('#archive-days').fill('30');
    assert.equal(await page.getByRole('button', { name: 'Inisialisasi pencatatan', exact: true }).isDisabled(), true);
    await page.locator('#tracking-confirmation').fill('INITIALIZE TRACKING');
    await page.getByRole('button', { name: 'Inisialisasi pencatatan', exact: true }).click();
    await page.getByText('Pencatatan retensi siap. Jalankan preview baru.', { exact: true }).waitFor();
    // Simulate elapsed months in PRIVATE fixture clocks, never a live database.
    execFileSync('python3', ['-c', `import sqlite3,time,sys; db=sqlite3.connect(sys.argv[1]); now=int(time.time()); db.execute('UPDATE pengudeck_retention_clock SET activity_at=? WHERE task_id=?',(now-31*86400,sys.argv[2])); db.execute('UPDATE pengudeck_retention_clock SET activity_at=?,archived_at=? WHERE task_id=?',(now-181*86400,now-181*86400,sys.argv[3])); db.commit(); db.close()`, env.HERMES_KANBAN_DB, done, purge]);
    // Enable requires typed confirmation; immediately disable again (fixture has no scheduler).
    await page.locator('#retention-enabled').check();
    assert.equal(await page.getByRole('button', { name: 'Simpan kebijakan' }).isDisabled(), true);
    await page.locator('#enable-confirmation').fill('ENABLE RETENTION');
    await page.getByRole('button', { name: 'Simpan kebijakan' }).click();
    await page.getByText('Kebijakan tersimpan.', { exact: true }).waitFor();
    await page.locator('#retention-enabled').uncheck();
    await page.getByRole('button', { name: 'Simpan kebijakan' }).click();
    await page.getByText('Kebijakan tersimpan.', { exact: true }).waitFor();
    await page.reload();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.locator('#archive-days').waitFor();
    assert.equal(await page.locator('#retention-enabled').isChecked(), false);
    await page.getByRole('button', { name: 'Preview / dry-run' }).click();
    await page.locator('#apply-confirmation').waitFor();
    const preview = JSON.parse(await page.locator('.retention-output').first().innerText());
    assert.deepEqual(preview.archive.map(x => x.id), [done]);
    assert.deepEqual(preview.purge.map(x => x.id), [purge]);
    assert.equal(await page.getByRole('button', { name: 'Jalankan arsip / hapus' }).isDisabled(), true);
    await page.locator('#apply-confirmation').fill('ARCHIVE AND DELETE');
    const lightContrast = await auditContrast(page);
    assert.ok(lightContrast.checked > 10, 'contrast audit must inspect rendered text');
    assert.deepEqual(lightContrast.failures, []);
    await page.screenshot({ path: path.join(output, `${engine}-desktop-light.png`), fullPage: true });
    await page.getByRole('button', { name: 'Jalankan arsip / hapus' }).click();
    await page.getByText('Eksekusi selesai; periksa hasil lalu muat ulang board.', { exact: true }).waitFor();
    const result = JSON.parse(await page.locator('.retention-output').last().innerText());
    assert.equal(result.archived.length, 1);
    assert.equal(result.purged.length, 1);
    assert.deepEqual(result.errors, []);
    const rows = JSON.parse(cli('list', '--archived', '--json'));
    assert.deepEqual(rows.map(x => [x.id, x.status]), [[done, 'archived']]);
    await page.getByRole('button', { name: 'Mode gelap', exact: true }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(output, `${engine}-mobile-dark.png`), fullPage: true });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    assert.equal(overflow, false, 'Settings should not overflow horizontally');
    const darkContrast = await auditContrast(page);
    assert.ok(darkContrast.checked > 10);
    assert.deepEqual(darkContrast.failures, []);
    assert.deepEqual(errors, []);
    assert.deepEqual(failures, []);
    assert.deepEqual(external, []);
    return { engine, passed: true, archived: result.archived.length, purged: result.purged.length, lightContrast, darkContrast, errors, failures, external };
  } finally {
    if (browser) await browser.close();
    if (server) { server.kill('SIGTERM'); await new Promise(resolve => { if (server.exitCode !== null) resolve(); else server.once('exit', resolve); }); }
    fs.rmSync(home, { recursive: true, force: true });
  }
}
(async () => {
  const results = [];
  for (const engine of process.argv.slice(2).length ? process.argv.slice(2) : ['chromium', 'firefox', 'webkit']) {
    try { results.push(await run(engine)); }
    catch (error) { results.push({ engine, passed: false, error: String(error), stack: error.stack }); }
  }
  fs.writeFileSync(path.join(output, 'results.json'), JSON.stringify(results, null, 2));
  console.log(JSON.stringify(results, null, 2));
  if (results.some(r => !r.passed)) process.exitCode = 1;
})();
