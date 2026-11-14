// The window chrome: custom title bar, collapsible sidebar, and the top bar (search, sync, notifications).
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { api, inTauri, openUrl } from '@/lib/api';
import { ago } from '@/lib/format';
import { prettyCombo } from '@/lib/hotkeys';
import type { Notification } from '@/lib/types';
import { EMPTY, useApp } from '@/state/app';
import { Icon, type IconName } from '@/ui/icons';
import { Logo } from '@/ui/Logo';
import { Button, IconButton, Kbd, cx } from '@/ui/primitives';

export const NAV: { path: string; label: string; icon: IconName }[] = [
  { path: '/home', label: 'Home', icon: 'home' },
  { path: '/for-you', label: 'For You', icon: 'sparkles' },
  { path: '/calendar', label: 'Calendar', icon: 'calendar' },
  { path: '/movies', label: 'Movies', icon: 'film' },
  { path: '/tv', label: 'TV', icon: 'tv' },
  { path: '/anime', label: 'Anime', icon: 'anime' },
  { path: '/games', label: 'Games', icon: 'gamepad' },
  { path: '/music', label: 'Music', icon: 'music' },
  { path: '/lists', label: 'Lists', icon: 'list' },
  { path: '/taste', label: 'Taste', icon: 'dna' },
  { path: '/settings', label: 'Settings', icon: 'settings' },
];

async function win() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return getCurrentWindow();
}

/** Minimise / maximise / close, drawn to match the theme. Close sends Marquee to the tray. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!inTauri) return;
    let off: (() => void) | undefined;
    void win().then(async (w) => {
      setMaximized(await w.isMaximized());
      off = await w.onResized(async () => setMaximized(await w.isMaximized()));
    });
    return () => off?.();
  }, []);
  if (!inTauri) return null;
  const btn = 'no-drag grid h-full w-11.5 place-items-center text-subtext transition-colors hover:bg-hover hover:text-text';
  return (
    <div className="flex h-full">
      <button type="button" aria-label="Minimize" className={btn} onClick={() => void win().then((w) => w.minimize())}>
        <Icon name="minimize" size={15} />
      </button>
      <button type="button" aria-label={maximized ? 'Restore' : 'Maximize'} className={btn} onClick={() => void win().then((w) => w.toggleMaximize())}>
        <Icon name={maximized ? 'restore' : 'maximize'} size={14} />
      </button>
      <button type="button" aria-label="Close" className={cx(btn, 'hover:bg-[#C42B1C]! hover:text-white!')} onClick={() => void win().then((w) => w.close())}>
        <Icon name="x" size={16} />
      </button>
    </div>
  );
}

export function Sidebar() {
  const path = useApp((s) => s.path);
  const item = useApp((s) => s.item);
  const collapsed = useApp((s) => s.sidebarCollapsed);
  const navigate = useApp((s) => s.navigate);
  const toggle = useApp((s) => s.toggleSidebar);
  const syncing = useApp((s) => s.sync.running);
  const lists = useApp((s) => s.boot?.lists ?? EMPTY);
  return (
    <motion.nav aria-label="Main" className="glass flex h-full shrink-0 flex-col overflow-hidden border-r border-white/6" animate={{ width: collapsed ? 64 : 224 }} transition={{ type: 'spring', stiffness: 420, damping: 40 }} initial={false}>
      <div className="drag flex h-13 shrink-0 items-center gap-2.5 pl-4.5">
        <Logo size={28} active={syncing} />
        {!collapsed && <span className="display text-[17px] font-bold tracking-tight">Marquee</span>}
      </div>
      <ul className="flex-1 space-y-0.5 overflow-y-auto overflow-x-hidden px-2.5 py-2">
        {NAV.map((n, i) => {
          const active = !item && (path === n.path || path.startsWith(`${n.path}/`));
          return (
            <li key={n.path}>
              {n.path === '/settings' && <div className="mx-2 my-2 h-px bg-white/8" />}
              <button
                type="button"
                onClick={() => navigate(n.path)}
                aria-current={active ? 'page' : undefined}
                title={collapsed ? `${n.label} (Ctrl+${i + 1})` : undefined}
                className={cx('relative flex h-10 w-full items-center gap-3 rounded-control px-3 text-left font-medium transition-colors', active ? 'text-text' : 'text-subtext hover:bg-hover/70 hover:text-text')}
              >
                {active && <motion.span layoutId="nav-active" className="absolute inset-0 rounded-control bg-strong" transition={{ type: 'spring', stiffness: 520, damping: 40 }} />}
                {active && <motion.span layoutId="nav-bar" className="absolute left-0 top-2.5 h-5 w-[3px] rounded-full bg-accent" transition={{ type: 'spring', stiffness: 520, damping: 40 }} />}
                <Icon name={n.icon} size={19} className={cx('relative shrink-0', active && 'text-accent')} />
                {!collapsed && <span className="relative truncate">{n.label}</span>}
              </button>
              {n.path === '/lists' && !collapsed && lists.length > 0 && (
                <ul className="mb-1 mt-0.5 space-y-px pl-10">
                  {lists.slice(0, 7).map((l) => (
                    <li key={l.id}>
                      <button
                        type="button"
                        onClick={() => navigate(`/lists/${l.id}`)}
                        aria-current={path === `/lists/${l.id}` ? 'page' : undefined}
                        className={cx('flex h-7.5 w-full items-center justify-between gap-2 rounded-md px-2 text-left text-[13px] transition-colors hover:bg-hover/70', path === `/lists/${l.id}` ? 'text-text' : 'text-faint hover:text-text')}
                      >
                        <span className="truncate">{l.name}</span>
                        <span className="tnum text-[11px] text-faint">{l.count || ''}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
      <div className="px-2.5 pb-3">
        <IconButton icon="panel" label={collapsed ? 'Expand sidebar (Ctrl+B)' : 'Collapse sidebar (Ctrl+B)'} onClick={toggle} />
      </div>
    </motion.nav>
  );
}

function SyncStatus() {
  const sync = useApp((s) => s.sync);
  const offline = useApp((s) => s.offline);
  const last = useApp((s) => s.boot?.settings['last_sync']?.at as string | undefined);
  const pending = useApp((s) => s.boot?.pending_writes ?? 0);
  const toast = useApp((s) => s.toast);
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 60_000);
    return () => clearInterval(t);
  }, []);
  const label = sync.running ? sync.label || 'Syncing' : offline ? 'Offline' : `Synced ${ago(last)}`;
  return (
    <button
      type="button"
      className="no-drag group flex h-9 items-center gap-2 rounded-control px-2.5 text-[13px] text-subtext transition-colors hover:bg-hover hover:text-text"
      title={sync.running ? 'Sync in progress' : 'Sync now (Ctrl+R)'}
      aria-label={`${label}. Sync now.`}
      disabled={sync.running}
      onClick={() => {
        useApp.getState().set({ sync: { running: true, label: 'Starting', pct: 0 } });
        api('sync').catch((e) => {
          useApp.getState().set({ sync: { running: false, label: '', pct: 0 } });
          toast(e.message ?? 'Sync failed', 'bad');
        });
      }}
    >
      <span className="relative grid h-5 w-5 place-items-center">
        {sync.running ? (
          <svg width="20" height="20" viewBox="0 0 20 20" className="-rotate-90">
            <circle cx="10" cy="10" r="7.500" fill="none" stroke="currentColor" strokeOpacity="0.2" strokeWidth="2.200" />
            <motion.circle cx="10" cy="10" r="7.500" fill="none" stroke="var(--accent)" strokeWidth="2.200" strokeLinecap="round" animate={{ pathLength: Math.max(0.04, sync.pct) }} transition={{ ease: 'easeOut', duration: 0.5 }} />
          </svg>
        ) : (
          <Icon name={offline ? 'wifiOff' : 'sync'} size={16} className={offline ? 'text-warn' : 'transition-transform duration-500 group-hover:rotate-180'} />
        )}
      </span>
      <span className="tnum max-w-52 truncate">{label}</span>
      {pending > 0 && !sync.running && <span className="tnum rounded-full bg-warn/20 px-1.5 text-[11px] font-semibold text-warn">{pending} queued</span>}
    </button>
  );
}

function NotificationCenter() {
  const open = useApp((s) => s.notificationsOpen);
  const setOpen = useApp((s) => s.setNotifications);
  const unread = useApp((s) => s.unread);
  const openItem = useApp((s) => s.openItem);
  const qc = useQueryClient();
  const ref = useRef<HTMLDivElement>(null);
  const { data } = useQuery({ queryKey: ['notifications'], queryFn: () => api<Notification[]>('notifications', { limit: 80 }), enabled: open });
  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent ? e.key === 'Escape' : !ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', close);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', close);
    };
  }, [open, setOpen]);
  const markAll = async () => {
    await api('notifications.read');
    useApp.getState().set({ unread: 0 });
    void qc.invalidateQueries({ queryKey: ['notifications'] });
  };
  const icons: Record<string, IconName> = { release: 'rocket', season: 'tv', album: 'music', sale: 'tag', deals: 'tag', digest: 'sparkles', system: 'info' };
  return (
    <div ref={ref} className="no-drag relative">
      <IconButton icon="bell" label={unread ? `Notifications, ${unread} unread` : 'Notifications'} active={open} onClick={() => setOpen(!open)} />
      {unread > 0 && <span className="tnum pointer-events-none absolute right-1 top-1 grid h-4 min-w-4 place-items-center rounded-full bg-accent px-1 text-[10px] font-bold text-on-accent">{unread > 9 ? '9+' : unread}</span>}
      <AnimatePresence>
        {open && (
          <motion.div role="dialog" aria-label="Notifications" className="panel absolute right-0 top-11 z-40 flex max-h-[70vh] w-96 flex-col" initial={{ opacity: 0, y: -8, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -6 }} transition={{ type: 'spring', stiffness: 480, damping: 34 }}>
            <div className="flex items-center justify-between px-4 pb-2 pt-3.5">
              <h2 className="font-semibold">Notifications</h2>
              <div className="flex gap-1">
                <Button size="sm" variant="ghost" disabled={!unread} onClick={() => void markAll()}>
                  Mark all read
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={!data?.length}
                  onClick={() =>
                    void api('notifications.clear').then(() => {
                      useApp.getState().set({ unread: 0 });
                      void qc.invalidateQueries({ queryKey: ['notifications'] });
                    })
                  }
                >
                  Clear
                </Button>
              </div>
            </div>
            <ul className="min-h-0 flex-1 overflow-y-auto p-1.5">
              {!data && Array.from({ length: 4 }, (_, i) => <li key={i} className="skeleton m-2 h-12" />)}
              {data?.length === 0 && (
                <li className="px-6 py-10 text-center text-subtext">
                  <Icon name="bell" size={26} className="mx-auto mb-3 text-faint" />
                  Nothing yet. Release days, new seasons, new albums and wishlist sales show up here.
                </li>
              )}
              {data?.map((n) => (
                <li key={n.id}>
                  <button
                    type="button"
                    className="flex w-full gap-3 rounded-control px-2.5 py-2.5 text-left transition-colors hover:bg-hover"
                    onClick={() => {
                      void api('notifications.read', { id: n.id });
                      if (!n.read) useApp.getState().set({ unread: Math.max(0, unread - 1) });
                      if (n.item_id) {
                        setOpen(false);
                        openItem(n.item_id);
                      }
                      void qc.invalidateQueries({ queryKey: ['notifications'] });
                    }}
                  >
                    <span className={cx('mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-full', n.read ? 'bg-strong text-faint' : 'bg-accent/15 text-accent')}>
                      <Icon name={icons[n.kind] ?? 'info'} size={16} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="flex items-baseline justify-between gap-2">
                        <span className={cx('truncate font-semibold', n.read && 'text-subtext')}>{n.title}</span>
                        <span className="tnum shrink-0 text-[11px] text-faint">{ago(n.created_at)}</span>
                      </span>
                      <span className="clamp-2 text-[13px] text-subtext">{n.body}</span>
                    </span>
                  </button>
                  {n.url && (
                    <div className="pb-2 pl-[54px]">
                      <Button size="sm" icon="external" onClick={() => void openUrl(n.url!)}>
                        Where to watch
                      </Button>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function UpdatePill() {
  const update = useApp((s) => s.update);
  const deferred = useApp((s) => s.sys?.deferred_to_hangar);
  // When Hangar manages updates it shows its own prompt; a second one here would be noise.
  if (!update || deferred) return null;
  return (
    <motion.button
      type="button"
      initial={{ opacity: 0, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      className="no-drag flex h-8 items-center gap-1.5 rounded-full bg-accent/15 px-3 text-[13px] font-semibold text-accent transition-colors hover:bg-accent/25"
      onClick={() => useApp.getState().set({ updateOpen: true })}
      title={`Marquee ${update.version} is ready`}
    >
      <Icon name="download" size={14} />
      Update ready
    </motion.button>
  );
}

export function TopBar() {
  const navigate = useApp((s) => s.navigate);
  const setPalette = useApp((s) => s.setPalette);
  const item = useApp((s) => s.item);
  const closeItem = useApp((s) => s.closeItem);
  const combo = useApp((s) => s.boot?.settings['shortcuts']?.palette ?? 'Ctrl+K');
  return (
    <header className="drag glass relative z-30 flex h-13 shrink-0 items-center gap-2 border-b border-white/6 pl-3" onDoubleClick={() => inTauri && void win().then((w) => w.toggleMaximize())}>
      <IconButton icon="arrowLeft" label="Back (Alt+Left)" className="no-drag" onClick={() => (item ? closeItem() : history.back())} />
      <button type="button" onClick={() => navigate('/search')} className="no-drag hairline flex h-9 w-full max-w-md items-center gap-2.5 rounded-control bg-surface/70 px-3 text-left text-faint transition-colors hover:bg-hover hover:text-subtext" aria-label="Search movies, shows, games and music">
        <Icon name="search" size={16} />
        <span className="flex-1 truncate">Search movies, shows, games, music…</span>
      </button>
      <button type="button" onClick={() => setPalette(true)} className="no-drag flex h-9 items-center gap-1 rounded-control px-2 text-faint transition-colors hover:bg-hover hover:text-text" aria-label="Open the command palette" title="Command palette">
        {prettyCombo(combo).map((k) => (
          <Kbd key={k}>{k}</Kbd>
        ))}
      </button>
      <div className="flex-1" />
      <UpdatePill />
      <SyncStatus />
      <NotificationCenter />
      <div className="ml-1 h-full">
        <WindowControls />
      </div>
    </header>
  );
}
