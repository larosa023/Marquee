// The poster card every row and grid is made of, plus the match ring and the type badge.
import { motion, useReducedMotion } from 'framer-motion';
import { memo, useEffect, useState, type MouseEvent } from 'react';
import { credit, releaseLabel, typeBadge } from '@/lib/format';
import { img } from '@/lib/img';
import type { Card as CardData } from '@/lib/types';
import { useApp } from '@/state/app';
import { useQuickAdd } from './actions';
import { Icon, mediaIcon } from './icons';
import { cx } from './primitives';

/** A ring that fills to the match percentage. Colour goes from muted to the accent as the match improves. */
export function MatchRing({ pct, size = 38, stroke = 3.5, label = true }: { pct: number; size?: number; stroke?: number; label?: boolean }) {
  const r = (size - stroke) / 2;
  const reduce = useReducedMotion();
  const strong = pct >= 75;
  return (
    <div className="relative grid place-items-center" style={{ width: size, height: size }} role="img" aria-label={`${pct}% match`}>
      <svg width={size} height={size} className="-rotate-90">
        <circle cx={size / 2} cy={size / 2} r={r} fill="rgb(0 0 0 / 0.55)" stroke="rgb(255 255 255 / 0.16)" strokeWidth={stroke} />
        <motion.circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={strong ? 'var(--accent)' : 'rgb(255 255 255 / 0.75)'}
          strokeWidth={stroke}
          strokeLinecap="round"
          initial={reduce ? false : { pathLength: 0 }}
          animate={{ pathLength: pct / 100 }}
          transition={{ duration: 0.9, ease: [0.2, 0.8, 0.2, 1], delay: 0.15 }}
        />
      </svg>
      {label && (
        <span className="tnum absolute text-[11px] font-bold text-white" style={{ fontSize: size * 0.3 }}>
          {pct}
        </span>
      )}
    </div>
  );
}

export function TypeBadge({ card }: { card: Pick<CardData, 'media' | 'meta'> }) {
  return (
    <span className="inline-flex h-5.5 items-center gap-1 rounded-md bg-black/60 px-1.5 text-[11px] font-semibold uppercase tracking-wide text-white backdrop-blur-sm">
      <Icon name={card.meta?.anime === true ? 'anime' : mediaIcon[card.media]} size={12} />
      {typeBadge(card)}
    </span>
  );
}

/** Poster or cover art with a designed fallback: a tinted tile with the title, never a broken image. */
/** Grid cards are ~180px wide: TMDB's 342px poster is sharp at 2x and a third the size of w500. */
export function sized(url: string | null | undefined, large?: boolean): string | null | undefined {
  if (!url || large) return url;
  return url.replace('image.tmdb.org/t/p/w500/', 'image.tmdb.org/t/p/w342/').replace('image.tmdb.org/t/p/original/', 'image.tmdb.org/t/p/w342/');
}

export function Art({ card, className, eager, large }: { card: Pick<CardData, 'title' | 'poster' | 'media'> & { meta?: CardData['meta'] }; className?: string; eager?: boolean; large?: boolean }) {
  // Steam's portrait art is missing for some games; its wide header art always exists.
  const sources = [sized(card.poster, large), typeof card.meta?.header === 'string' ? card.meta.header : null].filter(Boolean) as string[];
  const [attempt, setAttempt] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const failed = attempt >= sources.length;
  const onError = () => {
    setLoaded(false);
    setAttempt((a) => a + 1);
  };
  useEffect(() => {
    setAttempt(0);
    setLoaded(false);
  }, [card.poster]);
  const src = img(sources[attempt]);
  // Cached art is often ready before React attaches onLoad; catch that so it never stays hidden.
  const ref = (el: HTMLImageElement | null) => {
    if (el?.complete && el.naturalWidth > 0 && !loaded) setLoaded(true);
  };
  if (!src || failed) {
    return (
      <div className={cx('flex flex-col items-center justify-center gap-2 bg-linear-to-br from-strong to-surface p-3 text-center text-faint', className)}>
        <Icon name={mediaIcon[card.media]} size={26} />
        <span className="clamp-3 text-[12px] font-medium text-subtext">{card.title}</span>
      </div>
    );
  }
  if (card.media === 'album') {
    // Covers are square: show the whole cover over a blurred copy of itself rather than cropping it.
    return (
      <div className={cx('relative overflow-hidden bg-surface', className)}>
        <img src={src} alt="" aria-hidden="true" className="absolute inset-0 h-full w-full scale-150 object-cover opacity-60 blur-xl" loading="lazy" />
        {!loaded && <div className="absolute inset-0 bg-strong/60" aria-hidden="true" />}
        <img
          ref={ref}
          src={src}
          alt=""
          crossOrigin="anonymous"
          className={cx('relative aspect-square w-full object-cover shadow-lg transition-opacity duration-150', loaded ? 'opacity-100' : 'opacity-0')}
          loading={eager ? 'eager' : 'lazy'}
          fetchPriority={eager ? 'high' : 'auto'}
          decoding="async"
          onLoad={() => setLoaded(true)}
          onError={onError}
        />
      </div>
    );
  }
  return (
    <div className={cx('relative overflow-hidden', className)}>
      {!loaded && <div className="absolute inset-0 bg-strong/60" aria-hidden="true" />}
      <img ref={ref} src={src} alt="" crossOrigin="anonymous" className={cx('h-full w-full object-cover transition-opacity duration-150', loaded ? 'opacity-100' : 'opacity-0')} loading={eager ? 'eager' : 'lazy'} fetchPriority={eager ? 'high' : 'auto'} decoding="async" onLoad={() => setLoaded(true)} onError={onError} />
    </div>
  );
}

interface PosterCardProps {
  card: CardData;
  /** Names the row or grid this card sits in, so the poster morphs from the right place. */
  origin: string;
  onContext?: (card: CardData, at: { x: number; y: number }) => void;
  width?: number | string;
  showWhy?: boolean;
}

export const PosterCard = memo(function PosterCard({ card, origin, onContext, width, showWhy }: PosterCardProps) {
  const openItem = useApp((s) => s.openItem);
  const morph = useApp((s) => s.morphId === card.id && s.itemOrigin === origin);
  const quickAdd = useQuickAdd();
  const reduce = useReducedMotion();
  const sub = credit(card);
  const context = (e: MouseEvent) => {
    if (!onContext) return;
    e.preventDefault();
    onContext(card, { x: e.clientX, y: e.clientY });
  };
  return (
    <motion.article
      data-card={card.id}
      className="group relative shrink-0"
      style={{ width: width ?? 'var(--card-w)' }}
      variants={{ hidden: { opacity: 0, y: 14 }, show: { opacity: 1, y: 0 } }}
      whileHover={reduce ? undefined : { y: -6 }}
      transition={{ type: 'spring', stiffness: 420, damping: 30 }}
      onContextMenu={context}
    >
      <div className="relative">
        <button
          type="button"
          onClick={() => openItem(card.id, origin)}
          onKeyDown={(e) => {
            if (onContext && (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))) {
              e.preventDefault();
              const r = e.currentTarget.getBoundingClientRect();
              onContext(card, { x: r.left + r.width / 2, y: r.top + r.height / 2 });
            }
          }}
          aria-label={`${card.title}, ${typeBadge(card)}${card.match_pct != null ? `, ${card.match_pct}% match` : ''}, ${releaseLabel(card.release_date)}`}
          className="block w-full rounded-card text-left"
        >
          <motion.div
            layoutId={morph ? `art-${origin}-${card.id}` : undefined}
            className={cx(
              // Album covers are square: give them a square box so the whole cover shows, uncropped.
              card.media === 'album' ? 'aspect-square' : 'aspect-2/3',
              'shadow-card relative overflow-hidden rounded-card bg-surface transition-shadow duration-300 group-hover:shadow-[0_14px_40px_rgb(0_0_0/0.5),0_0_0_1px_color-mix(in_srgb,var(--accent)_45%,transparent),0_0_38px_color-mix(in_srgb,var(--accent)_calc(var(--glow)*38%),transparent)]',
            )}
          >
            <Art card={card} className="h-full w-full" />
            <div className="pointer-events-none absolute inset-x-0 top-0 flex items-start justify-between p-2">
              <TypeBadge card={card} />
              {card.match_pct != null && <MatchRing pct={card.match_pct} />}
            </div>
            {(card.meta?.discount ?? 0) > 0 && <span className="tnum absolute bottom-2 left-2 rounded-md bg-good px-1.5 py-0.5 text-[12px] font-bold text-black">-{card.meta.discount}%</span>}
            {card.done && !(card.meta?.discount > 0) && (
              <span className="absolute bottom-2 left-2 grid h-6 w-6 place-items-center rounded-full bg-black/65 text-good" title="In your history">
                <Icon name="check" size={14} />
              </span>
            )}
          </motion.div>
        </button>
        <button
          type="button"
          aria-label={card.saved ? `Saved: ${card.title}` : `Save ${card.title}`}
          title={card.saved ? 'Saved' : 'Save for later'}
          onClick={() => quickAdd(card)}
          className={cx(
            'absolute bottom-2 right-2 grid h-9 w-9 place-items-center rounded-full shadow-lg transition-all duration-200 focus-visible:translate-y-0 focus-visible:opacity-100 group-hover:translate-y-0 group-hover:opacity-100',
            card.saved ? 'bg-accent text-on-accent opacity-100' : 'translate-y-1 bg-white text-black opacity-0 hover:scale-110',
          )}
        >
          <Icon name={card.saved ? 'check' : 'plus'} size={18} />
        </button>
      </div>
      <div className="px-0.5 pt-2.5" onClick={() => openItem(card.id, origin)}>
        <div className="truncate font-semibold leading-snug">{card.title}</div>
        <div className="tnum mt-0.5 flex items-center gap-1.5 truncate text-[13px] text-subtext">
          <span>{releaseLabel(card.release_date)}</span>
          {sub && (
            <>
              <span className="text-faint">·</span>
              <span className="truncate">{sub}</span>
            </>
          )}
        </div>
        {showWhy && card.why[0] && <div className="clamp-2 mt-1 text-[12px] leading-snug text-faint">{card.why[0].replaceAll('*', '')}</div>}
      </div>
    </motion.article>
  );
});

export function CardSkeleton({ width }: { width?: number | string }) {
  return (
    <div className="shrink-0" style={{ width: width ?? 'var(--card-w)' }} aria-hidden="true">
      <div className="skeleton aspect-2/3 w-full rounded-card!" />
      <div className="skeleton mt-3 h-4 w-4/5" />
      <div className="skeleton mt-2 h-3 w-1/2" />
    </div>
  );
}

/** A responsive grid of cards with a staggered entrance. */
export function CardGrid({ cards, origin, onContext, showWhy }: { cards: CardData[]; origin: string; onContext?: PosterCardProps['onContext']; showWhy?: boolean }) {
  return (
    <motion.div initial="hidden" animate="show" variants={{ show: { transition: { staggerChildren: 0.018 } } }} className="grid gap-x-(--gap) gap-y-7 px-(--pad)" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(var(--card-w), 1fr))' }}>
      {cards.map((c) => (
        <PosterCard key={c.id} card={c} origin={origin} onContext={onContext} width="100%" showWhy={showWhy} />
      ))}
    </motion.div>
  );
}

export function GridSkeleton({ count = 18 }: { count?: number }) {
  return (
    <div className="grid gap-x-(--gap) gap-y-7 px-(--pad)" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(var(--card-w), 1fr))' }}>
      {Array.from({ length: count }, (_, i) => (
        <CardSkeleton key={i} width="100%" />
      ))}
    </div>
  );
}
