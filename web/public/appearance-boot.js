// Sets the saved look and colour mode on <html> before first paint, so a light look never flashes
// dark (nor Mission flashes Coastal) while the app loads. Mirrors applyAppearance() in src/appearance.ts.
/* global localStorage, document */
(function () {
  try {
    var a = JSON.parse(localStorage.getItem('sculptured.appearance.v1') || 'null');
    if (!a || (a.look !== 'standard' && a.look !== 'mission')) return;
    var mode = a.mode === 'light' || a.mode === 'dark' ? a.mode : 'system';
    var root = document.documentElement;
    root.setAttribute('data-look', a.look);
    root.setAttribute('data-mode', mode);
    var meta = document.querySelector('meta[name="color-scheme"]');
    // Mission is dark only, like Coastal (the default, which needs nothing set here).
    if (meta) meta.setAttribute('content', a.look === 'mission' ? 'dark' : mode === 'system' ? 'light dark' : mode);
  } catch { /* Storage can be unavailable; the app applies defaults when it loads. */ }
})();
