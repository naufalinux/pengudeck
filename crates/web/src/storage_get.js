// Fixed program: reads the saved board slug. Returns "" when absent, invalid, or storage is blocked.
return (async () => {
try {
    const value = localStorage.getItem("hermes-dash.board");
    return typeof value === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(value) ? value : "";
} catch (_) {
    return "";
}
})();
