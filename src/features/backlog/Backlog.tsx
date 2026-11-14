/** The game backlog: Play Later plus owned-but-unplayed games, with how long each takes to beat. */
import { useQuery } from '@tanstack/react-query';
import { motion, useReducedMotion } from 'framer-motion';
import { useState } from 'react';
import { api } from '@/lib/api';
import { releaseLabel } from '@/lib/format';
import { img } from '@/lib/img';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { useDone, useQuickAdd } from '@/ui/actions';
import { Art, MatchRing } from '@/ui/Card';
import { Icon } from '@/ui/icons';
import { Badge, Button, EmptyState, ErrorState, PageHeader, Segmented, Select, Skeleton, cx } from '@/ui/primitives';

type Filter = 'all' | 'play_later' | 'unplayed';
type Sort = 'next' | 'shortest' | 'longest' | 'match' | 'sale';

interface Ttb {
  quick?: number | null;
  main?: number | null;
  full?: number | null;
}

interface BacklogData {
  cards: Card[];
  next: { card: Card; reason: string } | null;
  counts: Record<Filter, number>;
  hours: number;
  with_length: number;
  lengths_available: boolean;
}

export function hoursLabel(h: number | null | undefined): string {
  if (h == null) return '—';
  if (h < 1) return '< 1 h';
  return `${Number.isInteger(h) ? h : h.toFixed(1)} h`;
}

const SORTS: { value: Sort; label: string }[] = [
  { value: 'next', label: 'Start next' },
  { value: 'shortest', label: 'Shortest first' },
  { value: 'longest', label: 'Longest first' },
  { value: 'match', label: 'Best match' },
  { value: 'sale', label: 'Biggest discount' },
];

export default function Backlog() {
  const [filter, setFilter] = useState<Filter>('all');
  const [sort, setSort] = useState<Sort>('next');
  const navigate = useApp((s) => s.navigate);
  const { data, error, refetch, isPending } = useQuery({ queryKey: ['backlog', filter, sort], queryFn: () => api<BacklogData>('backlog', { filter, sort }), placeholderData: (prev) => prev });

  return (
    <div className="pb-12">
      <PageHeader title="Backlog" subtitle="Games saved to Play Later or owned and barely played, with how long each takes to beat.">
        <Button icon="arrowLeft" onClick={() => navigate('/games')}>
          Games
        </Button>
      </PageHeader>
      {error ? (
        <ErrorState error={error} onRetry={() => void refetch()} />
      ) : isPending || !data ? (
        <div className="px-(--pad)">
          <Skeleton className="mb-6 h-44 w-full rounded-card!" />
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="mb-2 h-20 w-full" />
          ))}
        </div>
      ) : (
        <>
          {data.next && <StartNext card={data.next.card} reason={data.next.reason} />}
          <div className="mx-(--pad) mb-4 flex flex-wrap items-center gap-3">
            <Segmented
              label="Which games"
              value={filter}
              onChange={setFilter}
              size="sm"
              options={[
                { value: 'all', label: `All ${data.counts.all}` },
                { value: 'play_later', label: `Play Later ${data.counts.play_later}` },
                { value: 'unplayed', label: `Owned, unplayed ${data.counts.unplayed}` },
              ]}
            />
            <div className="ml-auto flex items-center gap-3">
              {data.with_length > 0 && (
                <span className="tnum text-[13px] text-subtext">
                  About <strong className="text-text">{data.hours.toLocaleString()} hours</strong> of main story across {data.with_length} game{data.with_length === 1 ? '' : 's'}
                </span>
              )}
              <Select label="Sort" className="w-48" value={sort} onChange={setSort} options={SORTS} />
            </div>
          </div>
          {!data.lengths_available && (
            <div role="status" className="mx-(--pad) mb-4 flex flex-wrap items-center gap-2.5 rounded-control bg-warn/12 px-3.5 py-2 text-[13px] text-warn">
              <Icon name="clock" size={16} />
              <span className="flex-1">How long each game takes to beat comes from IGDB. Add a Twitch client ID and secret to see lengths.</span>
              <Button size="sm" onClick={() => navigate('/settings/keys')}>
                Open API keys
              </Button>
            </div>
          )}
          {data.cards.length === 0 ? (
            <EmptyState icon="gamepad" title="Your backlog is clear">
              Save games to Play Later, or connect Steam: owned games with under an hour played show up here.
            </EmptyState>
          ) : (
            <ol className="mx-(--pad) flex flex-col gap-1.5" aria-label="Backlog">
              {data.cards.map((c, i) => (
                <BacklogRow key={c.id} card={c} index={i} />
              ))}
            </ol>
          )}
        </>
      )}
    </div>
  );
}

function StartNext({ card, reason }: { card: Card; reason: string }) {
  const openItem = useApp((s) => s.openItem);
  const reduce = useReducedMotion();
  const ttb = (card.meta?.ttb ?? {}) as Ttb;
  return (
    <motion.section aria-label="Start next" initial={reduce ? false : { opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="relative mx-(--pad) mb-7 overflow-hidden rounded-card bg-surface">
      {card.backdrop && <img src={img(card.backdrop)} alt="" className="absolute inset-0 h-full w-full object-cover opacity-25" />}
      <div className="absolute inset-0 bg-linear-to-r from-surface via-surface/85 to-transparent" />
      <div className="relative flex items-center gap-5 p-5">
        <button type="button" className="w-28 shrink-0 overflow-hidden rounded-control shadow-lg" onClick={() => openItem(card.id, 'backlog-next')} aria-label={`Open ${card.title}`}>
          <Art card={card} className="aspect-2/3 w-full" eager />
        </button>
        <div className="min-w-0 flex-1">
          <p className="text-[12px] font-semibold uppercase tracking-wider text-accent">Start next</p>
          <h2 className="display mt-1 truncate text-[26px] font-bold">{card.title}</h2>
          <p className="mt-1 text-subtext">{reason}</p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {ttb.main != null && (
              <span className="tnum mr-2 text-[13px] text-subtext">
                <strong className="text-text">{hoursLabel(ttb.main)}</strong> main story{ttb.full != null ? ` · ${hoursLabel(ttb.full)} to 100%` : ''}
              </span>
            )}
            <Button variant="primary" icon="chevronRight" onClick={() => openItem(card.id, 'backlog-next')}>
              Open
            </Button>
          </div>
        </div>
        {card.match_pct != null && <MatchRing pct={card.match_pct} size={64} stroke={5} />}
      </div>
    </motion.section>
  );
}

function BacklogRow({ card, index }: { card: Card; index: number }) {
  const openItem = useApp((s) => s.openItem);
  const done = useDone();
  const quickAdd = useQuickAdd();
  const ttb = (card.meta?.ttb ?? {}) as Ttb;
  const discount = typeof card.meta?.discount === 'number' ? card.meta.discount : 0;
  const price = card.meta?.price?.text as string | undefined;
  const out = !card.release_date || card.release_date <= new Date().toISOString().slice(0, 10);
  return (
    <li className="group flex items-center gap-4 rounded-control px-2 py-2 transition-colors hover:bg-hover">
      <span className="tnum w-6 shrink-0 text-right text-[13px] text-faint">{index + 1}</span>
      <button type="button" className="w-12 shrink-0 overflow-hidden rounded-md" onClick={() => openItem(card.id, 'backlog')} aria-label={`Open ${card.title}`} tabIndex={-1}>
        <Art card={card} className="aspect-2/3 w-full" />
      </button>
      <div className="min-w-0 flex-1">
        <button type="button" className="block max-w-full truncate text-left font-semibold hover:underline" onClick={() => openItem(card.id, 'backlog')}>
          {card.title}
        </button>
        <p className="truncate text-[13px] text-subtext">
          {typeof card.meta?.developer === 'string' ? `${card.meta.developer} · ` : ''}
          {out ? releaseLabel(card.release_date) : `Out ${releaseLabel(card.release_date)}`}
        </p>
      </div>
      <dl className="tnum hidden shrink-0 grid-cols-3 gap-5 text-center text-[13px] md:grid">
        {(
          [
            ['Rushed', ttb.quick],
            ['Main', ttb.main],
            ['100%', ttb.full],
          ] as const
        ).map(([label, h]) => (
          <div key={label} className="w-14">
            <dt className="text-[11px] text-faint">{label}</dt>
            <dd className={cx(label === 'Main' ? 'font-semibold' : 'text-subtext')}>{hoursLabel(h)}</dd>
          </div>
        ))}
      </dl>
      <div className="w-24 shrink-0 text-right text-[13px]">
        {discount > 0 ? (
          <>
            <Badge tone="good">-{discount}%</Badge>
            {price && <div className="mt-0.5 text-subtext">{price}</div>}
          </>
        ) : (
          price && <span className="text-subtext">{price}</span>
        )}
      </div>
      <div className="w-11 shrink-0">{card.match_pct != null && <MatchRing pct={card.match_pct} size={36} stroke={3} />}</div>
      <div className="flex shrink-0 gap-1 opacity-60 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
        <Button size="sm" icon="check" onClick={() => void done(card, true)} aria-label={`Mark ${card.title} as played`}>
          Beaten
        </Button>
        {!card.saved && (
          <Button size="sm" icon="plus" onClick={() => void quickAdd(card)} aria-label={`Save ${card.title} to Play Later`}>
            Save
          </Button>
        )}
      </div>
    </li>
  );
}
