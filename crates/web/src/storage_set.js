// Fixed program: saves the chosen board slug. The slug arrives serialized, never as source.
return (async () => {
const slug = await dioxus.recv();
try {
    if (typeof slug === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(slug)) {
        localStorage.setItem("hermes-dash.board", slug);
    }
} catch (_) {
    // Storage blocked: the app still works, it just will not remember the choice.
}
return true;
})();
