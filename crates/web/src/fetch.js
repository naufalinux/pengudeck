// Fixed program; URL/kind arrive as serialized values, never executable source.
const [url, kind] = await dioxus.recv();
let response;
try {
    response = await fetch(url, {
        method: "GET", credentials: "omit", cache: "no-store",
        redirect: "error", referrerPolicy: "no-referrer",
        signal: AbortSignal.timeout(10000),
    });
} catch (_) {
    return [0, [], []];
}
// Do not read or return error bodies, even when they contain JSON.
if (response.status !== 200) return [response.status, [], []];
try {
    const data = await response.json();
    if (kind === "boards") {
        if (!Array.isArray(data) || !data.every(slug =>
            typeof slug === "string" && /^[A-Za-z0-9_-]+$/.test(slug))) {
            return [1, [], []];
        }
        return [200, [...new Set(data)], []];
    }
    const statuses = ["todo", "ready", "running", "blocked", "review", "done"];
    if (!data || !Array.isArray(data.columns) || data.columns.length !== 6) {
        return [1, [], []];
    }
    const tasks = [];
    for (let i = 0; i < statuses.length; i++) {
        const column = data.columns[i];
        if (!column || column.status !== statuses[i] || !Array.isArray(column.tasks)) {
            return [1, [], []];
        }
        for (const task of column.tasks) {
            if (!task || typeof task.id !== "string" || typeof task.title !== "string" ||
                task.status !== column.status ||
                !(task.assignee == null || typeof task.assignee === "string")) {
                return [1, [], []];
            }
            tasks.push(task);
        }
    }
    return [200, [], tasks];
} catch (_) {
    return [1, [], []];
}
