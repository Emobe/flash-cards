// Applies the saved theme before first paint (ADR 0010 decision 3). Keep in step with theme.ts.
(() => {
  try {
    const theme = localStorage.getItem("fc.theme");
    if (theme === "light" || theme === "dark") {
      document.documentElement.setAttribute("data-theme", theme);
    }
  } catch {
    // Storage can be blocked: the page follows the system theme.
  }
})();
