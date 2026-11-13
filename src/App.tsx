import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, LayoutGroup, MotionConfig, motion } from 'framer-motion';
import { Suspense, lazy, useEffect, useMemo, useRef, useState } from 'react';
import { CommandPalette } from './features/shell/CommandPalette';
import { Sidebar, TopBar } from './features/shell/Chrome';
import { api, errorMessage, on, shell } from './lib/api';
import { useHotkeys } from './lib/hotkeys';
import { appearanceFrom, applyScale, applyTheme, resolveTheme } from './lib/theme';
import type { Bootstrap } from './lib/types';
import { useApp, type SystemInfo, type UpdateInfo } from './state/app';
import { refreshFeeds } from './ui/actions';
import { Logo } from './ui/Logo';
import { Modal, Toasts } from './ui/overlays';
import { Button, ErrorState, Skeleton } from './ui/primitives';

const Home = lazy(() => import('./features/home/Home'));
const ForYou = lazy(() => import('./features/foryou/ForYou'));
const QuickRate = lazy(() => import('./features/foryou/QuickRate'));
const Calendar = lazy(() => import('./features/calendar/Calendar'));
const Hub = lazy(() => import('./features/hub/Hub'));
const Lists = lazy(() => import('./features/lists/Lists'));
const Taste = lazy(() => import('./features/taste/Taste'));
const Search = lazy(() => import('./features/search/Search'));
const Settings = lazy(() => import('./features/settings/Settings'));
const Detail = lazy(() => import('./features/detail/Detail'));
const Onboarding = lazy(() => import('./features/onboarding/Onboarding'));
const Tray = lazy(() => import('./features/tray/Tray'));
const UpdateModal = lazy(() => import('./features/settings/DataSections').then((m) => ({ default: m.UpdateModal })));

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Feeds are local database reads: cheap to refetch, but keep them steady while the user looks at them.
      staleTime: 30_000,
      gcTime: 10 * 60_000,
      retry: (count, error) => (error as { kind?: string })?.kind !== 'offline' && (error as { kind?: string })?.kind !== 'not_configured' && count < 1,
      refetchOnWindowFocus: false,
    },
  },
});

function Page({ path }: { path: string }) {
  const [, root, sub] = path.split('/');
  switch (root) {
    case 'home':
      return <Home />;
    case 'for-you':
      return sub === 'rate' ? <QuickRate /> : <ForYou />;
    case 'calendar':
      return <Calendar />;
    case 'movies':
      return <Hub media="movie" />;
    case 'tv':
      return <Hub media="tv" />;
    case 'games':
      return <Hub media="game" />;
    case 'music':
      return <Hub media="album" />;
    case 'lists':
      return <Lists listId={sub ? Number(sub) : null} />;
    case 'taste':
      return <Taste />;
    case 'search':
      return <Search initial={sub ? decodeURIComponent(sub) : ''} />;
    case 'settings':
      return <Settings section={sub ?? 'general'} />;
    default:
      return <Home />;
  }
}

function PageFallback() {
  return (
    <div className="px-(--pad) pt-4" aria-busy="true">
      <Skeleton className="mb-6 h-9 w-64" />
      <Skeleton className="mb-8 h-72 w-full rounded-card!" />
      <Skeleton className="h-52 w-full rounded-card!" />
    </div>
  );
}

/** Loads the app's state and keeps it in step with the engine's events. */
function useBoot() {
  const setBoot = useApp((s) => s.setBoot);
  const [error, setError] = useState<unknown>(null);
  const qc = useQueryClient();
  const load = useMemo(
    () => () => {
      setError(null);
      Promise.all([api<Bootstrap>('bootstrap'), shell<SystemInfo>('system_info').catch(() => null), shell<{ available: UpdateInfo | null }>('update_state').catch(() => null)])
        .then(([boot, sys, update]) => {
          // Opened without a route: start on the screen chosen in Settings → General.
          const homeTab = boot.settings['general.home_tab'];
          if (!useApp.getState().boot && typeof homeTab === 'string' && (location.hash === '' || location.hash === '#/home') && homeTab !== '/home') location.hash = `#${homeTab}`;
          setBoot(boot);
          if (sys) useApp.getState().setSys(sys);
          if (update?.available) useApp.getState().set({ update: update.available });
        })
        .catch(setError);
    },
    [setBoot],
  );
  useEffect(() => {
    load();
    const app = useApp.getState;
    const offs = [
      on('sync:progress', (p) => app().set({ sync: { running: p.pct < 1, label: p.label, pct: p.pct } })),
      on('sync:done', (p) => {
        app().set({ sync: { running: false, label: '', pct: 0 }, offline: !!p?.offline });
        for (const w of (p?.warnings ?? []).slice(0, 2)) app().toast(w, 'info');
        void app().refreshBoot();
        refreshFeeds(qc);
        void qc.invalidateQueries({ queryKey: ['sources'] });
      }),
      on('taste:updated', () => {
        refreshFeeds(qc);
        void app().refreshBoot();
      }),
      on('sources:changed', () => {
        void qc.invalidateQueries({ queryKey: ['sources'] });
        void app().refreshBoot();
      }),
      on('notifications:new', () => {
        void app().refreshBoot();
        void qc.invalidateQueries({ queryKey: ['notifications'] });
      }),
      on('update:available', (info: UpdateInfo) => app().set({ update: info })),
      on('theme:hangar', () => void app().refreshBoot()),
      on('settings:changed', () => void app().refreshBoot()),
      on('data:restored', () => {
        void app().refreshBoot();
        void qc.invalidateQueries();
      }),
      on('navigate', (route: string) => app().navigate(route)),
      on('file:opened', (file: string) => {
        app().set({ pendingFile: file });
        app().navigate(file.endsWith('.marqueetheme') ? '/settings/appearance' : '/settings/backup');
      }),
    ];
    return () => offs.forEach((off) => off());
  }, [load, qc]);
  return { error, retry: load };
}

/** Applies the theme, motion, density and scale whenever they change (or Windows flips light/dark). */
function useAppearance() {
  const boot = useApp((s) => s.boot);
  const sys = useApp((s) => s.sys);
  const [prefersLight, setPrefersLight] = useState(() => window.matchMedia?.('(prefers-color-scheme: light)').matches ?? false);
  useEffect(() => {
    const mq = window.matchMedia?.('(prefers-color-scheme: light)');
    if (!mq) return;
    const onChange = () => setPrefersLight(mq.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);
  const appearance = useMemo(() => appearanceFrom(boot?.settings ?? {}), [boot?.settings]);
  useEffect(() => {
    if (!boot) return;
    const theme = resolveTheme(appearance, boot.themes, boot.system_light, boot.settings['hangar.theme'] ?? null, !!sys?.hangar_installed, prefersLight);
    applyTheme(theme, appearance, sys?.accent ?? null);
  }, [boot, appearance, sys, prefersLight]);
  useEffect(() => {
    void applyScale(appearance.scale);
  }, [appearance.scale]);
  return appearance;
}

function CrashNotice() {
  const crashed = useApp((s) => s.sys?.crashed);
  const logDir = useApp((s) => s.sys?.log_dir);
  const toast = useApp((s) => s.toast);
  const close = () => {
    void shell('clear_crash');
    const sys = useApp.getState().sys;
    if (sys) useApp.getState().setSys({ ...sys, crashed: null });
  };
  return (
    <Modal
      open={!!crashed}
      onClose={close}
      title="Marquee closed unexpectedly last time"
      footer={
        <>
          <Button onClick={() => void shell('open_folder', { which: 'logs' })}>Open the log folder</Button>
          <Button
            onClick={() =>
              void api<string>('diagnostics')
                .then((text) => navigator.clipboard.writeText(`${text}\nLast crash:\n${crashed}`))
                .then(() => toast('Diagnostics copied', 'good'))
                .catch((e) => toast(errorMessage(e), 'bad'))
            }
          >
            Copy diagnostics
          </Button>
          <Button variant="primary" onClick={close}>
            OK
          </Button>
        </>
      }
    >
      <p className="text-subtext">Your data is safe. If it keeps happening, the log files explain what went wrong. Nothing is sent anywhere unless you share it.</p>
      <p className="mt-3 text-[13px] text-faint">Logs: {logDir}</p>
      <pre className="mt-3 max-h-40 overflow-auto rounded-control bg-surface p-3 text-[12px] text-subtext">{crashed}</pre>
    </Modal>
  );
}

function Shell() {
  const { error, retry } = useBoot();
  const appearance = useAppearance();
  const boot = useApp((s) => s.boot);
  const path = useApp((s) => s.path);
  const item = useApp((s) => s.item);
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const sync = () => useApp.getState().syncLocation();
    window.addEventListener('hashchange', sync);
    // Tell the app shell the first frame is up (it shows the window now), and pick up anything it was holding.
    void shell<{ route?: string; restore_state?: { route?: string } }>('app_ready')
      .then((r) => {
        const route = r?.restore_state?.route ?? r?.route;
        if (route) location.hash = route.startsWith('#') ? route : `#${route}`;
      })
      .catch(() => {});
    return () => window.removeEventListener('hashchange', sync);
  }, []);

  // New page: back to the top. Also keep the shell informed, so a live update returns here.
  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
  }, [path]);
  useEffect(() => {
    void shell('set_ui_state', { state: { route: location.hash } }).catch(() => {});
  }, [path, item]);

  const go = useApp((s) => s.navigate);
  useHotkeys(boot?.settings['shortcuts'], {
    palette: () => useApp.getState().setPalette(!useApp.getState().paletteOpen),
    search: () => go('/search'),
    sync: () => {
      if (useApp.getState().sync.running) return;
      useApp.getState().set({ sync: { running: true, label: 'Starting', pct: 0 } });
      void api('sync').catch((e) => useApp.getState().toast(errorMessage(e), 'bad'));
    },
    settings: () => go('/settings'),
    back: () => history.back(),
    sidebar: () => useApp.getState().toggleSidebar(),
    notifications: () => useApp.getState().setNotifications(!useApp.getState().notificationsOpen),
    'quick-rate': () => go('/for-you/rate'),
    home: () => go('/home'),
    'for-you': () => go('/for-you'),
    calendar: () => go('/calendar'),
    movies: () => go('/movies'),
    tv: () => go('/tv'),
    games: () => go('/games'),
    music: () => go('/music'),
    lists: () => go('/lists'),
    taste: () => go('/taste'),
  });

  const motionProps = appearance.motion === 'off' ? { reducedMotion: 'always' as const, transition: { duration: 0 } } : appearance.motion === 'reduced' ? { reducedMotion: 'always' as const } : { reducedMotion: 'user' as const };

  let body;
  if (path === '/tray') {
    body = (
      <Suspense fallback={null}>
        <Tray />
      </Suspense>
    );
  } else if (error) {
    body = (
      <div className="grid h-full place-items-center bg-bg">
        <ErrorState error={error} onRetry={retry} />
      </div>
    );
  } else if (!boot) {
    // First paint: the logo with its bulbs lit, on the theme's own background.
    body = (
      <div className="grid h-full place-items-center bg-bg" aria-busy="true" aria-label="Starting Marquee">
        <Logo size={72} active />
      </div>
    );
  } else if (!boot.onboarded) {
    body = (
      <Suspense fallback={<div className="h-full bg-bg" />}>
        <Onboarding />
      </Suspense>
    );
  } else {
    const top = path.split('/')[1] ?? 'home';
    body = (
      <LayoutGroup>
        <div className="flex h-full">
          <Sidebar />
          <div className="flex min-w-0 flex-1 flex-col">
            <TopBar />
            <main ref={scroller} id="main" className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden bg-bg pt-5" tabIndex={-1}>
              <AnimatePresence mode="wait" initial={false}>
                <motion.div key={top} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.18, ease: [0.2, 0.7, 0.2, 1] }} className="min-h-full pb-16">
                  <Suspense fallback={<PageFallback />}>
                    <Page path={path} />
                  </Suspense>
                </motion.div>
              </AnimatePresence>
            </main>
          </div>
        </div>
        <AnimatePresence>
          {item && (
            <Suspense fallback={null}>
              <Detail key="detail" id={item} />
            </Suspense>
          )}
        </AnimatePresence>
        <CommandPalette />
        <CrashNotice />
        <Suspense fallback={null}>
          <UpdateModal />
        </Suspense>
      </LayoutGroup>
    );
  }
  return (
    <MotionConfig {...motionProps}>
      {body}
      <Toasts />
    </MotionConfig>
  );
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Shell />
    </QueryClientProvider>
  );
}
