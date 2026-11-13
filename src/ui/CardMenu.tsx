// The right-click (or Shift+F10) menu for any card.
import { AnimatePresence } from 'framer-motion';
import { useCallback, useMemo, useState } from 'react';
import { doneVerb } from '@/lib/format';
import type { Card } from '@/lib/types';
import { EMPTY, useApp } from '@/state/app';
import { useAddTo, useDismiss, useDone, useMoreLike } from './actions';
import { Menu, type MenuItem } from './overlays';

export function useCardMenu() {
  const [state, setState] = useState<{ card: Card; at: { x: number; y: number } } | null>(null);
  const onContext = useCallback((card: Card, at: { x: number; y: number }) => setState({ card, at }), []);
  const lists = useApp((s) => s.boot?.lists ?? EMPTY);
  const sources = useApp((s) => s.boot?.sources ?? EMPTY);
  const openItem = useApp((s) => s.openItem);
  const addTo = useAddTo();
  const done = useDone();
  const dismiss = useDismiss();
  const moreLike = useMoreLike();

  const items = useMemo<MenuItem[]>(() => {
    if (!state) return [];
    const c = state.card;
    const connected = (id: string) => sources.some((s) => s.id === id && s.status !== 'disconnected');
    const out: MenuItem[] = [
      { id: 'open', label: 'Open', icon: 'external', run: () => openItem(c.id) },
      { id: 's1', label: '', separator: true },
      { id: 'h1', label: 'Add to', heading: true },
    ];
    for (const l of lists) out.push({ id: `list-${l.id}`, label: l.name, icon: 'bookmark', run: () => void addTo(c, `list:${l.id}`, l.name) });
    if (c.media === 'movie') out.push({ id: 'lb', label: 'Letterboxd watchlist', icon: 'external', hint: 'Opens the film page', run: () => void addTo(c, 'letterboxd:watchlist', 'your Letterboxd watchlist') });
    if ((c.media === 'movie' || c.media === 'tv') && connected('trakt')) out.push({ id: 'trakt', label: 'Trakt watchlist', icon: 'cloud', run: () => void addTo(c, 'trakt:watchlist', 'your Trakt watchlist') });
    if (c.media === 'game' && connected('steam')) out.push({ id: 'steam', label: 'Steam wishlist', icon: 'external', hint: 'Opens the store page', run: () => void addTo(c, 'steam:wishlist', 'your Steam wishlist') });
    if (c.media === 'album' && connected('spotify')) {
      out.push({ id: 'sp-lib', label: 'Spotify library', icon: 'cloud', run: () => void addTo(c, 'spotify:library', 'your Spotify library') });
      out.push({ id: 'sp-picks', label: 'Marquee Picks playlist', icon: 'cloud', run: () => void addTo(c, 'spotify:playlist:picks', 'Marquee Picks') });
    }
    out.push(
      { id: 's2', label: '', separator: true },
      { id: 'done', label: c.done ? `Not ${doneVerb(c.media).toLowerCase()}` : `Mark as ${doneVerb(c.media).toLowerCase()}`, icon: c.done ? 'eyeOff' : 'eye', run: () => void done(c, !c.done) },
      { id: 'more', label: 'More like this', icon: 'sparkles', run: () => void moreLike(c) },
      { id: 's3', label: '', separator: true },
      { id: 'h2', label: 'Not interested', heading: true },
      { id: 'seen', label: "I've seen it", icon: 'check', run: () => void dismiss(c, 'seen') },
      { id: 'genre', label: 'Not my genre', icon: 'ban', run: () => void dismiss(c, 'genre') },
      { id: 'creator', label: c.media === 'album' ? "I don't like this artist" : "I don't like who made it", icon: 'thumbDown', run: () => void dismiss(c, 'creator') },
      { id: 'other', label: 'Just hide it', icon: 'eyeOff', run: () => void dismiss(c, 'other') },
    );
    return out;
  }, [state, lists, sources, addTo, done, dismiss, moreLike, openItem]);

  const element = <AnimatePresence>{state && <Menu at={state.at} items={items} onClose={() => setState(null)} label={`Actions for ${state.card.title}`} />}</AnimatePresence>;
  return { onContext, element };
}
