return (async () => {
const [operation, payload] = await dioxus.recv();
const routes = {
    get: ['GET', ''], save: ['PUT', ''],
    preview: ['POST', '/preview'], apply: ['POST', '/apply'], track: ['POST', '/track'],
};
try {
    if (!Object.hasOwn(routes, operation)) throw new Error('invalid operation');
    const [method, suffix] = routes[operation];
    const response = await fetch('http://127.0.0.1:8788/api/settings/retention' + suffix, {
        method, credentials: 'omit', cache: 'no-store', redirect: 'error',
        headers: { 'Content-Type': 'application/json', 'X-Pengudeck-Request': 'retention-v1' },
        body: method === 'GET' ? undefined : JSON.stringify(payload),
        signal: AbortSignal.timeout(70000),
    });
    if (!response.ok) throw new Error('request failed');
    const data = await response.json();
    if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('invalid response');
    return JSON.stringify({ ok: true, data });
} catch {
    return JSON.stringify({ ok: false, error: 'Permintaan retensi gagal. Muat ulang Settings dan preview sebelum mencoba lagi.' });
}
})();
