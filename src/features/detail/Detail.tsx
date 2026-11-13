// The detail page for any title. It opens over the current screen: the poster morphs up from the
// card that was clicked, the backdrop drifts as you scroll, and the page takes its accent from the art.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { motion, useScroll, useTransform } from 'framer-motion';
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { api, errorMessage, openUrl } from '@/lib/api';
import { countdown, doneVerb, longDate, price, releaseLabel, runtimeLabel } from '@/lib/format';
import { accentFrom, img, onAccent } from '@/lib/img';
import type { Card, Media } from '@/lib/types';
import { EMPTY, useApp } from '@/state/app';
import { useAddTo, useDismiss, useDone, useFollow, useMoreLike, useRate } from '@/ui/actions';
import { Art, MatchRing, TypeBadge } from '@/ui/Card';
import { useCardMenu } from '@/ui/CardMenu';
import { Icon } from '@/ui/icons';
import { MenuButton, Modal, type MenuItem } from '@/ui/overlays';
import { Badge, Button, ErrorState, IconButton, Input, Skeleton, cx } from '@/ui/primitives';
import { Row } from '@/ui/Row';
import { Stars } from '@/ui/Stars';
import { Reason } from '../home/Home';
import { Heatmap } from './Heatmap';

interface DetailData {
  item: { id: number; media: Media; title: string; year: number | null; release_date: string | null; poster: string | null; backdrop: string | null; overview: string | null; runtime: number | null; rating: number | null; data: Record<string, any>; ids: Record<string, string>; detail_level: number };
  card: Card;
  match_pct: number | null;
  why: string[];
  where: Record<string, any>;
  similar: { same: Card[]; cross: Partial<Record<Media, Card[]>> };
  in_lists: { list_id: number; sync_state: string }[];
  following: { item: boolean; artist: boolean | null; franchise: boolean | null };
  events: { source: string; kind: string; value: number; at: string }[];
  moods: { feature: string; label: string; weight: number }[];
  targets: { id: string; label: string; hint?: string; ready: boolean }[];
  seriesgraph: string | null;
  letterboxd: string | null;
  offline: boolean;
  note: string | null;
}

function Section({ title, children, hint }: { title: string; children: ReactNode; hint?: ReactNode }) {
  return (
    <section className="mt-10" aria-label={title}>
      <div className="mb-3 flex items-baseline justify-between gap-4">
        <h2 className="text-[19px] font-semibold">{title}</h2>
        {hint && <div className="text-[12px] text-faint">{hint}</div>}
      </div>
      {children}
    </section>
  );
}

function TrailerPlayer({ trailer, title, poster }: { trailer: any; title: string; poster: string | null }) {
  const [playing, setPlaying] = useState(false);
  if (!trailer) return null;
  const thumb = trailer.site === 'youtube' ? `https://i.ytimg.com/vi/${trailer.key}/hqdefault.jpg` : (trailer.thumb ?? poster);
  return (
    <div className="relative aspect-video w-full max-w-3xl overflow-hidden rounded-card bg-black">
      {playing ? (
        trailer.site === 'youtube' ? (
          <iframe title={`${title} trailer`} className="h-full w-full" src={`https://www.youtube-nocookie.com/embed/${trailer.key}?autoplay=1&rel=0&modestbranding=1`} allow="autoplay; encrypted-media; fullscreen" allowFullScreen referrerPolicy="strict-origin-when-cross-origin" />
        ) : (
          <video className="h-full w-full" src={trailer.url} controls autoPlay />
        )
      ) : (
        <button type="button" className="group relative h-full w-full" onClick={() => setPlaying(true)} aria-label={`Play the trailer for ${title}`}>
          {thumb && <img src={img(thumb)} alt="" className="h-full w-full object-cover opacity-80 transition-opacity group-hover:opacity-100" />}
          <span className="absolute inset-0 grid place-items-center">
            <span className="grid h-16 w-16 place-items-center rounded-full bg-accent text-on-accent shadow-2xl transition-transform group-hover:scale-110">
              <Icon name="play" size={26} filled />
            </span>
          </span>
          <span className="absolute bottom-3 left-4 text-[13px] font-medium text-white drop-shadow">{trailer.name ?? 'Trailer'}</span>
        </button>
      )}
      {trailer.site === 'youtube' && (
        <button type="button" className="absolute right-3 top-3 rounded-md bg-black/60 px-2 py-1 text-[12px] text-white hover:bg-black/80" onClick={() => void openUrl(`https://www.youtube.com/watch?v=${trailer.key}`)}>
          Watch on YouTube
        </button>
      )}
    </div>
  );
}

function Providers({ where }: { where: Record<string, any> }) {
  const p = where.providers;
  const groups: [string, any[]][] = p
    ? (
        [
          ['Stream', p.stream ?? []],
          ['Free', p.free ?? []],
          ['Rent', p.rent ?? []],
          ['Buy', p.buy ?? []],
        ] as [string, any[]][]
      ).filter(([, list]) => list.length > 0)
    : [];
  if (groups.length === 0) return <p className="text-subtext">No streaming, rental or purchase options are listed for your region yet.</p>;
  return (
    <div className="space-y-3">
      {groups.map(([label, list]) => (
        <div key={label} className="flex items-center gap-3">
          <span className="w-14 shrink-0 text-[13px] font-medium text-faint">{label}</span>
          <ul className="flex flex-wrap gap-2">
            {list.slice(0, 10).map((prov: any) => (
              <li key={prov.id}>
                <button type="button" title={prov.name} aria-label={`${label} on ${prov.name}`} onClick={() => p.link && void openUrl(p.link)} className="hairline flex items-center gap-2 rounded-control bg-surface py-1 pl-1 pr-2.5 text-[13px] transition-colors hover:bg-hover">
                  {prov.logo ? (
                    <img src={img(prov.logo)} alt="" className="h-7 w-7 rounded-md" />
                  ) : (
                    <span className="grid h-7 w-7 place-items-center rounded-md bg-strong">
                      <Icon name="tv" size={14} />
                    </span>
                  )}
                  {prov.name}
                </button>
              </li>
            ))}
          </ul>
        </div>
      ))}
      <p className="text-[12px] text-faint">Availability data from JustWatch, via TMDB.</p>
    </div>
  );
}

function GameStore({ where, title }: { where: Record<string, any>; title: string }) {
  const p = where.price;
  const deck: Record<string, { label: string; tone: 'good' | 'warn' | 'bad' | 'neutral' }> = { verified: { label: 'Deck Verified', tone: 'good' }, playable: { label: 'Deck Playable', tone: 'warn' }, unsupported: { label: 'Deck Unsupported', tone: 'bad' }, unknown: { label: 'Deck: untested', tone: 'neutral' } };
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-4">
        {p ? (
          <div className="tnum flex items-baseline gap-2.5">
            {(where.discount ?? 0) > 0 && <span className="rounded-md bg-good px-2 py-0.5 text-[15px] font-bold text-black">-{where.discount}%</span>}
            <span className="text-3xl font-bold">{price(p.cents, p.currency)}</span>
            {(where.discount ?? 0) > 0 && <span className="text-subtext line-through">{price(p.initial, p.currency)}</span>}
          </div>
        ) : (
          <span className="text-subtext">No Steam price yet.</span>
        )}
        {where.steam_uri && (
          <Button variant="primary" icon="bag" onClick={() => void openUrl(where.steam_uri)}>
            Open in Steam
          </Button>
        )}
        {where.store_url && (
          <Button icon="external" onClick={() => void openUrl(where.store_url)}>
            Store page
          </Button>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {where.deck && <Badge tone={deck[where.deck]?.tone ?? 'neutral'}>{deck[where.deck]?.label ?? where.deck}</Badge>}
        {where.reviews && (
          <span className="tnum text-[13px] text-subtext">
            <span className="font-semibold text-text">{where.reviews.label}</span> · {where.reviews.percent}% of {Number(where.reviews.total).toLocaleString()} Steam reviews
          </span>
        )}
      </div>
      {where.low?.cents != null && (
        <p className="tnum text-[13px] text-subtext">
          Lowest price {where.low.source === 'Marquee' ? 'Marquee has seen' : 'on record'}: <span className="font-semibold text-text">{price(where.low.cents, where.low.currency ?? p?.currency)}</span>
          {where.low.at ? ` on ${longDate(where.low.at)}` : ''}
          {where.low.shop ? ` at ${where.low.shop}` : ''}
          {where.low.source !== 'Marquee' ? ' (IsThereAnyDeal)' : '. Add an IsThereAnyDeal key in Settings for the all-time low.'}
        </p>
      )}
      {Array.isArray(where.release_dates) && where.release_dates.length > 0 && (
        <ul className="tnum flex flex-wrap gap-2 text-[13px]" aria-label={`Release dates for ${title} by platform`}>
          {where.release_dates.map((r: any) => (
            <li key={r.platform} className="hairline rounded-control bg-surface px-2.5 py-1">
              <span className="font-semibold">{r.platform}</span> <span className="text-subtext">{r.date ? releaseLabel(r.date) : (r.human ?? 'TBA')}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function MusicLinks({ where, tracks }: { where: Record<string, any>; tracks: any[] | undefined }) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" icon="play" onClick={() => void openUrl(where.spotify)}>
          {where.spotify_id ? 'Open in Spotify' : 'Find on Spotify'}
        </Button>
        <Button icon="external" onClick={() => void openUrl(where.lastfm)}>
          Last.fm
        </Button>
        {where.musicbrainz && (
          <Button icon="external" onClick={() => void openUrl(where.musicbrainz)}>
            MusicBrainz
          </Button>
        )}
      </div>
      {/* Spotify's own player gives 30-second previews without the app needing preview URLs. */}
      {where.spotify_id && <iframe title="Spotify preview" className="h-[352px] w-full max-w-2xl rounded-card" src={`https://open.spotify.com/embed/album/${where.spotify_id}?utm_source=marquee&theme=0`} allow="autoplay; clipboard-write; encrypted-media; fullscreen" loading="lazy" />}
      {!where.spotify_id && tracks && tracks.length > 0 && (
        <ol className="tnum max-w-2xl divide-y divide-white/6 rounded-card bg-surface">
          {tracks.map((t, i) => (
            <li key={i} className="flex items-center gap-3 px-4 py-2 text-[13px]">
              <span className="w-5 text-right text-faint">{t.n ?? i + 1}</span>
              <span className="min-w-0 flex-1 truncate">{t.title}</span>
              {t.ms > 0 && (
                <span className="text-faint">
                  {Math.floor(t.ms / 60000)}:{String(Math.round((t.ms % 60000) / 1000)).padStart(2, '0')}
                </span>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

function People({ data }: { data: Record<string, any> }) {
  const cast: any[] = data.cast ?? [];
  const crew = (data.creators ?? []).filter((c: any) => c.role !== 'cast');
  const roleName: Record<string, string> = { director: 'Director', writer: 'Writer', creator: 'Creator', composer: 'Music', studio: 'Studio', network: 'Network', developer: 'Developer', publisher: 'Publisher', artist: 'Artist', label: 'Label' };
  return (
    <>
      {crew.length > 0 && (
        <ul className="mb-5 flex flex-wrap gap-x-8 gap-y-3">
          {crew.slice(0, 10).map((c: any, i: number) => (
            <li key={`${c.name}-${c.role}-${i}`}>
              <div className="text-[12px] text-faint">{roleName[c.role] ?? c.role}</div>
              <div className="font-medium">{c.name}</div>
            </li>
          ))}
        </ul>
      )}
      {cast.length > 0 && (
        <ul className="no-scrollbar flex gap-3 overflow-x-auto pb-1">
          {cast.map((p) => (
            <li key={p.id ?? p.name} className="w-24 shrink-0 text-center">
              {p.photo ? <img src={img(p.photo)} alt="" className="mx-auto h-24 w-24 rounded-full object-cover" loading="lazy" /> : <div className="mx-auto grid h-24 w-24 place-items-center rounded-full bg-surface text-2xl font-semibold text-faint">{String(p.name ?? '?')[0]}</div>}
              <div className="mt-2 truncate text-[13px] font-medium">{p.name}</div>
              <div className="truncate text-[12px] text-faint">{p.character}</div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

const SOURCE_NAME: Record<string, string> = { marquee: 'Marquee', steam: 'Steam', trakt: 'Trakt', letterboxd: 'Letterboxd', spotify: 'Spotify', lastfm: 'Last.fm', import: 'an import', serializd: 'Serializd' };

function describeEvent(e: DetailData['events'][number], media: Media): string | null {
  const from = SOURCE_NAME[e.source] ?? e.source;
  switch (e.kind) {
    case 'rating':
      return `Rated ${e.value}★ on ${from}`;
    case 'play':
      return media === 'game' ? `${Math.round(e.value / 60).toLocaleString()} h played on ${from}` : `${Math.round(e.value).toLocaleString()} plays on ${from}`;
    case 'watched':
      return `${doneVerb(media)} (${from})`;
    case 'like':
      return `Liked on ${from}`;
    case 'owned':
      return `In your ${from} library`;
    case 'wishlist':
      return `On your ${from} ${media === 'game' ? 'wishlist' : 'watchlist'}`;
    default:
      return null;
  }
}

export default function Detail({ id }: { id: number }) {
  const close = useApp((s) => s.closeItem);
  const origin = useApp((s) => s.itemOrigin);
  const lists = useApp((s) => s.boot?.lists ?? EMPTY);
  const sources = useApp((s) => s.boot?.sources ?? EMPTY);
  const navigate = useApp((s) => s.navigate);
  const mode = (document.documentElement.dataset.mode as 'dark' | 'light') ?? 'dark';
  const qc = useQueryClient();
  const scroller = useRef<HTMLDivElement>(null);
  const { scrollY } = useScroll({ container: scroller });
  const backdropY = useTransform(scrollY, [0, 600], [0, 150]);
  const backdropScale = useTransform(scrollY, [0, 600], [1.04, 1.16]);
  const backdropOpacity = useTransform(scrollY, [0, 520], [1, 0.25]);
  const [accent, setAccent] = useState<string | null>(null);
  const [newPlaylist, setNewPlaylist] = useState<string | null>(null);
  const toast = useApp((s) => s.toast);
  const menu = useCardMenu();
  const addTo = useAddTo();
  const rate = useRate();
  const done = useDone();
  const dismiss = useDismiss();
  const moreLike = useMoreLike();
  const follow = useFollow();

  // Show what the card already knew while the full details load.
  const seed = useMemo<Card | undefined>(() => {
    for (const [, data] of qc.getQueriesData<any>({})) {
      const pools: Card[][] = [];
      if (Array.isArray(data?.cards)) pools.push(data.cards);
      if (Array.isArray(data?.hero)) pools.push(data.hero);
      for (const r of data?.rows ?? []) if (Array.isArray(r?.cards)) pools.push(r.cards);
      for (const p of data?.pages ?? []) if (Array.isArray(p?.cards)) pools.push(p.cards);
      if (Array.isArray(data?.entries)) pools.push(data.entries);
      for (const pool of pools) {
        const hit = pool.find((c) => c?.id === id);
        if (hit) return hit;
      }
    }
    return undefined;
  }, [qc, id]);

  const { data, error, refetch } = useQuery({ queryKey: ['item', id], queryFn: () => api<DetailData>('item', { id }), staleTime: 60_000 });
  const episodes = useQuery({ queryKey: ['episodes', id], queryFn: () => api<{ seasons: any[][]; average: number | null }>('item.episodes', { id }), enabled: data?.item.media === 'tv' && !!data.item.ids.tmdb, staleTime: 3_600_000, retry: false });
  const aiOn = useApp((s) => s.boot?.settings['ai']?.enabled !== false);
  const blurb = useQuery({ queryKey: ['blurb', id], queryFn: () => api<{ blurb: string | null }>('item.blurb', { id }), enabled: !!data && aiOn && data.why.length > 0, staleTime: Infinity, retry: false });

  const card = data?.card ?? seed;
  const poster = data?.item.poster ?? seed?.poster ?? null;
  useEffect(() => {
    setAccent(null);
    const src = img(poster);
    if (!src) return;
    let live = true;
    void accentFrom(src, mode).then((c) => live && setAccent(c));
    return () => {
      live = false;
    };
  }, [poster, mode]);

  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
    scroller.current?.focus({ preventScroll: true });
  }, [id]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented && !document.querySelector('[role="menu"], [aria-modal="true"]:not([data-detail])')) close();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [close]);

  const item = data?.item;
  const d = item?.data ?? {};
  const media = (item?.media ?? card?.media ?? 'movie') as Media;
  const title = item?.title ?? card?.title ?? '';
  const backdrop = item?.backdrop ?? seed?.backdrop ?? (media === 'album' ? poster : null);
  const connected = (s: string) => sources.some((x) => x.id === s && x.status !== 'disconnected');
  const spotifyPlaylists: any[] = sources.find((s) => s.id === 'spotify')?.config?.playlists ?? [];

  const addItems: MenuItem[] =
    !data || !card
      ? []
      : [
          { id: 'h-ext', label: 'On your services', heading: true },
          ...data.targets.map<MenuItem>((t) => ({ id: t.id, label: t.label, hint: t.ready ? t.hint : 'Not connected: set it up in Settings', icon: t.hint ? 'external' : 'cloud', disabled: !t.ready, run: () => void addTo(card, t.id, t.label) })),
          ...(media === 'album' && connected('spotify')
            ? [...spotifyPlaylists.slice(0, 8).map<MenuItem>((p) => ({ id: `pl-${p.id}`, label: p.name, hint: 'Spotify playlist', icon: 'music', run: () => void addTo(card, `spotify:playlist:${p.id}`, p.name) })), { id: 'pl-new', label: 'New Spotify playlist…', icon: 'plus', run: () => setNewPlaylist('') } as MenuItem]
            : []),
          { id: 'sep', label: '', separator: true },
          { id: 'h-lists', label: 'Marquee lists', heading: true },
          ...lists.map<MenuItem>((l) => ({
            id: `l-${l.id}`,
            label: l.name,
            icon: 'bookmark',
            checked: data.in_lists.some((x) => x.list_id === l.id),
            run: () => void (data.in_lists.some((x) => x.list_id === l.id) ? api('remove', { id, list_id: l.id }).then(() => qc.invalidateQueries()) : addTo(card, `list:${l.id}`, l.name)),
          })),
          { id: 'sep2', label: '', separator: true },
          { id: 'remind', label: data.following.item ? 'Stop reminding me' : 'Remind me on release', icon: 'bell', checked: data.following.item, run: () => void follow('item', String(id), title, data.following.item, id) },
        ];

  const moreItems: MenuItem[] = !card
    ? []
    : [
        { id: 'more', label: 'More like this', icon: 'sparkles', run: () => void moreLike(card) },
        { id: 'sep', label: '', separator: true },
        { id: 'h', label: 'Not interested', heading: true },
        { id: 'genre', label: 'Not my genre', icon: 'ban', run: () => void dismiss(card, 'genre').then(close) },
        { id: 'creator', label: media === 'album' ? "I don't like this artist" : "I don't like who made it", icon: 'thumbDown', run: () => void dismiss(card, 'creator').then(close) },
        { id: 'other', label: 'Just hide it', icon: 'eyeOff', run: () => void dismiss(card, 'other').then(close) },
      ];

  const facts = [
    item?.year ?? card?.year,
    card && runtimeLabel({ media, runtime: item?.runtime ?? card.runtime }),
    d.certification,
    d.seasons_count ? `${d.seasons_count} season${d.seasons_count === 1 ? '' : 's'}` : null,
    d.status && media === 'tv' ? d.status : null,
    Array.isArray(d.platforms) && media === 'game' ? d.platforms.slice(0, 5).join(' · ') : null,
    d.label,
  ].filter(Boolean);
  const cross = data?.similar.cross ?? {};
  const crossTitle: Record<Media, string> = { movie: 'Films with the same feel', tv: 'Shows with the same feel', game: 'Games with the same feel', album: 'Albums with the same feel' };
  const history = (data?.events ?? []).map((e) => describeEvent(e, media)).filter(Boolean) as string[];
  const accentStyle = accent ? ({ '--accent': accent, '--on-accent': onAccent(accent) } as React.CSSProperties) : undefined;

  return (
    <motion.div data-detail role="dialog" aria-modal="true" aria-label={title} className="fixed inset-0 z-40 bg-bg" style={accentStyle} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, transition: { duration: 0.16 } }} transition={{ duration: 0.22 }}>
      <div ref={scroller} tabIndex={-1} className="h-full overflow-y-auto overflow-x-hidden outline-none">
        {/* Backdrop: drifts and fades as the page scrolls, tinted with the art's own color. */}
        <div className="pointer-events-none absolute inset-x-0 top-0 h-[72vh] overflow-hidden">
          {backdrop && <motion.img src={img(backdrop)} alt="" className={cx('h-full w-full object-cover', media === 'album' && 'scale-125 blur-3xl')} style={{ y: backdropY, scale: backdropScale, opacity: backdropOpacity }} />}
          <div className="absolute inset-0" style={{ background: 'linear-gradient(to bottom, color-mix(in srgb, var(--bg) 35%, transparent) 0%, color-mix(in srgb, var(--bg) 72%, transparent) 55%, var(--bg) 100%)' }} />
          <div className="absolute inset-0 opacity-35 mix-blend-soft-light" style={{ background: 'radial-gradient(80% 90% at 20% 30%, var(--accent), transparent 70%)' }} />
        </div>

        <div className="drag sticky top-0 z-20 flex h-13 items-center justify-between px-3">
          <IconButton icon="arrowLeft" label="Back (Esc)" className="no-drag bg-black/40! text-white! backdrop-blur-md hover:bg-black/60!" onClick={close} />
        </div>

        <div className="relative mx-auto max-w-6xl px-(--pad) pb-24 pt-[12vh]">
          <div className="flex flex-col gap-9 md:flex-row md:items-end">
            <motion.div layoutId={origin ? `art-${origin}-${id}` : undefined} className="shadow-card w-56 shrink-0 overflow-hidden rounded-card bg-surface md:w-64" transition={{ type: 'spring', stiffness: 260, damping: 30 }}>
              {card ? <Art card={{ title, poster, media }} className={cx('w-full', media !== 'album' && 'aspect-2/3')} eager /> : <Skeleton className="aspect-2/3 w-full" />}
            </motion.div>

            <motion.div className="min-w-0 flex-1" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.08, duration: 0.3 }}>
              {card && (
                <div className="mb-3 flex flex-wrap items-center gap-2.5">
                  <TypeBadge card={card} />
                  <span className="tnum text-[13px] font-medium text-subtext">
                    {releaseLabel(item?.release_date ?? card.release_date)}
                    {countdown(item?.release_date ?? card.release_date) && (item?.release_date ?? card.release_date ?? '') > new Date().toISOString().slice(0, 10) ? ` · ${countdown(item?.release_date ?? card.release_date)}` : ''}
                  </span>
                  {card.done && <Badge tone="good">{doneVerb(media)}</Badge>}
                  {data?.offline && <Badge tone="warn">Offline copy</Badge>}
                </div>
              )}
              <h1 className="display selectable text-[42px] font-extrabold leading-[1.05]">{title || <Skeleton className="h-11 w-2/3" />}</h1>
              {(d.artist || d.tagline) && <p className="mt-2 text-lg text-subtext">{d.artist ?? d.tagline}</p>}
              <p className="tnum mt-2 flex flex-wrap gap-x-2 text-[14px] text-subtext">
                {facts.map((f, i) => (
                  <span key={i}>
                    {i > 0 && <span className="mr-2 text-faint">·</span>}
                    {String(f)}
                  </span>
                ))}
              </p>
              {(item?.data?.genres ?? card?.genres ?? []).length > 0 && (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {(item?.data?.genres ?? card?.genres ?? []).slice(0, 6).map((g: string) => (
                    <span key={g} className="hairline rounded-full bg-surface/70 px-2.5 py-0.5 text-[12px] text-subtext">
                      {g}
                    </span>
                  ))}
                </div>
              )}

              <div className="mt-6 flex flex-wrap items-center gap-2.5">
                <MenuButton items={addItems} label="Add to">
                  {(open, isOpen) => (
                    <Button variant="primary" size="lg" icon={data?.in_lists.length ? 'check' : 'plus'} onClick={open} aria-haspopup="menu" aria-expanded={isOpen} disabled={!data}>
                      {data?.in_lists.length ? 'Saved' : 'Add to…'}
                      <Icon name="chevronDown" size={15} />
                    </Button>
                  )}
                </MenuButton>
                {card && (
                  <Button size="lg" icon={card.done ? 'check' : 'eye'} onClick={() => void done(card, !card.done)} aria-pressed={card.done}>
                    {card.done ? doneVerb(media) : `Mark ${doneVerb(media).toLowerCase()}`}
                  </Button>
                )}
                {data && media === 'album' && d.artist && (
                  <Button
                    size="lg"
                    icon="heart"
                    aria-pressed={!!data.following.artist}
                    onClick={() =>
                      void follow(
                        'artist',
                        String(d.artist)
                          .toLowerCase()
                          .replace(/[^a-z0-9]+/g, '-')
                          .replace(/^-|-$/g, ''),
                        d.artist,
                        !!data.following.artist,
                        id,
                        connected('spotify'),
                      )
                    }
                  >
                    {data.following.artist ? `Following ${d.artist}` : `Follow ${d.artist}`}
                  </Button>
                )}
                {data && d.collection?.id && (
                  <Button size="lg" icon="pin" aria-pressed={!!data.following.franchise} onClick={() => void follow('franchise', String(d.collection.id), d.collection.name, !!data.following.franchise, id)}>
                    {data.following.franchise ? 'Following the franchise' : 'Follow the franchise'}
                  </Button>
                )}
                <MenuButton items={moreItems} label="More actions">
                  {(open, isOpen) => <IconButton icon="more" label="More actions" className="h-11.5! w-11.5! hairline bg-strong" onClick={open} aria-haspopup="menu" aria-expanded={isOpen} />}
                </MenuButton>
              </div>

              {card && (
                <div className="mt-5 flex flex-wrap items-center gap-x-5 gap-y-2">
                  <Stars value={card.user_rating} onChange={(s) => void rate(card, s)} label={`Your rating for ${title}`} />
                  <span className="text-[13px] text-faint">{card.user_rating ? `You rated this ${card.user_rating}★` : 'Rate it to tune your picks'}</span>
                  {item?.rating != null && (
                    <span className="tnum text-[13px] text-subtext">
                      {media === 'game' ? 'Critics' : 'TMDB'} {item.rating.toFixed(1)}/10
                    </span>
                  )}
                </div>
              )}
            </motion.div>
          </div>

          {error && !data && <ErrorState error={error} onRetry={() => void refetch()} />}
          {data?.note && <p className="mt-6 rounded-control bg-warn/12 px-3.5 py-2 text-[13px] text-warn">Some details couldn't be refreshed: {data.note}</p>}

          {/* Why you'd like it */}
          {data && (data.match_pct != null || data.why.length > 0) && (
            <motion.section aria-label="Why you'd like it" className="mt-10 flex flex-wrap items-center gap-5 rounded-card p-5" style={{ background: 'color-mix(in srgb, var(--accent) 11%, var(--surface))' }} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.15 }}>
              {data.match_pct != null && <MatchRing pct={data.match_pct} size={72} stroke={6} />}
              <div className="min-w-0 flex-1">
                <h2 className="mb-1 text-[13px] font-semibold uppercase tracking-wider text-accent">Why you'd like it</h2>
                {data.why.length > 0 ? (
                  <ul className="space-y-0.5 text-[15px] text-subtext">
                    {data.why.map((w) => (
                      <li key={w}>
                        <Reason text={w} />
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="text-subtext">{data.match_pct != null && data.match_pct < 50 ? 'This one is outside what you usually go for.' : 'Rate a few things and Marquee will explain its picks.'}</p>
                )}
                {blurb.data?.blurb && (
                  <p className="mt-3 border-t border-white/10 pt-3 text-[14px] leading-relaxed text-text">
                    {blurb.data.blurb} <span className="text-[11px] text-faint">· written on this PC by your local model</span>
                  </p>
                )}
                {data.moods.length > 0 && (
                  <div className="mt-3 flex flex-wrap gap-1.5">
                    {data.moods.slice(0, 8).map((m) => (
                      <span key={m.feature} className="rounded-full bg-black/25 px-2.5 py-0.5 text-[12px] text-subtext">
                        {m.label}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            </motion.section>
          )}

          {!data && !error && (
            <div className="mt-10 space-y-4" aria-busy="true">
              <Skeleton className="h-24 w-full rounded-card!" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-5/6" />
              <Skeleton className="h-4 w-2/3" />
            </div>
          )}

          {data && (
            <>
              {(item?.overview || d.storyline) && (
                <Section title={media === 'album' ? 'About' : 'Synopsis'}>
                  <p className="selectable max-w-3xl text-[15px] leading-relaxed text-subtext">{item?.overview ?? d.storyline}</p>
                </Section>
              )}

              {history.length > 0 && (
                <Section title="Your history">
                  <ul className="flex flex-wrap gap-2">
                    {history.map((h) => (
                      <li key={h} className="hairline tnum rounded-full bg-surface px-3 py-1 text-[13px] text-subtext">
                        {h}
                      </li>
                    ))}
                  </ul>
                </Section>
              )}

              <Section title="Where to get it">
                {media === 'game' ? <GameStore where={data.where} title={title} /> : media === 'album' ? <MusicLinks where={data.where} tracks={d.tracks} /> : <Providers where={data.where} />}
                {media === 'movie' && data.letterboxd && (
                  <Button className="mt-4" icon="external" onClick={() => void openUrl(data.letterboxd!)}>
                    Open on Letterboxd
                  </Button>
                )}
              </Section>

              {d.trailer && (
                <Section title="Trailer">
                  <TrailerPlayer trailer={d.trailer} title={title} poster={backdrop} />
                </Section>
              )}

              {media === 'tv' && (
                <Section title="Seasons and episode ratings" hint={episodes.data?.average ? `Average episode ${episodes.data.average}/10 · ratings from TMDB` : undefined}>
                  {Array.isArray(d.seasons) && d.seasons.length > 0 && (
                    <ul className="no-scrollbar mb-5 flex gap-3 overflow-x-auto pb-1">
                      {d.seasons.map((s: any) => (
                        <li key={s.number} className="hairline flex w-56 shrink-0 gap-3 rounded-control bg-surface p-2">
                          {s.poster ? (
                            <img src={img(s.poster)} alt="" className="h-18 w-12 rounded object-cover" loading="lazy" />
                          ) : (
                            <span className="grid h-18 w-12 place-items-center rounded bg-strong text-faint">
                              <Icon name="tv" size={16} />
                            </span>
                          )}
                          <span className="min-w-0">
                            <span className="block truncate font-semibold">{s.name}</span>
                            <span className="tnum block text-[13px] text-subtext">{s.episodes} episodes</span>
                            <span className="tnum block text-[12px] text-faint">{s.air_date ? releaseLabel(s.air_date) : 'TBA'}</span>
                          </span>
                        </li>
                      ))}
                    </ul>
                  )}
                  {episodes.isPending && episodes.fetchStatus !== 'idle' ? (
                    <Skeleton className="h-40 w-full max-w-3xl rounded-card!" />
                  ) : episodes.data && episodes.data.seasons.some((s) => s.length > 0) ? (
                    <Heatmap seasons={episodes.data.seasons} />
                  ) : (
                    <p className="text-subtext">Episode ratings aren't available for this show{data.offline ? ' while offline' : ''}.</p>
                  )}
                  {data.seriesgraph && (
                    <Button className="mt-4" icon="chart" onClick={() => void openUrl(data.seriesgraph!)}>
                      View episode ratings graph on SeriesGraph
                    </Button>
                  )}
                </Section>
              )}

              {((d.cast ?? []).length > 0 || (d.creators ?? []).some((c: any) => c.role !== 'cast')) && (
                <Section title={media === 'movie' || media === 'tv' ? 'Cast and crew' : 'Credits'}>
                  <People data={d} />
                </Section>
              )}

              {Array.isArray(d.similar_artists) && d.similar_artists.length > 0 && (
                <Section title={`If you like ${d.artist}`}>
                  <ul className="flex flex-wrap gap-2">
                    {d.similar_artists.map((a: any) => (
                      <li key={a.name}>
                        <button type="button" className="hairline rounded-full bg-surface px-3 py-1 text-[13px] transition-colors hover:bg-hover" onClick={() => navigate(`/search/${encodeURIComponent(a.name)}`)}>
                          {a.name}
                        </button>
                      </li>
                    ))}
                  </ul>
                </Section>
              )}

              {Array.isArray(d.gallery) && d.gallery.length > 0 && (
                <Section title="Gallery">
                  <ul className="no-scrollbar flex gap-3 overflow-x-auto pb-1">
                    {d.gallery.map((g: string) => (
                      <li key={g} className="shrink-0">
                        <img src={img(g)} alt="" className="h-48 rounded-control object-cover" loading="lazy" />
                      </li>
                    ))}
                  </ul>
                </Section>
              )}
            </>
          )}
        </div>

        {data && (
          <div className="relative -mt-10 pb-16">
            {data.similar.same.length > 0 && <Row id={`sim-${id}`} title="More like this" cards={data.similar.same} onContext={menu.onContext} />}
            {(Object.keys(cross) as Media[]).map((m) => (
              <Row key={m} id={`cross-${id}-${m}`} title={crossTitle[m]} subtitle={`Shares its mood and themes with ${title}`} cards={cross[m] ?? []} onContext={menu.onContext} />
            ))}
          </div>
        )}
      </div>
      {menu.element}
      <Modal
        open={newPlaylist != null}
        onClose={() => setNewPlaylist(null)}
        title="New Spotify playlist"
        width={420}
        footer={
          <>
            <Button onClick={() => setNewPlaylist(null)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!newPlaylist?.trim() || !card}
              onClick={() => {
                const name = newPlaylist!.trim();
                setNewPlaylist(null);
                void api<{ id: string }>('spotify.create_playlist', { name })
                  .then((r) => addTo(card!, `spotify:playlist:${r.id}`, name))
                  .then(() => useApp.getState().refreshBoot())
                  .catch((e) => toast(errorMessage(e), 'bad'));
              }}
            >
              Create and add
            </Button>
          </>
        }
      >
        <label className="block">
          <span className="mb-1.5 block text-[13px] text-subtext">Playlist name. It is created as private; {title} is added to it.</span>
          <Input data-autofocus value={newPlaylist ?? ''} maxLength={100} placeholder="Late night drives" onChange={(e) => setNewPlaylist(e.target.value)} />
        </label>
      </Modal>
    </motion.div>
  );
}
