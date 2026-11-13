import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { inTauri } from './lib/api';
import './index.css';

document.documentElement.dataset.shell = inTauri ? 'tauri' : 'browser';
// The app is a tool, not a page: no browser context menu or accidental drag-selection of the UI.
window.addEventListener('contextmenu', (e) => {
  const el = e.target as HTMLElement;
  if (!(el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.closest('.selectable'))) e.preventDefault();
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
