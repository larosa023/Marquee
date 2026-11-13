// Global search. Two modes share one box:
//  - Titles: your library and lists answer instantly as you type; the catalogs follow a moment later.
//  - Vibe: describe a mood ("a short cozy game for a rainy Sunday") and get matches across media.
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '@/lib/api';
import { credit, noun, releaseLabel, typeBadge } from '@/lib/format';
import { MEDIA, type Card, type Media } from '@/lib/types';
import { useApp, useSetting } from '@/state/app';
import { Art, CardGrid, GridSkeleton, MatchRing } from '@/ui/Card';
import { useCardMenu } from '@/ui/CardMenu';
import { Icon, mediaIcon } from '@/ui/icons';
import { Badge, Button, Chip, EmptyState, ErrorState, IconButton, Kbd, Segmented, cx } from '@/ui/primitives';

type Tab = 'all' | Media | 'lists';
const VIBE_IDEAS = ['a short cozy game for a rainy Sunday', 'albums that sound like Blade Runner', 'slow-burn sci-fi with a big idea', 'something funny and under two hours', 'melancholic but hopeful', 'games like Outer Wilds'];

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

function ResultRow({ card, active, origin, onHover, id }: { card: Card; active: boolean; origin: string; onHover: () => void; id: string }) {
  const openItem = useApp((s) => s.openItem);
  const morph = useApp((s) => s.morphId === card.id && s.itemOrigin === origin);
  const sub = credit(card);
  return (
    <li id={id} role="option" aria-selected={active}>
      <button type="button" tabIndex={-1} onMouseMove={onHover} onClick={() => openItem(card.id, origin)} className={cx('flex w-full items-center gap-3.5 rounded-card p-2 pr-3 text-left transition-colors', active ? 'bg-hover' : 'hover:bg-hover')}>
        <motion.span layoutId={morph ? `art-${origin}-${card.id}` : undefined} className="block h-18 w-12 shrink-0 overflow-hidden rounded-md bg-strong">
          <Art card={card} className="h-full w-full" />
        </motion.span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[15px] font-semibold">{card.title}</span>
          <span className="tnum mt-0.5 flex items-center gap-1.5 truncate text-[13px] text-subtext">
            <Icon name={mediaIcon[card.media]} size={13} className="shrink-0 text-faint" />
            <span>{typeBadge(card)}</span>
            <span className="text-faint">·</span>
            <span>{card.year ?? releaseLabel(card.release_date)}</span>
            {sub && (
              <>
                <span className="text-faint">·</span>
                <span className="truncate">{sub}</span>
              </>
            )}
          </span>
        </span>
        {card.saved && <Badge tone="accent">Saved</Badge>}
        {card.done && <Badge tone="good">Done</Badge>}
        {card.match_pct != null && <MatchRing pct={card.match_pct} size={36} />}
      </button>
    </li>
  );
}

export default function Search({ initial }: { initial: string }) {
  const navigate = useApp((s) => s.navigate);
  const openItem = useApp((s) => s.openItem);
  const itemOpen = useApp((s) => s.item != null);
  const aiOn = useApp((s) => s.boot?.settings['ai']?.enabled !== false);
  const [recent, setRecent] = useSetting<string[]>('search.recent', []);
  const startsVibe = initial.startsWith('vibe:');
  const [mode, setMode] = useState<'titles' | 'vibe'>(startsVibe ? 'vibe' : 'titles');
  const [text, setText] = useState(startsVibe ? initial.slice(5) : initial);
  const [submitted, setSubmitted] = useState(startsVibe ? initial.slice(5) : '');
  const [tab, setTab] = useState<Tab>('all');
  const [cursor, setCursor] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const menu = useCardMenu();

  // A new route (from the command palette, or a "search for this artist" link) replaces what's typed.
  useEffect(() => {
    const vibe = initial.startsWith('vibe:');
    setMode(vibe ? 'vibe' : 'titles');
    setText(vibe ? initial.slice(5) : initial);
    setSubmitted(vibe ? initial.slice(5) : '');
    setTab('all');
  }, [initial]);
  useEffect(() => {
    if (!itemOpen) input.current?.focus({ preventScroll: true });
  }, [itemOpen, mode]);

  const q = text.trim();
  const debounced = useDebounced(q, 320);
  const media = tab !== 'all' && tab !== 'lists' ? tab : undefined;

  const local = useQuery({ queryKey: ['search', 'local', q], queryFn: () => api<{ cards: Card[]; lists: { id: number; name: string }[] }>('search.local', { query: q }), enabled: mode === 'titles' && q.length >= 2, placeholderData: keepPreviousData, staleTime: 5_000 });
  const remote = useQuery({ queryKey: ['search', 'catalog', debounced, media ?? 'all'], queryFn: () => api<{ cards: Card[]; offline: boolean }>('search', { query: debounced, media }), enabled: mode === 'titles' && debounced.length >= 2, placeholderData: keepPreviousData, staleTime: 60_000 });
  const vibe = useQuery({ queryKey: ['search', 'vibe', submitted], queryFn: () => api<{ cards: Card[]; engine: 'ollama' | 'tags'; understood: string[]; media: Media[]; like: string[] }>('vibe', { query: submitted }), enabled: mode === 'vibe' && submitted.length >= 3, staleTime: 5 * 60_000, retry: false });
  const ai = useQuery({ queryKey: ['ai.status'], queryFn: () => api<{ enabled: boolean; running: boolean; chat_model: string | null }>('ai.status'), enabled: mode === 'vibe', staleTime: 60_000 });

  const remember = (term: string) => {
    const t = term.trim();
    if (t.length < 2) return;
    setRecent([t, ...recent.filter((r) => r.toLowerCase() !== t.toLowerCase())].slice(0, 8));
  };

  // Titles mode: library hits first, then catalog hits that aren't already shown.
  const { mine, others, lists } = useMemo(() => {
    const mine = (q.length >= 2 ? (local.data?.cards ?? []) : []).filter((c) => !media || c.media === media);
    const seen = new Set(mine.map((c) => c.id));
    const others = (debounced.length >= 2 && q.length >= 2 ? (remote.data?.cards ?? []) : []).filter((c) => !seen.has(c.id) && (!media || c.media === media));
    const lists = q.length >= 2 ? (local.data?.lists ?? []) : [];
    return { mine: tab === 'lists' ? [] : mine, others: tab === 'lists' ? [] : others, lists: tab === 'all' || tab === 'lists' ? lists : [] };
  }, [local.data, remote.data, q, debounced, media, tab]);
  const flat = useMemo(() => [...lists.map((l) => ({ kind: 'list' as const, list: l })), ...mine.map((c) => ({ kind: 'card' as const, card: c, origin: 'search-mine' })), ...others.map((c) => ({ kind: 'card' as const, card: c, origin: 'search-all' }))], [lists, mine, others]);
  useEffect(() => setCursor(0), [q, tab]);
  useEffect(() => {
    document.getElementById(`sr-${cursor}`)?.scrollIntoView({ block: 'nearest' });
  }, [cursor]);

  const counts = useMemo(() => {
    const all = [...(local.data?.cards ?? []), ...(remote.data?.cards ?? [])];
    const ids = new Set<number>();
    const n: Record<string, number> = {};
    for (const c of all) {
      if (ids.has(c.id)) continue;
      ids.add(c.id);
      n[c.media] = (n[c.media] ?? 0) + 1;
    }
    return n;
  }, [local.data, remote.data]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (mode === 'vibe') {
      if (e.key === 'Enter' && q.length >= 3) {
        setSubmitted(q);
        remember(q);
      }
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setCursor((c) => Math.min(flat.length - 1, c + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setCursor((c) => Math.max(0, c - 1));
    } else if (e.key === 'Home' && flat.length) {
      e.preventDefault();
      setCursor(0);
    } else if (e.key === 'End' && flat.length) {
      e.preventDefault();
      setCursor(flat.length - 1);
    } else if (e.key === 'Enter') {
      const hit = flat[cursor];
      if (!hit) return;
      remember(q);
      if (hit.kind === 'list') navigate(`/lists/${hit.list.id}`);
      else openItem(hit.card.id, hit.origin);
    } else if (e.key === 'Escape' && text) {
      e.preventDefault();
      setText('');
    }
  };

  const searching = mode === 'titles' && q.length >= 2 && (remote.isFetching || debounced !== q);
  const nothing = mode === 'titles' && q.length >= 2 && !searching && flat.length === 0 && !remote.error;
  let index = -1;

  return (
    <div className="mx-auto max-w-5xl px-(--pad)">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <h1 className="display text-[30px] font-bold leading-tight">Search</h1>
        <Segmented
          label="Search mode"
          value={mode}
          onChange={(m) => (setMode(m), setCursor(0))}
          options={[
            { value: 'titles', label: 'Titles', icon: 'search' },
            { value: 'vibe', label: 'Vibe', icon: 'sparkles' },
          ]}
        />
      </div>

      <div className="relative">
        <Icon name={mode === 'vibe' ? 'sparkles' : 'search'} size={20} className={cx('pointer-events-none absolute left-4 top-1/2 -translate-y-1/2', mode === 'vibe' ? 'text-accent' : 'text-faint')} />
        <input
          ref={input}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          role="combobox"
          aria-expanded={mode === 'titles' && flat.length > 0}
          aria-controls="search-results"
          aria-activedescendant={mode === 'titles' && flat.length > 0 ? `sr-${cursor}` : undefined}
          aria-label={mode === 'vibe' ? 'Describe what you are in the mood for' : 'Search movies, shows, games, music and your lists'}
          placeholder={mode === 'vibe' ? 'Describe a mood: "a short cozy game for a rainy Sunday"' : 'Movies, shows, games, albums, artists, your lists'}
          spellCheck={false}
          autoComplete="off"
          className="hairline h-14 w-full rounded-card bg-surface pl-12 pr-28 text-[17px] placeholder:text-faint focus:border-accent"
        />
        <div className="absolute right-3 top-1/2 flex -translate-y-1/2 items-center gap-1.5">
          {text && <IconButton icon="x" label="Clear" onClick={() => (setText(''), setSubmitted(''), input.current?.focus())} />}
          {mode === 'vibe' && (
            <Button variant="primary" size="sm" disabled={q.length < 3} onClick={() => (setSubmitted(q), remember(q))}>
              Find
            </Button>
          )}
        </div>
      </div>

      {mode === 'vibe' && (
        <p className="mt-2 flex flex-wrap items-center gap-x-2 text-[12px] text-faint">
          <Icon name={ai.data?.running && aiOn ? 'cpu' : 'tag'} size={13} />
          {ai.data?.running && aiOn ? `Understood by ${ai.data.chat_model ?? 'your local model'} through Ollama, on this PC. Nothing is sent anywhere.` : 'Matched by mood and theme tags. Start Ollama and Marquee will understand full sentences, still entirely on this PC.'}
        </p>
      )}

      {mode === 'titles' && q.length >= 2 && (
        <div className="mt-4 flex flex-wrap items-center gap-1.5" role="tablist" aria-label="Filter results by type">
          {([['all', 'All', undefined], ...MEDIA.map((m) => [m.id, m.plural, counts[m.id]] as const), ['lists', 'Lists', local.data?.lists.length]] as [Tab, string, number | undefined][]).map(([id, label, n]) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              onClick={() => setTab(id)}
              className={cx('inline-flex h-8 items-center gap-1.5 rounded-full px-3 text-[13px] font-medium transition-colors', tab === id ? 'bg-accent text-on-accent' : 'hairline bg-surface text-subtext hover:bg-hover hover:text-text')}
            >
              {id !== 'all' && id !== 'lists' && <Icon name={mediaIcon[id]} size={14} />}
              {label}
              {n ? <span className="tnum opacity-70">{n}</span> : null}
            </button>
          ))}
          <span className="ml-auto hidden items-center gap-1.5 text-[12px] text-faint sm:flex">
            <Kbd>↑</Kbd>
            <Kbd>↓</Kbd> to move <Kbd>Enter</Kbd> to open
          </span>
        </div>
      )}

      <div className="mt-4" aria-live="polite" aria-busy={searching || vibe.isFetching}>
        {/* ------------------------------------------------------------ nothing typed */}
        {((mode === 'titles' && q.length < 2) || (mode === 'vibe' && !submitted)) && (
          <div className="grid gap-8 pt-4 md:grid-cols-2">
            <section aria-label="Recent searches">
              <div className="mb-2 flex items-center justify-between">
                <h2 className="text-[13px] font-semibold uppercase tracking-wider text-faint">Recent</h2>
                {recent.length > 0 && (
                  <button type="button" className="rounded-md text-[12px] text-faint hover:text-text" onClick={() => setRecent([])}>
                    Clear
                  </button>
                )}
              </div>
              {recent.length === 0 ? (
                <p className="text-[13px] text-subtext">Your recent searches will show up here.</p>
              ) : (
                <ul className="space-y-0.5">
                  {recent.map((r) => (
                    <li key={r}>
                      <button type="button" className="flex w-full items-center gap-2.5 rounded-control px-2.5 py-2 text-left hover:bg-hover" onClick={() => (setText(r), mode === 'vibe' && setSubmitted(r))}>
                        <Icon name="clock" size={15} className="text-faint" />
                        <span className="truncate">{r}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
            <section aria-label="Vibe search ideas">
              <h2 className="mb-2 text-[13px] font-semibold uppercase tracking-wider text-faint">Try a vibe</h2>
              <div className="flex flex-wrap gap-2">
                {VIBE_IDEAS.map((v) => (
                  <Chip key={v} icon="sparkles" onClick={() => (setMode('vibe'), setText(v), setSubmitted(v), remember(v))}>
                    {v}
                  </Chip>
                ))}
              </div>
            </section>
          </div>
        )}

        {/* ------------------------------------------------------------------- titles */}
        {mode === 'titles' && q.length >= 2 && (
          <>
            {remote.data?.offline && (
              <p className="mb-3 flex items-center gap-2 rounded-control bg-warn/12 px-3.5 py-2 text-[13px] text-warn">
                <Icon name="wifiOff" size={15} /> You're offline, so this is only what's already saved on this PC.
              </p>
            )}
            {remote.error && !remote.data && <ErrorState error={remote.error} onRetry={() => void remote.refetch()} />}
            <ul id="search-results" role="listbox" aria-label="Search results" className="space-y-0.5">
              {lists.length > 0 && (
                <li role="presentation" className="px-2 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
                  Your lists
                </li>
              )}
              {lists.map((l) => {
                index += 1;
                const i = index;
                return (
                  <li key={`l-${l.id}`} id={`sr-${i}`} role="option" aria-selected={cursor === i}>
                    <button type="button" tabIndex={-1} onMouseMove={() => setCursor(i)} onClick={() => navigate(`/lists/${l.id}`)} className={cx('flex w-full items-center gap-3 rounded-card p-2.5 text-left', cursor === i ? 'bg-hover' : 'hover:bg-hover')}>
                      <span className="grid h-9 w-9 place-items-center rounded-control bg-accent/12 text-accent">
                        <Icon name="list" size={17} />
                      </span>
                      <span className="font-semibold">{l.name}</span>
                      <span className="text-[13px] text-faint">List</span>
                    </button>
                  </li>
                );
              })}
              {mine.length > 0 && (
                <li role="presentation" className="px-2 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-faint">
                  In your library
                </li>
              )}
              {mine.map((c) => {
                index += 1;
                const i = index;
                return <ResultRow key={`m-${c.id}`} id={`sr-${i}`} card={c} origin="search-mine" active={cursor === i} onHover={() => setCursor(i)} />;
              })}
              {others.length > 0 && (
                <li role="presentation" className="px-2 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-faint">
                  Everything else
                </li>
              )}
              {others.map((c) => {
                index += 1;
                const i = index;
                return <ResultRow key={`o-${c.id}`} id={`sr-${i}`} card={c} origin="search-all" active={cursor === i} onHover={() => setCursor(i)} />;
              })}
            </ul>
            {searching && (
              <div className="mt-1 space-y-1" aria-hidden="true">
                {Array.from({ length: flat.length ? 2 : 5 }, (_, i) => (
                  <div key={i} className="flex items-center gap-3.5 p-2">
                    <div className="skeleton h-18 w-12 rounded-md!" />
                    <div className="flex-1">
                      <div className="skeleton h-4 w-1/3" />
                      <div className="skeleton mt-2 h-3 w-1/4" />
                    </div>
                  </div>
                ))}
              </div>
            )}
            {nothing && (
              <EmptyState
                icon="search"
                title={`Nothing found for "${q}"`}
                action={
                  <Button icon="sparkles" onClick={() => (setMode('vibe'), setSubmitted(q), remember(q))}>
                    Try it as a vibe
                  </Button>
                }
              >
                Check the spelling, try the original title, or switch the type tab back to All.
              </EmptyState>
            )}
          </>
        )}

        {/* --------------------------------------------------------------------- vibe */}
        {mode === 'vibe' && submitted && (
          <>
            {vibe.isPending && vibe.fetchStatus !== 'idle' && (
              <div className="-mx-(--pad) pt-2">
                <GridSkeleton count={12} />
              </div>
            )}
            {vibe.error && <ErrorState error={vibe.error} onRetry={() => void vibe.refetch()} />}
            {vibe.data && (
              <>
                <div className="mb-4 flex flex-wrap items-center gap-2 text-[13px]">
                  <span className="text-subtext">Heard:</span>
                  {vibe.data.understood.length === 0 && <span className="text-faint">nothing it recognised</span>}
                  {vibe.data.understood.map((u) => (
                    <span key={u} className="hairline rounded-full bg-surface px-2.5 py-0.5 text-subtext">
                      {u}
                    </span>
                  ))}
                  {vibe.data.like.map((u) => (
                    <span key={u} className="rounded-full bg-accent/15 px-2.5 py-0.5 text-accent">
                      like {u}
                    </span>
                  ))}
                  <span className="tnum ml-auto text-faint">
                    {vibe.data.cards.length} {noun(vibe.data.cards.length, 'match', 'matches')} · {vibe.data.engine === 'ollama' ? 'local AI' : 'tag matching'}
                  </span>
                </div>
                {vibe.data.cards.length === 0 ? (
                  <EmptyState icon="sparkles" title="No matches for that vibe">
                    {vibe.data.understood.length === 0 ? 'Try plainer mood words: cozy, dark, funny, slow-burn, epic, nostalgic, tense, dreamy. Or name something it should feel like.' : "Marquee understood the mood but hasn't got anything that fits yet. A sync brings in more titles."}
                  </EmptyState>
                ) : (
                  <div className="-mx-(--pad)">
                    <CardGrid cards={vibe.data.cards} origin="vibe" onContext={menu.onContext} showWhy />
                  </div>
                )}
              </>
            )}
          </>
        )}
      </div>
      {menu.element}
    </div>
  );
}
