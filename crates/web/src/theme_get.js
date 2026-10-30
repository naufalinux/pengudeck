// Fixed program: reads the saved theme. Returns "" unless it is exactly "light" or "dark".
return (async () => {
try {
    const value = localStorage.getItem("hermes-dash.theme");
    return value === "light" || value === "dark" ? value : "";
} catch (_) {
    return "";
}
})();
