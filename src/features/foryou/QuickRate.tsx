// Quick Rate: a deck of cards to rate or skip, fast. Swipe, click, or use the keyboard.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion, useMotionValue, useTransform, type PanInfo } from 'framer-motion';
import { useCallback, useEffect, useState } from 'react';
import { api } from '@/lib/api';
import { credit, typeBadge } from '@/lib/format';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { useRate } from '@/ui/actions';
import { Art, TypeBadge } from '@/ui/Card';
import { Button, EmptyState, ErrorState, Kbd, PageHeader, Skeleton } from '@/ui/primitives';
import { Stars } from '@/ui/Stars';

function Deck({ cards, onRated }: { cards: Card[]; onRated: (rated: number) => void }) {
  const [index, setIndex] = useState(0);
  const [rated, setRated] = useState(0);
  const [leaving, setLeaving] = useState<1 | -1 | 0>(0);
  const rate = useRate();
  const x = useMotionValue(0);
  const rotate = useTransform(x, [-260, 260], [-14, 14]);
  const like = useTransform(x, [30, 140], [0, 1]);
  const nope = useTransform(x, [-140, -30], [1, 0]);
  const card = cards[index];

  const next = useCallback(
    (dir: 1 | -1 | 0) => {
      setLeaving(dir);
      setIndex((i) => i + 1);
      x.set(0);
    },
    [x],
  );
  const give = useCallback(
    (stars: number) => {
      if (!card) return;
      void rate(card, stars, true);
      setRated((n) => n + 1);
      next(stars >= 3 ? 1 : -1);
    },
    [card, rate, next],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === 'INPUT' || e.ctrlKey || e.altKey || e.metaKey) return;
      if (/^[1-5]$/.test(e.key)) give(Number(e.key));
      else if (e.key === 'ArrowRight') give(4.5);
      else if (e.key === 'ArrowLeft') give(2);
      else if (e.key === ' ' || e.key === 'ArrowDown' || e.key === 's') {
        e.preventDefault();
        next(0);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [give, next]);

  useEffect(() => {
    if (index >= cards.length) onRated(rated);
  }, [index, cards.length, rated, onRated]);

  if (!card) return null;
  const onDragEnd = (_: unknown, info: PanInfo) => {
    if (info.offset.x > 130 || info.velocity.x > 700) give(4.5);
    else if (info.offset.x < -130 || info.velocity.x < -700) give(2);
  };
  return (
    <div className="flex flex-col items-center px-(--pad)">
      <div className="tnum mb-4 text-[13px] text-faint" aria-live="polite">
        {index + 1} of {cards.length} · {rated} rated
      </div>
      <div className="relative h-[min(58vh,520px)] w-[min(86vw,360px)]">
        {/* The next card waits underneath. */}
        {cards[index + 1] && (
          <div className="absolute inset-0 translate-y-3 scale-[0.95] overflow-hidden rounded-[calc(var(--radius)+4px)] opacity-60">
            <Art card={cards[index + 1]} className="h-full w-full" />
          </div>
        )}
        <AnimatePresence initial={false} custom={leaving}>
          <motion.div
            key={card.id}
            className="shadow-card absolute inset-0 cursor-grab overflow-hidden rounded-[calc(var(--radius)+4px)] bg-surface active:cursor-grabbing"
            style={{ x, rotate }}
            drag="x"
            dragSnapToOrigin
            dragElastic={0.7}
            onDragEnd={onDragEnd}
            variants={{
              enter: { scale: 0.95, opacity: 0, y: 12 },
              shown: { scale: 1, opacity: 1, y: 0 },
              // Flies off the side it was rated to; a skip drops away.
              leave: (dir: number) => ({ x: dir === 0 ? 0 : dir * 520, y: dir === 0 ? 80 : 0, opacity: 0, rotate: dir * 18, transition: { duration: 0.28 } }),
            }}
            custom={leaving}
            initial="enter"
            animate="shown"
            exit="leave"
            transition={{ type: 'spring', stiffness: 380, damping: 30 }}
          >
            <Art card={card} className="h-full w-full" eager />
            <div className="absolute inset-0 bg-linear-to-t from-black/90 via-black/10 to-transparent" />
            <motion.div style={{ opacity: like }} className="absolute left-5 top-5 -rotate-12 rounded-lg border-[3px] border-good px-3 py-1 text-xl font-extrabold uppercase tracking-wider text-good">
              Loved it
            </motion.div>
            <motion.div style={{ opacity: nope }} className="absolute right-5 top-5 rotate-12 rounded-lg border-[3px] border-bad px-3 py-1 text-xl font-extrabold uppercase tracking-wider text-bad">
              Not for me
            </motion.div>
            <div className="absolute inset-x-0 bottom-0 p-5 text-white">
              <TypeBadge card={card} />
              <h2 className="display mt-2 text-2xl font-bold leading-tight">{card.title}</h2>
              <p className="tnum mt-0.5 text-[13px] text-white/75">{[card.year, credit(card)].filter(Boolean).join(' · ')}</p>
            </div>
          </motion.div>
        </AnimatePresence>
      </div>

      <div className="mt-6 flex items-center gap-4">
        <Button variant="danger" size="lg" icon="thumbDown" onClick={() => give(2)} aria-label={`Not for me: ${card.title}`}>
          Not for me
        </Button>
        <Stars value={null} onChange={(s) => s > 0 && give(s)} size={34} label={`Rate ${card.title} (${typeBadge(card)})`} />
        <Button variant="primary" size="lg" icon="heart" onClick={() => give(4.5)} aria-label={`Loved it: ${card.title}`}>
          Loved it
        </Button>
      </div>
      <Button variant="ghost" className="mt-3" onClick={() => next(0)}>
        Haven't seen it · skip
      </Button>
      <p className="mt-5 flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-[12px] text-faint">
        <span>
          <Kbd>1</Kbd>–<Kbd>5</Kbd> stars
        </span>
        <span>
          <Kbd>←</Kbd> not for me
        </span>
        <span>
          <Kbd>→</Kbd> loved it
        </span>
        <span>
          <Kbd>Space</Kbd> skip
        </span>
      </p>
    </div>
  );
}

export default function QuickRate() {
  const [round, setRound] = useState(0);
  const [done, setDone] = useState<number | null>(null);
  const navigate = useApp((s) => s.navigate);
  const qc = useQueryClient();
  const { data, error, isPending, refetch } = useQuery({ queryKey: ['quick_rate', round], queryFn: () => api<Card[]>('quick_rate', { limit: 24 }), staleTime: Infinity, gcTime: 0 });
  const onRated = useCallback(
    (rated: number) => {
      setDone(rated);
      void qc.invalidateQueries({ queryKey: ['home'] });
    },
    [qc],
  );
  return (
    <>
      <PageHeader title="Quick Rate" subtitle="Rate what you know, skip what you don't. Every rating sharpens your picks." />
      {isPending ? (
        <div className="flex justify-center px-(--pad)" aria-busy="true">
          <Skeleton className="h-[min(58vh,520px)] w-[min(86vw,360px)] rounded-[calc(var(--radius)+4px)]!" />
        </div>
      ) : error ? (
        <ErrorState error={error} onRetry={() => void refetch()} />
      ) : data.length === 0 ? (
        <EmptyState icon="dice" title="Nothing to rate right now" action={<Button onClick={() => navigate('/home')}>Back to Home</Button>}>
          Once the catalog has synced there'll be plenty here.
        </EmptyState>
      ) : done !== null ? (
        <EmptyState
          icon="sparkles"
          title={done > 0 ? `Nice. ${done} rated.` : 'All skipped'}
          action={
            <>
              <Button
                variant="primary"
                icon="dice"
                onClick={() => {
                  setDone(null);
                  setRound((r) => r + 1);
                }}
              >
                Another round
              </Button>
              <Button icon="sparkles" onClick={() => navigate('/for-you')}>
                See my picks
              </Button>
            </>
          }
        >
          {done > 0 ? 'Your taste profile has been updated and your picks rescored.' : 'No problem. Try another round for different titles.'}
        </EmptyState>
      ) : (
        <Deck key={round} cards={data} onRated={onRated} />
      )}
    </>
  );
}
