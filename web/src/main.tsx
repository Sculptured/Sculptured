import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import { TooltipLayer } from './shell/ui';
import './tokens.css';
import './styles.css';
import './polish.css';
import './usability.css';
import './release/release.css';
import './coastal.css';
import './simple.css';
import './ux.css';
import './power.css';
import './distill.css';
import './standard.css';
import './pages.css';
import './shell/logos.css';
import { applyAppearance, readAppearance } from './appearance';

// Retain browser preferences when upgrading an existing installation.
for (const storage of [localStorage,sessionStorage]) {
  try {
    for (const key of Object.keys(storage)) {
      const renamed=key.replace(/^rig-control(?=[.:/-]|$)/,'sculptured').replace(/^rig-workspace-/,'sculptured-workspace-');
      if(renamed!==key&&storage.getItem(renamed)===null)storage.setItem(renamed,storage.getItem(key)!);
    }
  } catch { /* Storage can be unavailable; use session defaults. */ }
}
applyAppearance(readAppearance());

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
    <TooltipLayer />
  </StrictMode>,
);
