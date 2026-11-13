// Ctrl+K: search, jump to a screen, add a title to a list, switch theme, sync now.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { api } from '@/lib/api';
import { typeBadge } from '@/lib/format';
import { img } from '@/lib/img';
import { appearanceFrom } from '@/lib/theme';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { useAddTo } from '@/ui/actions';
import { Icon, mediaIcon, type IconName } from '@/ui/icons';
import { Kbd, cx } from '@/ui/primitives';
import { NAV } from './Chrome';

interface Command {
  id: string;
  group: string;
  label: string;
  hint?: string;
  icon: IconName;
  image?: string | null;
  keywords?: string;
  run: () => void;
}

function score(query: string, text: string): number {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 1;
  if (t.startsWith(q)) return 100 - t.length * 0.01;
  const word = t.split(/\s+/).findIndex((w) => w.startsWith(q));
  if (word >= 0) return 80 - word;
  if (t.includes(q)) return 60;
  // Letters in order ("fy" finds "For You").
  let i = 0;
  for (const ch of t) if (ch === q[i]) i++;
  return i === q.length ? 30 : 0;
}

export function CommandPalette() {
  const open = useApp((s) => s.paletteOpen);
  const setOpen = useApp((s) => s.setPalette);
  const boot = useApp((s) => s.boot);
  const navigate = useApp((s) => s.navigate);
  const openItem = useApp((s) => s.openItem);
  const setSetting = useApp((s) => s.setSetting);
  const toast = useApp((s) => s.toast);
  const addTo = useAddTo();
  const qc = useQueryClient();
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (open) {
      setQuery('');
      setIndex(0);
      requestAnimationFrame(() => input.current?.focus());
    }
  }, [open]);

  // Titles already in the library and lists, as you type (no network).
  const q = query.trim();
  const { data: found } = useQuery({ queryKey: ['palette-search', q], queryFn: () => api<{ cards: Card[] }>('search.local', { query: q }), enabled: open && q.length >= 2, staleTime: 10_000 });

  const commands = useMemo<Command[]>(() => {
    if (!boot) return [];
    const appearance = appearanceFrom(boot.settings);
    const out: Command[] = [];
    for (const n of NAV) out.push({ id: `go-${n.path}`, group: 'Go to', label: n.label, icon: n.icon, run: () => navigate(n.path) });
    out.push({ id: 'go-rate', group: 'Go to', label: 'Quick Rate', icon: 'dice', keywords: 'swipe rate tune', run: () => navigate('/for-you/rate') });
    for (const l of boot.lists) out.push({ id: `list-${l.id}`, group: 'Lists', label: l.name, hint: `${l.count} saved`, icon: 'bookmark', run: () => navigate(`/lists/${l.id}`) });
    out.push({
      id: 'sync',
      group: 'Actions',
      label: 'Sync now',
      icon: 'sync',
      keywords: 'refresh update',
      run: () => {
        useApp.getState().set({ sync: { running: true, label: 'Starting', pct: 0 } });
        void api('sync').catch((e) => toast(e.message, 'bad'));
      },
    });
    out.push({
      id: 'backup',
      group: 'Actions',
      label: 'Back up now',
      icon: 'database',
      run: () =>
        void api('backup.create')
          .then((r) => toast(`Backed up to ${r.path}`, 'good'))
          .catch((e) => toast(e.message, 'bad')),
    });
    out.push({ id: 'updates', group: 'Actions', label: 'Check for updates', icon: 'download', run: () => navigate('/settings/updates') });
    out.push({ id: 'connect', group: 'Actions', label: 'Connect a service', icon: 'plug', keywords: 'steam spotify trakt letterboxd lastfm', run: () => navigate('/settings/connections') });
    out.push({ id: 'import', group: 'Actions', label: 'Import a file (CSV or JSON)', icon: 'upload', keywords: 'imdb backloggd rateyourmusic', run: () => navigate('/settings/connections') });
    for (const t of boot.themes) {
      out.push({
        id: `theme-${t.id}`,
        group: 'Theme',
        label: `Theme: ${t.name}`,
        hint: t.id === appearance.themeId ? 'Current' : t.description,
        icon: 'palette',
        keywords: 'appearance color dark light',
        run: () => void setSetting('appearance', { ...appearance, themeId: t.id, useSuiteTheme: false }),
      });
    }
    for (const m of ['full', 'reduced', 'off'] as const) out.push({ id: `motion-${m}`, group: 'Theme', label: `Motion: ${m[0].toUpperCase()}${m.slice(1)}`, icon: 'sparkles', keywords: 'animation', run: () => void setSetting('appearance', { ...appearance, motion: m }) });
    for (const [s, label] of [
      ['general', 'General'],
      ['connections', 'Connections'],
      ['keys', 'API keys'],
      ['taste', 'Taste'],
      ['streaming', 'Streaming services'],
      ['notifications', 'Notifications'],
      ['appearance', 'Appearance'],
      ['ai', 'AI'],
      ['storage', 'Storage'],
      ['backup', 'Backup & Restore'],
      ['updates', 'Updates'],
      ['shortcuts', 'Keyboard shortcuts'],
      ['about', 'About'],
    ]) {
      out.push({ id: `settings-${s}`, group: 'Settings', label: `Settings: ${label}`, icon: 'settings', run: () => navigate(`/settings/${s}`) });
    }
    return out;
  }, [boot, navigate, setSetting, toast]);

  const results = useMemo<Command[]>(() => {
    const ranked = commands
      .map((c) => ({ c, s: Math.max(score(q, c.label), score(q, c.keywords ?? '') * 0.6) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, q ? 8 : 14)
      .map((x) => x.c);
    const out: Command[] = [];
    if (q.length >= 2) {
      const titles = found?.cards ?? [];
      for (const card of titles.slice(0, 5)) {
        out.push({ id: `item-${card.id}`, group: 'In your library', label: card.title, hint: `${typeBadge(card)}${card.year ? ` · ${card.year}` : ''}`, icon: mediaIcon[card.media], image: card.poster, run: () => openItem(card.id) });
      }
      const top = titles[0];
      if (top && boot) {
        const slug = top.media === 'game' ? 'play-later' : top.media === 'album' ? 'listen-later' : 'watchlist';
        const target = boot.lists.find((l) => l.slug === slug);
        if (target && !top.saved) out.push({ id: `add-${top.id}`, group: 'In your library', label: `Add “${top.title}” to ${target.name}`, icon: 'plus', run: () => void addTo(top, `list:${target.id}`, target.name).then(() => qc.invalidateQueries({ queryKey: ['palette-search'] })) });
      }
    }
    // Matching commands come before the catch-all searches, so "calendar" + Enter goes to the Calendar.
    const searches: Command[] = [];
    if (q.length >= 2) {
      searches.push({ id: 'search-all', group: 'Search', label: `Search everything for “${q}”`, icon: 'search', run: () => navigate(`/search/${encodeURIComponent(q)}`) });
      searches.push({ id: 'vibe', group: 'Search', label: `Vibe search: “${q}”`, hint: 'Describe a mood, get matches across media', icon: 'sparkles', run: () => navigate(`/search/vibe:${encodeURIComponent(q)}`) });
    }
    return [...out, ...ranked, ...searches];
  }, [commands, q, found, boot, openItem, navigate, addTo, qc]);

  useEffect(() => setIndex(0), [q]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [index]);

  const run = (c: Command | undefined) => {
    if (!c) return;
    setOpen(false);
    c.run();
  };

  let lastGroup = '';
  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div className="fixed inset-0 z-80 flex justify-center px-6 pt-[14vh]" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.12 }}>
          <div className="absolute inset-0 bg-black/55 backdrop-blur-sm" onMouseDown={() => setOpen(false)} />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label="Command palette"
            className="panel relative flex h-fit max-h-[62vh] w-full max-w-xl flex-col overflow-hidden"
            initial={{ y: -14, scale: 0.97 }}
            animate={{ y: 0, scale: 1 }}
            exit={{ y: -8, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 520, damping: 36 }}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.stopPropagation();
                setOpen(false);
              }
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                setIndex((i) => Math.min(results.length - 1, i + 1));
              }
              if (e.key === 'ArrowUp') {
                e.preventDefault();
                setIndex((i) => Math.max(0, i - 1));
              }
              if (e.key === 'Enter') {
                e.preventDefault();
                run(results[index]);
              }
            }}
          >
            <div className="flex items-center gap-3 border-b border-white/8 px-4">
              <Icon name="search" size={18} className="text-faint" />
              <input
                ref={input}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search, jump to a screen, switch theme…"
                aria-label="Command"
                role="combobox"
                aria-expanded="true"
                aria-controls="palette-list"
                aria-activedescendant={results[index] ? `palette-${results[index].id}` : undefined}
                className="h-13 flex-1 bg-transparent text-[15px] placeholder:text-faint"
                spellCheck={false}
              />
              <Kbd>Esc</Kbd>
            </div>
            <ul ref={list} id="palette-list" role="listbox" className="min-h-0 flex-1 overflow-y-auto p-1.5">
              {results.length === 0 && <li className="px-4 py-8 text-center text-subtext">Nothing matches “{q}”. Press Enter on “Search everything” to look it up.</li>}
              {results.map((c, i) => {
                const heading = c.group !== lastGroup ? c.group : null;
                lastGroup = c.group;
                return (
                  <li key={c.id}>
                    {heading && <div className="px-2.5 pb-1 pt-2.5 text-[11px] font-semibold uppercase tracking-wider text-faint">{heading}</div>}
                    <button
                      type="button"
                      id={`palette-${c.id}`}
                      role="option"
                      aria-selected={i === index}
                      data-index={i}
                      tabIndex={-1}
                      onMouseMove={() => setIndex(i)}
                      onClick={() => run(c)}
                      className={cx('flex w-full items-center gap-3 rounded-control px-2.5 py-2 text-left', i === index ? 'bg-strong text-text' : 'text-subtext')}
                    >
                      {c.image ? <img src={img(c.image)} alt="" className="h-9 w-6.5 rounded-sm object-cover" /> : <Icon name={c.icon} size={17} className={i === index ? 'text-accent' : undefined} />}
                      <span className="min-w-0 flex-1 truncate">{c.label}</span>
                      {c.hint && <span className="shrink-0 truncate text-[12px] text-faint">{c.hint}</span>}
                    </button>
                  </li>
                );
              })}
            </ul>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
