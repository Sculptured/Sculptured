// Sets the saved look and colour mode on <html> before first paint, so a light look never flashes
// dark while the app loads. Mirrors applyAppearance() in src/appearance.ts.
/* global localStorage, document */
(function () {
  try {
    var a = JSON.parse(localStorage.getItem('sculptured.appearance.v1') || 'null');
    if (!a || a.look !== 'standard') return;
    var mode = a.mode === 'light' || a.mode === 'dark' ? a.mode : 'system';
    var root = document.documentElement;
    root.setAttribute('data-look', 'standard');
    root.setAttribute('data-mode', mode);
    var meta = document.querySelector('meta[name="color-scheme"]');
    if (meta) meta.setAttribute('content', mode === 'system' ? 'light dark' : mode);
  } catch { /* Storage can be unavailable; the app applies defaults when it loads. */ }
})();
