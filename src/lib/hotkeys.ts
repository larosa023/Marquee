// Keyboard shortcuts. Every shortcut has an id and a default; Settings → Keyboard shortcuts can rebind them.
import { useEffect, useRef } from 'react';

export interface ShortcutDef {
  id: string;
  label: string;
  combo: string;
}

export const SHORTCUTS: ShortcutDef[] = [
  { id: 'palette', label: 'Command palette', combo: 'Ctrl+K' },
  { id: 'search', label: 'Search', combo: 'Ctrl+F' },
  { id: 'sync', label: 'Sync now', combo: 'Ctrl+R' },
  { id: 'settings', label: 'Settings', combo: 'Ctrl+,' },
  { id: 'back', label: 'Back', combo: 'Alt+ArrowLeft' },
  { id: 'sidebar', label: 'Collapse or expand the sidebar', combo: 'Ctrl+B' },
  { id: 'notifications', label: 'Notifications', combo: 'Ctrl+N' },
  { id: 'quick-rate', label: 'Quick Rate', combo: 'Ctrl+Shift+R' },
  { id: 'home', label: 'Home', combo: 'Ctrl+1' },
  { id: 'for-you', label: 'For You', combo: 'Ctrl+2' },
  { id: 'calendar', label: 'Calendar', combo: 'Ctrl+3' },
  { id: 'movies', label: 'Movies', combo: 'Ctrl+4' },
  { id: 'tv', label: 'TV', combo: 'Ctrl+5' },
  { id: 'games', label: 'Games', combo: 'Ctrl+6' },
  { id: 'music', label: 'Music', combo: 'Ctrl+7' },
  { id: 'lists', label: 'Lists', combo: 'Ctrl+8' },
  { id: 'taste', label: 'Your Taste', combo: 'Ctrl+9' },
];

/** "Ctrl+Shift+K" for a key event. Returns null while only modifiers are held. */
export function comboFrom(e: KeyboardEvent | React.KeyboardEvent): string | null {
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  parts.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
  return parts.join('+');
}

export function prettyCombo(combo: string): string[] {
  return combo.split('+').map((p) => ({ ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓', Escape: 'Esc', ' ': 'Space' })[p] ?? p);
}

export function bindings(overrides: Record<string, string> | undefined): Record<string, string> {
  const map: Record<string, string> = {};
  for (const s of SHORTCUTS) map[s.id] = overrides?.[s.id] ?? s.combo;
  return map;
}

function typing(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable);
}

/** Runs `handlers[id]` when that shortcut's combo is pressed. */
export function useHotkeys(overrides: Record<string, string> | undefined, handlers: Record<string, () => void>) {
  const latest = useRef(handlers);
  latest.current = handlers;
  useEffect(() => {
    const map = bindings(overrides);
    const byCombo = new Map(Object.entries(map).map(([id, combo]) => [combo, id]));
    const onKey = (e: KeyboardEvent) => {
      const combo = comboFrom(e);
      if (!combo) return;
      // Plain keys never fire while typing; modified ones do (Ctrl+K works from the search box).
      if (typing(e.target) && !(e.ctrlKey || e.metaKey || e.altKey)) return;
      const id = byCombo.get(combo) ?? (combo === 'F5' ? 'sync' : combo === '/' && !typing(e.target) ? 'search' : undefined);
      if (id && latest.current[id]) {
        e.preventDefault();
        latest.current[id]();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [overrides]);
}
