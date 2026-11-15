/** Your ratings: everything you've rated, in one place, editable, with how you rate. */
import { useInfiniteQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { api } from '@/lib/api';
import { typeBadge } from '@/lib/format';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { useRate } from '@/ui/actions';
import { Art } from '@/ui/Card';
import { Icon } from '@/ui/icons';
import { Button, EmptyState, ErrorState, PageHeader, Segmented, Select, Skeleton, cx } from '@/ui/primitives';
import { Stars } from '@/ui/Stars';

export type RatingSection = 'all' | 'movie' | 'tv' | 'anime' | 'game' | 'album';
type Sort = 'recent' | 'high' | 'low' | 'title';

interface RatingsPage {
  items: { card: Card; stars: number; at: string; source: string }[];
  next: number | null;
  histogram: number[];
  total: number;
  average: number;
  sections: Record<Exclude<RatingSection, 'all'>, number>;
}

const SOURCE: Record<string, string> = { marquee: 'Marquee', letterboxd: 'Letterboxd', trakt: 'Trakt', mal: 'MyAnimeList', serializd: 'Serializd', import: 'Import', lastfm: 'Last.fm' };

export default function Ratings() {
  const [section, setSection] = useState<RatingSection>('all');
  const [sort, setSort] = useState<Sort>('recent');
  const navigate = useApp((s) => s.navigate);
  const q = useInfiniteQuery({
    queryKey: ['ratings', section, sort],
    queryFn: ({ pageParam }) => api<RatingsPage>('ratings', { section, sort, offset: pageParam, limit: 60 }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.next ?? undefined,
    placeholderData: (prev) => prev,
  });
  const first = q.data?.pages[0];
  const items = q.data?.pages.flatMap((p) => p.items) ?? [];
  const n = (s: Exclude<RatingSection, 'all'>) => (first ? ` ${first.sections[s]}` : '');

  return (
    <div className="pb-14">
      <PageHeader title="Your ratings" subtitle="Everything you've rated, here and in the services you've connected. Change a rating and your picks update.">
        <Button icon="sparkles" onClick={() => navigate('/taste/build')}>
          Build your taste
        </Button>
        <Button variant="primary" icon="star" onClick={() => navigate('/taste/build/rate')}>
          Rate more
        </Button>
      </PageHeader>
      {q.error ? (
        <ErrorState error={q.error} onRetry={() => void q.refetch()} />
      ) : !first ? (
        <div className="px-(--pad)">
          <Skeleton className="mb-6 h-36 w-full rounded-card!" />
          <Skeleton className="h-96 w-full rounded-card!" />
        </div>
      ) : (
        <>
          <Summary page={first} />
          <div className="mx-(--pad) mb-4 flex flex-wrap items-center gap-3">
            <Segmented
              label="Kind"
              size="sm"
              value={section}
              onChange={setSection}
              options={[
                { value: 'all', label: 'Everything' },
                { value: 'movie', label: `Movies${n('movie')}` },
                { value: 'tv', label: `TV${n('tv')}` },
                { value: 'anime', label: `Anime${n('anime')}` },
                { value: 'game', label: `Games${n('game')}` },
                { value: 'album', label: `Music${n('album')}` },
              ]}
            />
            <Select
              label="Sort"
              className="ml-auto w-44"
              value={sort}
              onChange={setSort}
              options={[
                { value: 'recent', label: 'Most recent' },
                { value: 'high', label: 'Highest first' },
                { value: 'low', label: 'Lowest first' },
                { value: 'title', label: 'Title' },
              ]}
            />
          </div>
          {items.length === 0 ? (
            <EmptyState
              icon="star"
              title="No ratings here yet"
              action={
                <Button variant="primary" icon="star" onClick={() => navigate('/taste/build/rate')}>
                  Rate some titles
                </Button>
              }
            >
              Rate from any card (hover a poster), a title's page, Quick Rate, or "Rate more" here. Ratings from Letterboxd, Trakt and MyAnimeList show up after a sync.
            </EmptyState>
          ) : (
            <ul className="mx-(--pad) grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(330px, 1fr))' }} aria-label="Rated titles">
              {items.map((r) => (
                <RatedRow key={r.card.id} {...r} />
              ))}
            </ul>
          )}
          {q.hasNextPage && (
            <div className="mt-6 flex justify-center">
              <Button busy={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
                Show more
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function Summary({ page }: { page: RatingsPage }) {
  const max = Math.max(1, ...page.histogram);
  return (
    <section aria-label="How you rate" className="mx-(--pad) mb-6 flex flex-wrap items-end gap-8 rounded-card bg-surface p-5">
      <div>
        <div className="display tnum text-[42px] font-extrabold leading-none">{page.total.toLocaleString()}</div>
        <div className="mt-1 text-subtext">titles rated</div>
      </div>
      <div>
        <div className="display tnum text-[42px] font-extrabold leading-none">{page.total ? `${page.average.toFixed(1)}★` : '—'}</div>
        <div className="mt-1 text-subtext">average</div>
      </div>
      <div className="min-w-[280px] flex-1">
        <div className="flex h-24 items-end gap-1" role="img" aria-label={`Ratings by stars: ${page.histogram.map((n, i) => `${(i + 1) / 2} stars ${n}`).join(', ')}`}>
          {page.histogram.map((n, i) => (
            <div key={i} className="flex h-full flex-1 flex-col justify-end" title={`${(i + 1) / 2}★: ${n}`}>
              <div className={cx('w-full rounded-t-[4px]', i % 2 ? 'bg-accent' : 'bg-accent/55')} style={{ height: `${n ? Math.max(4, (n / max) * 100) : 0}%` }} />
            </div>
          ))}
        </div>
        <div className="mt-1 flex justify-between text-[11px] text-faint">
          <span>½★</span>
          <span>5★</span>
        </div>
      </div>
    </section>
  );
}

function RatedRow({ card, stars, source }: { card: Card; stars: number; source: string }) {
  const rate = useRate();
  const openItem = useApp((s) => s.openItem);
  const [value, setValue] = useState<number | null>(stars);
  const [gone, setGone] = useState(false);
  if (gone) return null;
  return (
    <li className="flex items-center gap-3 rounded-control bg-surface p-2 pr-3">
      <button type="button" className={cx('shrink-0 overflow-hidden rounded-md', card.media === 'album' ? 'w-14' : 'w-12')} onClick={() => openItem(card.id, 'ratings')} aria-label={`Open ${card.title}`} tabIndex={-1}>
        <Art card={card} className={card.media === 'album' ? 'aspect-square w-full' : 'aspect-2/3 w-full'} />
      </button>
      <div className="min-w-0 flex-1">
        <button type="button" className="block max-w-full truncate text-left font-semibold hover:underline" onClick={() => openItem(card.id, 'ratings')}>
          {card.title}
        </button>
        <p className="truncate text-[12px] text-subtext">
          {typeBadge(card)}
          {card.release_date ? ` · ${card.release_date.slice(0, 4)}` : ''}
          {source !== 'marquee' ? ` · from ${SOURCE[source] ?? source}` : ''}
        </p>
        <Stars
          size={18}
          value={value}
          label={`Your rating for ${card.title}`}
          onChange={(v) => {
            setValue(v || null);
            void rate(card, v, true);
          }}
        />
      </div>
      {source === 'marquee' && (
        <button
          type="button"
          className="grid h-8 w-8 shrink-0 place-items-center rounded-full text-faint hover:bg-hover hover:text-text"
          aria-label={`Remove your rating for ${card.title}`}
          title="Remove rating"
          onClick={() => {
            setGone(true);
            void rate(card, 0);
          }}
        >
          <Icon name="x" size={16} />
        </button>
      )}
    </li>
  );
}
