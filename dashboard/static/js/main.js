// ─── App entry point ──────────────────────────────────────────────────────────
// Bootstraps Lucide icons and triggers the initial navigation.
// Must be the last script loaded.

document.addEventListener('DOMContentLoaded', () => {
  if (window.lucide) lucide.createIcons();
  navigate();
});
