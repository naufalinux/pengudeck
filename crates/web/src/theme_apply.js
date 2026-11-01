// Fixed program: puts the theme on <html>, so body, page background and every token share one palette.
// Only "light" or "dark" is applied; the value arrives serialized.
return (async () => {
const theme = await dioxus.recv();
if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
return true;
})();
