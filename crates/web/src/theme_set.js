// Fixed program: saves the chosen theme. The value arrives serialized; only "light" or "dark" is stored.
return (async () => {
const theme = await dioxus.recv();
try {
    if (theme === "light" || theme === "dark") localStorage.setItem("hermes-dash.theme", theme);
} catch (_) {
    // Storage blocked: the theme still switches for this visit.
}
return true;
})();
