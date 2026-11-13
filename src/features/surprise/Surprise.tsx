/** "Surprise me": one random title you'd probably like, for when you need a spark. */
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { useCallback, useEffect, useRef, useState } from 'react';
import { api, errorMessage } from '@/lib/api';
import { credit, releaseLabel } from '@/lib/format';
import type { Card, Media } from '@/lib/types';
import { useApp } from '@/state/app';
import { Reason } from '@/features/home/Home';
import { useQuickAdd } from '@/ui/actions';
import { Art, MatchRing, TypeBadge } from '@/ui/Card';
import { Icon } from '@/ui/icons';
import { Modal } from '@/ui/overlays';
import { Button, Segmented, Toggle } from '@/ui/primitives';

type Kind = Media | 'any';

const KINDS: { value: Kind; label: string; icon: 'film' | 'tv' | 'gamepad' | 'music' | 'dice' }[] = [
  { value: 'movie', label: 'Movie', icon: 'film' },
  { value: 'tv', label: 'Show', icon: 'tv' },
  { value: 'game', label: 'Game', icon: 'gamepad' },
  { value: 'album', label: 'Album', icon: 'music' },
  { value: 'any', label: 'Anything', icon: 'dice' },
];

const NOUN: Record<Kind, string> = { movie: 'movie', tv: 'show', game: 'game', album: 'album', any: 'title' };

export default function Surprise() {
  const kind = useApp((s) => s.surprise);
  const close = () => useApp.getState().set({ surprise: null });
  return (
    <Modal open={kind != null} onClose={close} title="Surprise me" width={620}>
      {kind != null && <Roller initial={kind} onDone={close} />}
    </Modal>
  );
}

function Roller({ initial, onDone }: { initial: Kind; onDone: () => void }) {
  const [kind, setKind] = useState<Kind>(initial);
  const [upcoming, setUpcoming] = useState(false);
  const [card, setCard] = useState<Card | null | undefined>(undefined);
  const [rolling, setRolling] = useState(false);
  const [spins, setSpins] = useState(0);
  const [error, setError] = useState<string | null>(null);
  // The last few picks, so a re-roll always lands somewhere new.
  const seen = useRef<number[]>([]);
  const reduce = useReducedMotion();
  const quickAdd = useQuickAdd();
  const openItem = useApp((s) => s.openItem);

  const roll = useCallback(
    async (k: Kind, up: boolean) => {
      setRolling(true);
      setSpins((n) => n + 1);
      setError(null);
      try {
        const [next] = await Promise.all([api<Card | null>('surprise', { media: k, upcoming: up, skip: seen.current }), new Promise((r) => setTimeout(r, reduce ? 0 : 420))]);
        let pick = next;
        if (!pick && seen.current.length) {
          // Out of fresh ones: start the cycle again.
          seen.current = [];
          pick = await api<Card | null>('surprise', { media: k, upcoming: up, skip: [] });
        }
        if (pick) seen.current = [pick.id, ...seen.current].slice(0, 30);
        setCard(pick);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setRolling(false);
      }
    },
    [reduce],
  );

  useEffect(() => {
    void roll(kind, upcoming);
    // Re-roll whenever the kind or the upcoming switch changes.
  }, [kind, upcoming, roll]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.key === 'r' || e.key === ' ') && !(e.target instanceof HTMLInputElement) && !rolling) {
        e.preventDefault();
        void roll(kind, upcoming);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [kind, upcoming, roll, rolling]);

  const sub = card ? credit(card) : null;
  return (
    <div className="flex flex-col gap-4">
      <p className="text-subtext">Can't decide? Roll for something you'd probably like.</p>
      <Segmented label="Kind of title" value={kind} onChange={(v) => setKind(v)} options={KINDS} size="sm" />
      <div className="relative min-h-[300px]" aria-live="polite" aria-busy={rolling}>
        <AnimatePresence mode="wait">
          {error ? (
            <motion.div key="error" initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="grid h-[300px] place-items-center text-center text-subtext">
              <div>
                <Icon name="alert" size={26} className="mx-auto mb-2 text-warn" />
                {error}
              </div>
            </motion.div>
          ) : card === null ? (
            <motion.div key="empty" initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="grid h-[300px] place-items-center text-center text-subtext">
              <div className="max-w-sm">
                <Icon name="dice" size={30} className="mx-auto mb-3 text-faint" />
                <p className="font-medium text-text">Nothing to roll yet</p>
                <p className="mt-1">
                  No {NOUN[kind]}s you haven't seen{upcoming ? '' : ' that are out'} yet. {upcoming ? 'Sync to fill the catalog.' : 'Try including upcoming ones, or sync to fill the catalog.'}
                </p>
              </div>
            </motion.div>
          ) : card === undefined || rolling ? (
            <motion.div key={`roll-${spins}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="grid h-[300px] place-items-center">
              <motion.div animate={reduce ? undefined : { rotate: [0, 90, 200, 300, 360], scale: [1, 1.12, 0.95, 1.06, 1] }} transition={{ duration: 0.42, ease: 'easeOut' }} className="text-accent">
                <Icon name="dice" size={54} />
              </motion.div>
              <span className="sr-only">Rolling…</span>
            </motion.div>
          ) : (
            <motion.div key={`card-${card.id}`} initial={reduce ? { opacity: 0 } : { opacity: 0, y: 14, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0 }} transition={{ type: 'spring', stiffness: 380, damping: 30 }} className="flex gap-5">
              <button type="button" className="w-[180px] shrink-0 overflow-hidden rounded-card shadow-lg focus-visible:outline-2" onClick={() => (onDone(), openItem(card.id, 'surprise'))} aria-label={`Open ${card.title}`}>
                <Art card={card} className={card.media === 'album' ? 'w-full' : 'aspect-2/3 w-full'} eager large />
              </button>
              <div className="flex min-w-0 flex-1 flex-col gap-2">
                <div className="flex items-center gap-2">
                  <TypeBadge card={card} />
                  <span className="tnum text-[13px] text-subtext">{releaseLabel(card.release_date)}</span>
                </div>
                <h3 className="display text-[24px] font-bold leading-tight">{card.title}</h3>
                {sub && <p className="text-subtext">{sub}</p>}
                {card.match_pct != null && (
                  <div className="flex items-center gap-2 text-[13px] text-subtext">
                    <MatchRing pct={card.match_pct} size={30} stroke={3} />
                    match
                  </div>
                )}
                {card.why.slice(0, 2).map((w) => (
                  <Reason key={w} text={w} className="text-[13.5px] text-subtext" />
                ))}
                {card.overview && <p className="clamp-3 text-[13.5px] text-subtext">{card.overview}</p>}
                <div className="mt-auto flex flex-wrap gap-2 pt-2">
                  <Button variant="primary" icon="chevronRight" onClick={() => (onDone(), openItem(card.id, 'surprise'))}>
                    Open
                  </Button>
                  <Button icon="plus" onClick={() => void quickAdd(card)}>
                    {card.saved ? 'Saved' : 'Save for later'}
                  </Button>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-white/8 pt-3">
        <div className="min-w-[220px]">
          <Toggle checked={upcoming} onChange={setUpcoming} label="Include upcoming" />
        </div>
        <Button variant="primary" icon="dice" busy={rolling} onClick={() => void roll(kind, upcoming)}>
          {card ? `Roll another ${kind === 'any' ? 'one' : NOUN[kind]}` : 'Roll'}
        </Button>
      </div>
      <p className="text-[12px] text-faint">
        Press <kbd>R</kbd> to roll again. Picks lean toward your best matches; set the taste dial to Adventurous for wilder ones.
      </p>
    </div>
  );
}
