// The mini panel that opens from the tray icon: what's out today, a quick search, sync, quit.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api, errorMessage, on, shell } from '@/lib/api';
import { ago, credit, typeBadge } from '@/lib/format';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { Art, MatchRing } from '@/ui/Card';
import { Icon, mediaIcon } from '@/ui/icons';
import { Logo } from '@/ui/Logo';
import { Button, IconButton } from '@/ui/primitives';

export default function Tray() {
  const qc = useQueryClient();
  const boot = useApp((s) => s.boot);
  const sync = useApp((s) => s.sync);
  const unread = useApp((s) => s.unread);
  const [query, setQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const { data, isPending } = useQuery({ queryKey: ['today'], queryFn: () => api<Card[]>('today', { limit: 6 }), enabled: !!boot, staleTime: 60_000 });

  const open = (route?: string) => void shell('open_main', { route: route ?? null }).catch(() => {});
  const hide = () => void shell('tray_hide').catch(() => {});

  useEffect(() => {
    document.documentElement.dataset.window = 'tray';
    const off = on('tray:opened', () => {
      setQuery('');
      setError(null);
      void qc.invalidateQueries({ queryKey: ['today'] });
      void useApp.getState().refreshBoot();
      input.current?.focus();
    });
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && hide();
    window.addEventListener('keydown', onKey);
    return () => {
      off();
      window.removeEventListener('keydown', onKey);
    };
  }, [qc]);

  const lastSync = boot?.settings['last_sync']?.at as string | undefined;
  const syncNow = () => {
    setError(null);
    useApp.getState().set({ sync: { running: true, label: 'Starting', pct: 0.01 } });
    void api('sync').catch((e) => {
      setError(errorMessage(e));
      useApp.getState().set({ sync: { running: false, label: '', pct: 0 } });
    });
  };

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-card bg-bg text-text" role="dialog" aria-label="Marquee">
      <header className="flex items-center gap-2.5 px-4 pb-2 pt-3.5">
        <Logo size={24} active={sync.running} />
        <span className="display flex-1 text-[15px] font-bold">Marquee</span>
        {unread > 0 && (
          <button type="button" className="tnum flex h-7 items-center gap-1 rounded-full bg-accent/15 px-2.5 text-[12px] font-semibold text-accent hover:bg-accent/25" onClick={() => open('/home')} aria-label={`${unread} unread notifications. Open Marquee.`}>
            <Icon name="bell" size={13} /> {unread}
          </button>
        )}
        <IconButton icon="external" label="Open Marquee" onClick={() => open()} />
      </header>

      <form
        className="relative px-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (query.trim()) open(`/search/${encodeURIComponent(query.trim())}`);
        }}
      >
        <Icon name="search" size={16} className="pointer-events-none absolute left-7 top-1/2 -translate-y-1/2 text-faint" />
        <input ref={input} autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Quick search" aria-label="Quick search. Press Enter to search in Marquee." className="hairline h-9.5 w-full rounded-control bg-surface pl-9 pr-3 placeholder:text-faint focus:border-accent" />
      </form>

      <section aria-label="Out today" className="mt-3 flex min-h-0 flex-1 flex-col">
        <h2 className="px-4 pb-1.5 text-[11px] font-semibold uppercase tracking-wider text-faint">Out today</h2>
        <div className="min-h-0 flex-1 overflow-y-auto px-2">
          {!boot || isPending ? (
            <div className="space-y-2 px-2" aria-busy="true">
              {[0, 1, 2].map((i) => (
                <div key={i} className="flex items-center gap-3">
                  <div className="skeleton h-14 w-9.5 rounded-md!" />
                  <div className="flex-1">
                    <div className="skeleton h-3.5 w-2/3" />
                    <div className="skeleton mt-2 h-3 w-1/3" />
                  </div>
                </div>
              ))}
            </div>
          ) : !data || data.length === 0 ? (
            <div className="px-2 py-6 text-center">
              <Icon name="calendar" size={24} className="mx-auto text-faint" />
              <p className="mt-2 text-[13px] text-subtext">Nothing on your radar is out today.</p>
              <button type="button" className="mt-1 rounded-md text-[13px] font-medium text-accent hover:underline" onClick={() => open('/calendar')}>
                See what's coming
              </button>
            </div>
          ) : (
            <ul>
              {data.map((c) => (
                <li key={c.id}>
                  <button type="button" className="flex w-full items-center gap-3 rounded-control p-2 text-left hover:bg-hover" onClick={() => open(`/home?item=${c.id}`)}>
                    <span className="block h-14 w-9.5 shrink-0 overflow-hidden rounded-md bg-strong">
                      <Art card={c} className="h-full w-full" eager />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[13px] font-semibold">{c.title}</span>
                      <span className="flex items-center gap-1 truncate text-[12px] text-subtext">
                        <Icon name={mediaIcon[c.media]} size={12} className="shrink-0 text-faint" />
                        {typeBadge(c)}
                        {credit(c) ? ` · ${credit(c)}` : ''}
                      </span>
                    </span>
                    {c.match_pct != null && <MatchRing pct={c.match_pct} size={32} stroke={3} />}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>

      {error && (
        <p role="alert" className="mx-4 mb-1 rounded-control bg-bad/10 px-2.5 py-1.5 text-[12px] text-bad">
          {error}
        </p>
      )}
      <footer className="flex items-center gap-2 border-t border-white/8 px-3 py-2.5">
        <Button size="sm" icon="sync" disabled={sync.running} onClick={syncNow}>
          {sync.running ? 'Syncing' : 'Sync now'}
        </Button>
        <span className="tnum min-w-0 flex-1 truncate text-[12px] text-faint" aria-live="polite">
          {sync.running ? `${sync.label} · ${Math.round(sync.pct * 100)}%` : lastSync ? `Synced ${ago(lastSync)}` : 'Not synced yet'}
        </span>
        <Button size="sm" variant="ghost" icon="x" onClick={() => void shell('quit_app')}>
          Quit
        </Button>
      </footer>
    </div>
  );
}
