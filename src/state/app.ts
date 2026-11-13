import { useRef } from 'react';
import { create } from 'zustand';
import { api } from '@/lib/api';
import type { Bootstrap, Media } from '@/lib/types';

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'good' | 'bad';
  action?: { label: string; run: () => void };
}

export interface SystemInfo {
  version: string;
  accent: string | null;
  portable: boolean;
  data_dir: string;
  default_data_dir: string;
  log_dir: string;
  hangar_installed: boolean;
  deferred_to_hangar: boolean;
  crashed: string | null;
  platform: string;
}

export interface UpdateInfo {
  current: string;
  version: string;
  notes: string;
  prerelease: boolean;
  date: string | null;
}

interface Location {
  path: string;
  item: number | null;
}

function parseHash(hash: string): Location {
  const raw = hash.replace(/^#/, '') || '/home';
  const [path, query = ''] = raw.split('?');
  const item = Number(new URLSearchParams(query).get('item'));
  return { path: path || '/home', item: Number.isFinite(item) && item > 0 ? item : null };
}

function toHash(l: Location): string {
  return `#${l.path}${l.item ? `?item=${l.item}` : ''}`;
}

interface AppState {
  boot: Bootstrap | null;
  sys: SystemInfo | null;
  path: string;
  item: number | null;
  /** Where the poster morph should start from, so the same title in two rows animates from the one clicked. */
  itemOrigin: string | null;
  /** The one title whose poster morphs into the detail page. Only it carries a layoutId, so a
   * grid of a hundred cards isn't measured on every render. */
  morphId: number | null;
  paletteOpen: boolean;
  notificationsOpen: boolean;
  sidebarCollapsed: boolean;
  toasts: Toast[];
  sync: { running: boolean; label: string; pct: number };
  offline: boolean;
  unread: number;
  update: UpdateInfo | null;
  /** The release-notes dialog (opened from the "Update ready" pill). */
  updateOpen: boolean;
  /** The "Surprise me" dialog, opened on a kind of title (or 'any'). */
  surprise: Media | 'any' | null;
  /** A .marqueetheme or .marqueebackup file opened from Explorer, waiting for its screen. */
  pendingFile: string | null;

  setBoot(boot: Bootstrap): void;
  refreshBoot(): Promise<void>;
  setSys(sys: SystemInfo): void;
  setSetting(key: string, value: unknown): Promise<void>;
  navigate(path: string): void;
  openItem(id: number, origin?: string): void;
  closeItem(): void;
  syncLocation(): void;
  setPalette(open: boolean): void;
  setNotifications(open: boolean): void;
  toggleSidebar(): void;
  toast(text: string, kind?: Toast['kind'], action?: Toast['action']): void;
  dismissToast(id: number): void;
  set(partial: Partial<AppState>): void;
}

/** One shared empty list, so selectors that fall back to "nothing" return the same value every time. */
export const EMPTY: never[] = [];

let toastId = 0;

export const useApp = create<AppState>((set, get) => ({
  boot: null,
  sys: null,
  ...parseHash(typeof location === 'undefined' ? '' : location.hash),
  itemOrigin: null,
  morphId: null,
  paletteOpen: false,
  notificationsOpen: false,
  sidebarCollapsed: false,
  toasts: [],
  sync: { running: false, label: '', pct: 0 },
  offline: false,
  unread: 0,
  update: null,
  updateOpen: false,
  surprise: null,
  pendingFile: null,

  setBoot: (boot) => set({ boot, unread: boot.unread, sidebarCollapsed: !!boot.settings['ui.sidebar_collapsed'], offline: !!boot.settings['last_sync']?.offline }),
  refreshBoot: async () => {
    const boot = await api<Bootstrap>('bootstrap');
    set({ boot, unread: boot.unread });
  },
  setSys: (sys) => set({ sys }),
  setSetting: async (key, value) => {
    const boot = get().boot;
    if (boot) set({ boot: { ...boot, settings: { ...boot.settings, [key]: value } } });
    await api('settings.set', { key, value });
  },
  navigate: (path) => {
    if (path === get().path && !get().item) return;
    location.hash = toHash({ path, item: null });
  },
  openItem: (id, origin) => {
    set({ itemOrigin: origin ?? null, morphId: id });
    location.hash = toHash({ path: get().path, item: id });
  },
  closeItem: () => {
    // Going "back" keeps browser-style history: Alt+Left returns to the previous title in a chain.
    if (history.length > 1 && get().item) history.back();
    else location.hash = toHash({ path: get().path, item: null });
  },
  syncLocation: () => set(parseHash(location.hash)),
  setPalette: (paletteOpen) => set({ paletteOpen }),
  setNotifications: (notificationsOpen) => set({ notificationsOpen }),
  toggleSidebar: () => {
    const collapsed = !get().sidebarCollapsed;
    set({ sidebarCollapsed: collapsed });
    void get().setSetting('ui.sidebar_collapsed', collapsed);
  },
  toast: (text, kind = 'info', action) => {
    const id = ++toastId;
    set({ toasts: [...get().toasts.slice(-3), { id, text, kind, action }] });
    setTimeout(() => get().dismissToast(id), action ? 9000 : 5000);
  },
  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
  set: (partial) => set(partial),
}));

export function setting<T>(settings: Record<string, any> | undefined, key: string, fallback: T): T {
  const v = settings?.[key];
  return v === undefined || v === null ? fallback : (v as T);
}

export function useSetting<T>(key: string, fallback: T): [T, (value: T) => void] {
  // Keep the first fallback, so a literal like [] or {} is the same value on every render.
  const stable = useRef(fallback).current;
  const value = useApp((s) => setting(s.boot?.settings, key, stable));
  const setSetting = useApp((s) => s.setSetting);
  return [value, (v) => void setSetting(key, v)];
}
