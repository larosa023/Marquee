import { shell } from './api';
import { contrast, onAccent } from './img';
import type { Theme } from './types';

export type MotionMode = 'full' | 'reduced' | 'off';

export interface Appearance {
  themeId: string;
  /** Hex override, or null to use the theme's accent. */
  accent: string | null;
  motion: MotionMode;
  density: 'compact' | 'comfortable' | 'spacious';
  scale: number;
  useSuiteTheme: boolean;
}

export const defaultAppearance: Appearance = { themeId: 'marquee', accent: null, motion: 'full', density: 'comfortable', scale: 1, useSuiteTheme: true };

export function appearanceFrom(settings: Record<string, any>): Appearance {
  return { ...defaultAppearance, ...(settings['appearance'] ?? {}) };
}

/** Picks the theme to show: Hangar's suite theme, the light/dark half of "Follow Windows", or the chosen one. */
export function resolveTheme(a: Appearance, themes: Theme[], systemLight: Theme | null, suite: Theme | null, hangarInstalled: boolean, prefersLight: boolean): Theme {
  if (a.useSuiteTheme && hangarInstalled && suite) return suite;
  const chosen = themes.find((t) => t.id === a.themeId) ?? themes.find((t) => t.id === 'marquee') ?? themes[0];
  if (chosen.id === 'system' && prefersLight && systemLight) return systemLight;
  return chosen;
}

/** Makes sure an accent can be seen against the page: lightens it on dark themes, darkens it on light ones. */
export function readableAccent(accent: string, background: string, mode: 'dark' | 'light'): string {
  let c = accent;
  for (let i = 0; i < 24 && contrast(c, background) < 3; i++) {
    c = mix(c, mode === 'dark' ? '#FFFFFF' : '#000000', 0.08);
  }
  return c;
}

function mix(a: string, b: string, amount: number): string {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const ch = (shift: number) => {
    const x = (pa >> shift) & 255;
    const y = (pb >> shift) & 255;
    return Math.round(x + (y - x) * amount)
      .toString(16)
      .padStart(2, '0');
  };
  return `#${ch(16)}${ch(8)}${ch(0)}`.toUpperCase();
}

/** The CSS variables a theme sets. Also used by the theme editor's live preview. */
export function themeVars(theme: Theme, accentOverride: string | null, systemAccent: string | null): Record<string, string> {
  const c = theme.colors;
  const raw = accentOverride ?? (c.accent === 'windows' ? (systemAccent ?? '#F2C14E') : c.accent);
  const accent = readableAccent(raw, c.background, theme.mode);
  const m = theme.marquee;
  return {
    '--bg': c.background,
    '--sidebar': c.sidebar,
    '--surface': theme.surfaceOpacity < 1 ? `color-mix(in srgb, ${c.surface} ${Math.round(theme.surfaceOpacity * 100)}%, transparent)` : c.surface,
    '--surface-hover': c.surfaceHover,
    '--surface-strong': c.surfaceStrong,
    '--text': c.text,
    '--subtext': c.subtext,
    '--faint': c.faint,
    '--accent': accent,
    '--on-accent': onAccent(accent),
    '--good': theme.mode === 'dark' ? '#4FD08A' : '#177A45',
    '--warn': theme.mode === 'dark' ? '#F2B84B' : '#8A5A00',
    '--bad': theme.mode === 'dark' ? '#FF7B7B' : '#B3261E',
    '--radius': `${m.radius}px`,
    '--control-radius': `${m.controlRadius}px`,
    '--blur': `${m.blur}px`,
    '--shadow': String(m.shadow),
    '--glow': String(m.glow),
    '--motion': String(m.motion),
    '--transparency': String(theme.transparency),
    '--font': `'${m.font}', 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif`,
    '--display': `'${m.displayFont}', 'Segoe UI Variable Display', 'Segoe UI', system-ui, sans-serif`,
  };
}

let lastBackdrop = '';

export function applyTheme(theme: Theme, a: Appearance, systemAccent: string | null) {
  const root = document.documentElement;
  const apply = () => {
    for (const [k, v] of Object.entries(themeVars(theme, a.accent, systemAccent))) root.style.setProperty(k, v);
    root.dataset.mode = theme.mode;
    root.dataset.backdrop = theme.backdrop;
    root.dataset.motion = a.motion;
    root.dataset.density = a.density;
    root.dataset.theme = theme.id;
  };
  // Crossfade between themes where the browser supports it and motion is on.
  const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
  if (doc.startViewTransition && a.motion === 'full' && root.dataset.theme && root.dataset.theme !== theme.id) doc.startViewTransition(apply);
  else apply();
  if (theme.backdrop !== lastBackdrop) {
    lastBackdrop = theme.backdrop;
    shell('set_backdrop', { kind: theme.backdrop }).catch(() => {});
  }
}

export async function applyScale(scale: number) {
  const s = Math.min(1.5, Math.max(0.9, scale));
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    await getCurrentWebview().setZoom(s);
  } catch {
    // In a browser, fall back to CSS zoom.
    (document.documentElement.style as any).zoom = String(s);
  }
}
