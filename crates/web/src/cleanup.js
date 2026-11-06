return (async () => {
const [operation, payload] = await dioxus.recv();
const routes = {
    status: ['GET', ''], review: ['POST', '/review'], save: ['PUT', ''], run: ['POST', '/run'],
};
try {
    if (!Object.hasOwn(routes, operation)) throw new Error('invalid operation');
    const [method, suffix] = routes[operation];
    const response = await fetch('http://127.0.0.1:8788/api/settings/cleanup' + suffix, {
        method, credentials: 'omit', cache: 'no-store', redirect: 'error',
        headers: { 'Content-Type': 'application/json', 'X-Pengudeck-Request': 'retention-v1' },
        body: method === 'GET' ? undefined : JSON.stringify(payload),
        signal: AbortSignal.timeout(70000),
    });
    let data = null;
    try { data = await response.json(); } catch { data = null; }
    if (!response.ok) {
        // Only a fixed lowercase category is forwarded; anything else stays generic.
        const code = data && typeof data.error === 'string' && /^[a-z_]{1,48}$/.test(data.error) ? data.error : 'cleanup_failed';
        return JSON.stringify({ ok: false, code });
    }
    if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('invalid response');
    return JSON.stringify({ ok: true, data });
} catch {
    return JSON.stringify({ ok: false, code: 'cleanup_failed' });
}
})();
