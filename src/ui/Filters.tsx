// The filter bar shared by For You and the media hubs.
import type { Filters, Media } from '@/lib/types';
import { MEDIA } from '@/lib/types';
import { mediaIcon } from './icons';
import { Chip, Segmented, Select } from './primitives';

const GENRES: Record<Media | 'all', [string, string][]> = {
  all: [
    ['sci-fi', 'Sci-fi'],
    ['fantasy', 'Fantasy'],
    ['drama', 'Drama'],
    ['comedy', 'Comedy'],
    ['horror', 'Horror'],
    ['thriller', 'Thriller'],
    ['action', 'Action'],
    ['adventure', 'Adventure'],
    ['mystery', 'Mystery'],
    ['romance', 'Romance'],
    ['animation', 'Animation'],
    ['documentary', 'Documentary'],
  ],
  movie: [
    ['sci-fi', 'Sci-fi'],
    ['drama', 'Drama'],
    ['comedy', 'Comedy'],
    ['horror', 'Horror'],
    ['thriller', 'Thriller'],
    ['action', 'Action'],
    ['fantasy', 'Fantasy'],
    ['romance', 'Romance'],
    ['animation', 'Animation'],
    ['documentary', 'Documentary'],
    ['crime', 'Crime'],
    ['mystery', 'Mystery'],
  ],
  tv: [
    ['drama', 'Drama'],
    ['comedy', 'Comedy'],
    ['sci-fi', 'Sci-fi'],
    ['fantasy', 'Fantasy'],
    ['crime', 'Crime'],
    ['mystery', 'Mystery'],
    ['animation', 'Animation'],
    ['documentary', 'Documentary'],
    ['reality', 'Reality'],
    ['action', 'Action'],
  ],
  game: [
    ['rpg', 'RPG'],
    ['adventure', 'Adventure'],
    ['action', 'Action'],
    ['indie', 'Indie'],
    ['shooter', 'Shooter'],
    ['strategy', 'Strategy'],
    ['simulation', 'Simulation'],
    ['puzzle', 'Puzzle'],
    ['platformer', 'Platformer'],
    ['racing', 'Racing'],
    ['sports', 'Sports'],
    ['horror', 'Horror'],
  ],
  album: [
    ['electronic', 'Electronic'],
    ['rock', 'Rock'],
    ['indie', 'Indie'],
    ['pop', 'Pop'],
    ['hip-hop', 'Hip-hop'],
    ['ambient', 'Ambient'],
    ['metal', 'Metal'],
    ['jazz', 'Jazz'],
    ['folk', 'Folk'],
    ['r&b', 'R&B'],
    ['classical', 'Classical'],
    ['soundtrack', 'Soundtrack'],
  ],
};

const SORTS: { value: string; label: string }[] = [
  { value: 'match', label: 'Best match' },
  { value: 'date', label: 'Release date (soonest)' },
  { value: 'date_desc', label: 'Release date (newest)' },
  { value: 'popularity', label: 'Popularity' },
  { value: 'rating', label: 'Rating' },
];

interface Props {
  value: Filters;
  onChange: (f: Filters) => void;
  /** Fix the bar to one medium (the hubs) or let the user pick (For You). */
  media?: Media;
  platforms?: string[];
  hasServices: boolean;
}

export function FilterBar({ value, onChange, media, platforms, hasServices }: Props) {
  const set = (patch: Partial<Filters>) => onChange({ ...value, ...patch });
  const toggle = <K extends 'media' | 'genres' | 'album_types'>(key: K, item: string) => {
    const current = (value[key] ?? []) as string[];
    set({ [key]: current.includes(item) ? current.filter((x) => x !== item) : [...current, item] } as Partial<Filters>);
  };
  const scope: Media | 'all' = media ?? ((value.media?.length === 1 ? value.media[0] : 'all') as Media | 'all');
  const showScreen = scope === 'all' || scope === 'movie' || scope === 'tv';
  const showGames = scope === 'all' || scope === 'game';
  return (
    <div className="mb-6 space-y-3 px-(--pad)" role="group" aria-label="Filters">
      <div className="flex flex-wrap items-center gap-2">
        {!media && (
          <>
            {MEDIA.map((m) => (
              <Chip key={m.id} icon={mediaIcon[m.id]} selected={value.media?.includes(m.id)} onClick={() => toggle('media', m.id)}>
                {m.plural}
              </Chip>
            ))}
            <span className="mx-1 h-5 w-px bg-white/12" />
          </>
        )}
        <Segmented
          size="sm"
          label="Release status"
          value={value.status ?? 'all'}
          onChange={(status) => set({ status })}
          options={[
            { value: 'all', label: 'All' },
            { value: 'released', label: 'Out now' },
            { value: 'upcoming', label: 'Upcoming' },
          ]}
        />
        {scope === 'movie' && (
          <Select
            label="Runtime"
            className="w-40"
            value={value.max_runtime ? 'short' : value.min_runtime ? 'long' : 'any'}
            onChange={(v) => set({ max_runtime: v === 'short' ? 100 : null, min_runtime: v === 'long' ? 140 : null })}
            options={[
              { value: 'any', label: 'Any runtime' },
              { value: 'short', label: 'Under 1h 40m' },
              { value: 'long', label: 'Over 2h 20m' },
            ]}
          />
        )}
        {scope === 'game' && (
          <Select
            label="Length"
            className="w-40"
            value={value.max_runtime ? 'short' : value.min_runtime ? 'long' : 'any'}
            onChange={(v) => set({ max_runtime: v === 'short' ? 600 : null, min_runtime: v === 'long' ? 2400 : null })}
            options={[
              { value: 'any', label: 'Any length' },
              { value: 'short', label: 'Short (under 10 h)' },
              { value: 'long', label: 'Long (40 h+)' },
            ]}
          />
        )}
        {showScreen && (
          <Chip icon="tv" selected={!!value.my_services} onClick={() => set({ my_services: !value.my_services })}>
            {hasServices ? 'On my services' : 'On my services (choose in Settings)'}
          </Chip>
        )}
        {showGames && (
          <>
            <Chip icon="tag" selected={!!value.on_sale} onClick={() => set({ on_sale: !value.on_sale })}>
              On sale
            </Chip>
            <Chip icon="flame" selected={!!value.near_low} onClick={() => set({ near_low: !value.near_low })}>
              Near all-time low
            </Chip>
            <Select
              label="Price"
              className="w-36"
              value={String(value.max_price ?? 'any')}
              onChange={(v) => set({ max_price: v === 'any' ? null : Number(v) })}
              options={[
                { value: 'any', label: 'Any price' },
                { value: '0', label: 'Free' },
                { value: '1000', label: 'Under $10' },
                { value: '2500', label: 'Under $25' },
                { value: '5000', label: 'Under $50' },
              ]}
            />
          </>
        )}
        {scope === 'game' && (
          <>
            <Chip icon="bookmark" selected={!!value.wishlisted} onClick={() => set({ wishlisted: !value.wishlisted })}>
              On my wishlist
            </Chip>
            {platforms && platforms.length > 0 && <Select label="Platform" className="w-36" value={value.platform ?? ''} onChange={(platform) => set({ platform: platform || null })} options={[{ value: '', label: 'All platforms' }, ...platforms.map((p) => ({ value: p, label: p }))]} />}
          </>
        )}
        {scope === 'album' && (
          <>
            {(['album', 'ep', 'single'] as const).map((t) => (
              <Chip key={t} selected={value.album_types?.includes(t)} onClick={() => toggle('album_types', t)}>
                {t === 'ep' ? 'EPs' : t === 'album' ? 'Albums' : 'Singles'}
              </Chip>
            ))}
            <Chip icon="heart" selected={!!value.my_artists} onClick={() => set({ my_artists: !value.my_artists })}>
              Artists I follow or play
            </Chip>
          </>
        )}
        {scope === 'tv' && (
          <Chip icon="eye" selected={!!value.tracking} onClick={() => set({ tracking: !value.tracking })}>
            Shows I'm watching
          </Chip>
        )}
        <div className="flex-1" />
        <Select label="Sort by" className="w-52" value={value.sort ?? 'match'} onChange={(sort) => set({ sort })} options={SORTS} />
      </div>
      <div className="flex flex-wrap gap-1.5" role="group" aria-label="Genres">
        {GENRES[scope].map(([id, label]) => (
          <Chip key={id} selected={value.genres?.includes(id)} onClick={() => toggle('genres', id)}>
            {label}
          </Chip>
        ))}
      </div>
    </div>
  );
}

export function activeFilterCount(f: Filters): number {
  return (f.media?.length ?? 0) + (f.genres?.length ?? 0) + (f.album_types?.length ?? 0) + [f.status && f.status !== 'all', f.max_runtime, f.min_runtime, f.my_services, f.on_sale, f.near_low, f.max_price != null, f.platform, f.wishlisted, f.tracking, f.my_artists].filter(Boolean).length;
}
