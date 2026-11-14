// The media hubs (Movies, TV, Anime, Games, Music): new and upcoming for one medium, filtered and sorted.
// Anime has its own screen (series and films), and TV leaves it out.
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { useState } from 'react';
import { api } from '@/lib/api';
import { countdown, releaseLabel } from '@/lib/format';
import { img } from '@/lib/img';
import type { Card, Filters, Media } from '@/lib/types';
import { setting, useApp } from '@/state/app';
import { CardGrid, GridSkeleton } from '@/ui/Card';
import { useCardMenu } from '@/ui/CardMenu';
import { FilterBar, activeFilterCount } from '@/ui/Filters';
import { Icon } from '@/ui/icons';
import { Button, EmptyState, ErrorState, OfflineBanner, PageHeader, SectionTitle, Segmented } from '@/ui/primitives';

interface Page {
  cards: Card[];
  next: number | null;
  genres: { id: string; count: number }[];
  platforms: string[];
}

type Section = Media | 'anime';

const COPY: Record<Section, { title: string; subtitle: string; key: string | null; keyHint: string }> = {
  movie: { title: 'Movies', subtitle: 'In theaters, on streaming, and on the way', key: 'tmdb', keyHint: 'Movies come from TMDB. Add a TMDB key to fill this page.' },
  tv: { title: 'TV', subtitle: 'Premieres, new seasons and the shows you follow', key: null, keyHint: 'Shows come from TVmaze (no key needed) or TMDB. Sync to fetch what’s premiering.' },
  anime: { title: 'Anime', subtitle: 'This season, next season and the anime you follow', key: 'mal_client_id', keyHint: 'Seasonal anime comes from MyAnimeList. Connect MyAnimeList in Connections, or add a TMDB key for anime from TMDB.' },
  game: { title: 'Games', subtitle: 'New and upcoming, with prices and Deck status', key: 'igdb_client_id', keyHint: 'The games catalog comes from IGDB. Add a Twitch client ID and secret to fill this page.' },
  album: { title: 'Music', subtitle: 'New albums, EPs and singles from artists you follow and artists like them', key: 'lastfm_key', keyHint: 'New music is found through the artists you listen to. Connect Last.fm or Spotify, and add a Last.fm key for similar artists.' },
};

/** The shows being tracked, soonest episode first, with a countdown. */
function Tracking({ anime }: { anime: boolean }) {
  const openItem = useApp((s) => s.openItem);
  const { data } = useQuery({ queryKey: ['tracked', anime], queryFn: () => api<{ card: Card; next_episode: any; days: number | null }[]>('tracked_shows', { anime }) });
  if (!data || data.length === 0) return null;
  const upcoming = data.filter((d) => d.days != null && d.days >= 0);
  const list = (upcoming.length ? upcoming : data).slice(0, 12);
  return (
    <section className="mb-9 px-(--pad)" aria-label={anime ? "Anime you're watching" : "Shows you're watching"}>
      <SectionTitle hint={`${data.length} tracked`}>{anime ? "Anime you're watching" : "Shows you're watching"}</SectionTitle>
      <motion.ul initial="hidden" animate="show" variants={{ show: { transition: { staggerChildren: 0.03 } } }} className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))' }}>
        {list.map(({ card, next_episode: ep, days }) => (
          <motion.li key={card.id} variants={{ hidden: { opacity: 0, y: 10 }, show: { opacity: 1, y: 0 } }}>
            <button type="button" onClick={() => openItem(card.id)} className="hairline flex w-full items-center gap-3.5 rounded-card bg-surface p-2.5 text-left transition-colors hover:bg-hover">
              {card.poster ? (
                <img src={img(card.poster)} alt="" className="h-20 w-13.5 shrink-0 rounded-md object-cover" loading="lazy" />
              ) : (
                <div className="grid h-20 w-13.5 shrink-0 place-items-center rounded-md bg-strong text-faint">
                  <Icon name="tv" />
                </div>
              )}
              <div className="min-w-0 flex-1">
                <div className="truncate font-semibold">{card.title}</div>
                {ep?.air_date ? (
                  <>
                    <div className="tnum mt-0.5 truncate text-[13px] text-subtext">
                      S{ep.season} E{ep.episode}
                      {ep.name && !/^Episode \d+$/.test(ep.name) ? ` · ${ep.name}` : ''}
                    </div>
                    <div className="tnum mt-1.5 flex items-center gap-2 text-[12px]">
                      <span className={days != null && days <= 1 ? 'font-semibold text-accent' : 'text-faint'}>{countdown(ep.air_date) ?? releaseLabel(ep.air_date)}</span>
                      {ep.episode === 1 && <span className="rounded bg-accent/15 px-1.5 py-0.5 font-semibold text-accent">Season premiere</span>}
                      {ep.finale && <span className="rounded bg-warn/15 px-1.5 py-0.5 font-semibold text-warn">Finale</span>}
                    </div>
                  </>
                ) : (
                  <div className="mt-0.5 text-[13px] text-faint">{card.meta?.status === 'Ended' || card.meta?.status === 'Canceled' ? card.meta.status : 'No new episode announced'}</div>
                )}
              </div>
            </button>
          </motion.li>
        ))}
      </motion.ul>
    </section>
  );
}

/** Each screen's filters, kept while the app is open so leaving and coming back doesn't reset them. */
const remembered = new Map<Section, Filters>();

export default function Hub({ media }: { media: Section }) {
  const [filters, setFiltersState] = useState<Filters>(() => remembered.get(media) ?? {});
  const setFilters = (f: Filters) => {
    remembered.set(media, f);
    setFiltersState(f);
  };
  const copy = COPY[media];
  const offline = useApp((s) => s.offline);
  const navigate = useApp((s) => s.navigate);
  const keySet = useApp((s) => copy.key == null || (s.boot?.keys.find((k) => k.name === copy.key)?.set ?? false));
  const hasServices = useApp((s) => setting<number[]>(s.boot?.settings, 'my_providers', []).length > 0);
  const menu = useCardMenu();
  const q = useInfiniteQuery({
    queryKey: ['hub', media, filters],
    queryFn: ({ pageParam }) => api<Page>('hub', { media, filters, offset: pageParam, limit: 60 }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.next,
  });
  const cards = q.data?.pages.flatMap((p) => p.cards) ?? [];
  const platforms = q.data?.pages[0]?.platforms ?? [];
  return (
    <>
      <PageHeader title={copy.title} subtitle={copy.subtitle}>
        {media === 'game' && (
          <Button icon="clock" onClick={() => navigate('/games/backlog')}>
            Backlog
          </Button>
        )}
        {media !== 'album' && (
          <Button icon="play" onClick={() => navigate('/reel')}>
            Trailers
          </Button>
        )}
        <Button icon="dice" onClick={() => useApp.getState().set({ surprise: media === 'anime' ? 'tv' : media })}>
          Surprise me
        </Button>
      </PageHeader>
      {offline && <OfflineBanner />}
      {!keySet && cards.length > 0 && (
        <div role="status" className="mx-(--pad) mb-4 flex flex-wrap items-center gap-2.5 rounded-control bg-warn/12 px-3.5 py-2 text-[13px] text-warn">
          <Icon name="plug" size={16} />
          <span className="flex-1">Posters, release dates and details need one more key. {copy.keyHint}</span>
          <Button size="sm" onClick={() => navigate('/settings/keys')}>
            Open API keys
          </Button>
        </div>
      )}
      {(media === 'tv' || media === 'anime') && <Tracking anime={media === 'anime'} />}
      {media === 'anime' && (
        <div className="mx-(--pad) mb-3">
          <Segmented
            label="Series or films"
            size="sm"
            value={filters.media?.length === 1 ? filters.media[0] : 'both'}
            onChange={(v) => setFilters({ ...filters, media: v === 'both' ? undefined : [v as Media] })}
            options={[
              { value: 'both', label: 'Everything' },
              { value: 'tv', label: 'Series', icon: 'tv' },
              { value: 'movie', label: 'Films', icon: 'film' },
            ]}
          />
        </div>
      )}
      <FilterBar value={filters} onChange={setFilters} media={media === 'anime' ? 'tv' : media} platforms={platforms} hasServices={hasServices} />
      {q.isPending ? (
        <GridSkeleton />
      ) : q.error ? (
        <ErrorState error={q.error} onRetry={() => void q.refetch()} />
      ) : cards.length === 0 ? (
        <EmptyState
          icon={activeFilterCount(filters) ? 'filter' : keySet ? 'sync' : 'plug'}
          title={activeFilterCount(filters) ? 'Nothing matches those filters' : keySet ? `No ${copy.title.toLowerCase()} yet` : 'One thing to set up'}
          action={
            activeFilterCount(filters) ? (
              <Button onClick={() => setFilters({})}>Clear filters</Button>
            ) : keySet ? (
              <Button variant="primary" icon="sync" onClick={() => void api('sync')}>
                Sync now
              </Button>
            ) : (
              <Button variant="primary" icon="plug" onClick={() => navigate('/settings/connections')}>
                Open Connections
              </Button>
            )
          }
        >
          {activeFilterCount(filters) ? 'Loosen a filter or two.' : keySet ? 'Sync to fetch what’s new and upcoming.' : copy.keyHint}
        </EmptyState>
      ) : (
        <>
          <CardGrid cards={cards} origin={`hub-${media}`} onContext={menu.onContext} />
          {q.hasNextPage && (
            <div className="mt-8 flex justify-center">
              <Button busy={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
                {q.isFetchingNextPage ? 'Loading…' : 'Show more'}
              </Button>
            </div>
          )}
        </>
      )}
      {menu.element}
    </>
  );
}
