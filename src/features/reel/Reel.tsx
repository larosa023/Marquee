/** The trailer reel: full-screen trailers for new and upcoming titles you'd probably like.
 * Next/previous with the arrow keys, the wheel or the buttons; S saves, X skips for good. */
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { useCallback, useEffect, useRef, useState } from 'react';
import { api, errorMessage } from '@/lib/api';
import { credit, releaseLabel } from '@/lib/format';
import type { Card, Media } from '@/lib/types';
import { useApp } from '@/state/app';
import { Reason } from '@/features/home/Home';
import { useDismiss, useQuickAdd } from '@/ui/actions';
import { Art, MatchRing, TypeBadge } from '@/ui/Card';
import { Icon } from '@/ui/icons';
import { Button, IconButton, Kbd, Segmented } from '@/ui/primitives';

type Kind = Media | 'any';

const KINDS: { value: Kind; label: string }[] = [
  { value: 'any', label: 'Everything' },
  { value: 'movie', label: 'Movies' },
  { value: 'tv', label: 'Shows' },
  { value: 'game', label: 'Games' },
];

export default function Reel() {
  const [kind, setKind] = useState<Kind>('any');
  const [items, setItems] = useState<Card[]>([]);
  const [index, setIndex] = useState(0);
  const [exhausted, setExhausted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<Set<number>>(new Set());
  const loading = useRef(false);
  const navigate = useApp((s) => s.navigate);
  const openItem = useApp((s) => s.openItem);
  const quickAdd = useQuickAdd();
  const dismiss = useDismiss();
  const reduce = useReducedMotion();
  const frame = useRef<HTMLIFrameElement>(null);
  // A title opened from the reel covers it: stop the trailer rather than play it underneath.
  const covered = useApp((s) => s.item != null);

  const load = useCallback(async (k: Kind, have: Card[]) => {
    if (loading.current) return;
    loading.current = true;
    try {
      const more = await api<Card[]>('reel', { media: k, skip: have.map((c) => c.id) });
      if (!more.length) setExhausted(true);
      setItems([...have, ...more]);
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      loading.current = false;
    }
  }, []);

  useEffect(() => {
    setItems([]);
    setIndex(0);
    setExhausted(false);
    void load(kind, []);
  }, [kind, load]);

  // Keep a few trailers queued ahead.
  useEffect(() => {
    if (!exhausted && items.length && index >= items.length - 3) void load(kind, items);
  }, [index, items, exhausted, kind, load]);

  const card = items[index];
  const next = useCallback(() => setIndex((i) => Math.min(i + 1, Math.max(0, items.length - 1))), [items.length]);
  const prev = useCallback(() => setIndex((i) => Math.max(0, i - 1)), []);
  const save = useCallback(async () => {
    if (!card || saved.has(card.id) || card.saved) return;
    await quickAdd(card);
    setSaved((s) => new Set(s).add(card.id));
  }, [card, quickAdd, saved]);
  const skip = useCallback(async () => {
    if (!card) return;
    void dismiss(card, 'other');
    next();
  }, [card, dismiss, next]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (useApp.getState().item != null) return;
      if (e.target instanceof HTMLInputElement || e.ctrlKey || e.metaKey || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === 'arrowdown' || k === 'arrowright' || k === 'j') next();
      else if (k === 'arrowup' || k === 'arrowleft' || k === 'k') prev();
      else if (k === 's') void save();
      else if (k === 'x') void skip();
      else if (k === 'enter' && card) openItem(card.id, 'reel');
      else if (k === 'escape') navigate('/home');
      else return;
      e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [next, prev, save, skip, card, openItem, navigate]);

  // One trailer per wheel gesture.
  const wheelLock = useRef(0);
  const onWheel = (e: React.WheelEvent) => {
    const now = Date.now();
    if (now - wheelLock.current < 700 || Math.abs(e.deltaY) < 30) return;
    wheelLock.current = now;
    if (e.deltaY > 0) next();
    else prev();
  };

  // Move on when a trailer finishes (YouTube's player reports state 0 = ended).
  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      if (!/youtube(-nocookie)?\.com$/.test(new URL(e.origin).hostname)) return;
      try {
        const m = typeof e.data === 'string' ? JSON.parse(e.data) : e.data;
        if ((m?.event === 'onStateChange' && m.info === 0) || (m?.event === 'infoDelivery' && m.info?.playerState === 0)) next();
      } catch {
        /* not ours */
      }
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [next]);
  const listen = () => frame.current?.contentWindow?.postMessage(JSON.stringify({ event: 'listening', id: 'marquee-reel' }), '*');

  const key = card?.meta?.trailer?.key as string | undefined;
  const sub = card ? credit(card) : null;
  const isSaved = !!card && (card.saved || saved.has(card.id));
  return (
    <div className="fixed inset-0 z-30 flex flex-col bg-black text-white" onWheel={onWheel} role="dialog" aria-label="Trailer reel">
      <div className="drag flex items-center gap-3 px-5 py-3">
        <div className="no-drag">
          <IconButton icon="arrowLeft" label="Leave the reel" onClick={() => navigate('/home')} />
        </div>
        <h1 className="font-semibold">Trailer reel</h1>
        <div className="no-drag ml-4">
          <Segmented label="Kind" size="sm" value={kind} onChange={setKind} options={KINDS} />
        </div>
        <span className="tnum ml-auto text-[13px] text-white/60">{items.length ? `${index + 1} / ${items.length}${exhausted ? '' : '+'}` : ''}</span>
      </div>
      <div className="flex min-h-0 flex-1 items-center gap-6 px-6 pb-6">
        <div className="relative flex min-w-0 flex-1 items-center justify-center self-stretch">
          {error ? (
            <p className="text-white/70">{error}</p>
          ) : !card ? (
            exhausted ? (
              <div className="max-w-md text-center text-white/70">
                <Icon name="play" size={34} className="mx-auto mb-3 opacity-50" />
                <p className="font-medium text-white">No trailers to show yet</p>
                <p className="mt-1">Trailers come with titles' details from TMDB and IGDB. Sync, or try another kind.</p>
              </div>
            ) : (
              <div className="skeleton aspect-video w-full max-w-[1400px] rounded-card" aria-busy="true" />
            )
          ) : (
            <AnimatePresence mode="wait">
              <motion.div key={card.id} initial={reduce ? { opacity: 0 } : { opacity: 0, y: 40 }} animate={{ opacity: 1, y: 0 }} exit={reduce ? { opacity: 0 } : { opacity: 0, y: -40 }} transition={{ duration: 0.25 }} className="aspect-video w-full max-w-[1400px] overflow-hidden rounded-card bg-black shadow-2xl">
                {key && !covered && (
                  <iframe
                    ref={frame}
                    title={`${card.title} trailer`}
                    className="h-full w-full bg-black"
                    style={{ colorScheme: 'dark' }}
                    src={`https://www.youtube-nocookie.com/embed/${key}?autoplay=1&rel=0&modestbranding=1&playsinline=1&enablejsapi=1&origin=${encodeURIComponent(location.origin)}`}
                    allow="autoplay; encrypted-media; fullscreen"
                    allowFullScreen
                    referrerPolicy="strict-origin-when-cross-origin"
                    onLoad={listen}
                  />
                )}
              </motion.div>
            </AnimatePresence>
          )}
        </div>
        {card && (
          <aside className="flex w-80 shrink-0 flex-col gap-3 self-center" aria-label="About this trailer">
            <div className="flex gap-3">
              <button type="button" className="w-20 shrink-0 overflow-hidden rounded-control" onClick={() => openItem(card.id, 'reel')} aria-label={`Open ${card.title}`}>
                <Art card={card} className="aspect-2/3 w-full" eager />
              </button>
              <div className="min-w-0">
                <TypeBadge card={card} />
                <h2 className="display mt-1.5 text-[22px] font-bold leading-tight">{card.title}</h2>
                <p className="text-[13px] text-white/70">
                  {releaseLabel(card.release_date)}
                  {sub ? ` · ${sub}` : ''}
                </p>
              </div>
            </div>
            {card.match_pct != null && (
              <div className="flex items-center gap-2 text-[13px] text-white/70">
                <MatchRing pct={card.match_pct} size={30} stroke={3} /> match
              </div>
            )}
            {card.why.slice(0, 2).map((w) => (
              <Reason key={w} text={w} className="text-[13px] text-white/75" />
            ))}
            <div className="mt-1 grid grid-cols-2 gap-2">
              <Button variant="primary" icon={isSaved ? 'check' : 'plus'} onClick={() => void save()} disabled={isSaved}>
                {isSaved ? 'Saved' : 'Save'}
              </Button>
              <Button icon="thumbDown" onClick={() => void skip()}>
                Not for me
              </Button>
              <Button icon="chevronRight" onClick={() => openItem(card.id, 'reel')}>
                Details
              </Button>
              <Button icon="chevronDown" onClick={next} disabled={index >= items.length - 1 && exhausted}>
                Next
              </Button>
            </div>
            <p className="mt-2 text-[12px] leading-relaxed text-white/50">
              <Kbd>↓</Kbd> next · <Kbd>↑</Kbd> back · <Kbd>S</Kbd> save · <Kbd>X</Kbd> not for me · <Kbd>Enter</Kbd> details · scroll works too
            </p>
            {items.length > index + 1 && (
              <div className="mt-2">
                <p className="mb-1.5 text-[12px] text-white/50">Up next</p>
                <ol className="flex gap-2">
                  {items.slice(index + 1, index + 5).map((c, i) => (
                    <li key={c.id} className="w-14">
                      <button type="button" className="block w-full overflow-hidden rounded-md opacity-70 transition-opacity hover:opacity-100" onClick={() => setIndex(index + 1 + i)} aria-label={`Play ${c.title}`}>
                        <Art card={c} className="aspect-2/3 w-full" />
                      </button>
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </aside>
        )}
      </div>
    </div>
  );
}
