/** Marquee Wrapped: the year in review across movies, TV, games and music, shareable as an image. */
import { useQuery } from '@tanstack/react-query';
import { motion, useReducedMotion } from 'framer-motion';
import { useRef, useState, type ReactNode } from 'react';
import { api, errorMessage, pickSave, shell } from '@/lib/api';
import { img } from '@/lib/img';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { Art } from '@/ui/Card';
import { Icon, type IconName } from '@/ui/icons';
import { Modal } from '@/ui/overlays';
import { Badge, Button, EmptyState, ErrorState, PageHeader, Select, Skeleton, cx } from '@/ui/primitives';

interface TasteLine {
  feature: string;
  label: string;
  weight: number;
  new: boolean;
}

export interface WrappedData {
  year: number;
  years: number[];
  complete: boolean;
  totals: { hours: number; titles: number; rated: number; avg_rating: number | null };
  movies: { count: number; hours: number; best: Card[] };
  tv: { shows: number; episodes: number; hours: number; best: Card[] };
  games: { count: number; hours: number; top: { card: Card; hours: number | null }[]; best: Card[]; hours_since: string | null };
  music: {
    best: Card[];
    source: string | null;
    scrobbles?: number;
    hours?: number;
    top_artists?: { name: string; plays: number }[];
    top_albums?: { title: string; artist: string | null; plays: number }[];
    artists_count?: number;
  };
  months: number[];
  busiest: { month: number; count: number } | null;
  creators: { name: string; role: string; count: number }[];
  taste: TasteLine[];
  adventurous: Card | null;
  first: Card | null;
}

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const n = (x: number) => Math.round(x).toLocaleString();

export default function Wrapped({ year: initial }: { year?: number }) {
  const [year, setYear] = useState<number | undefined>(initial);
  const { data, error, refetch } = useQuery({ queryKey: ['wrapped', year ?? 'latest'], queryFn: () => api<WrappedData>('wrapped', year ? { year } : {}), placeholderData: (p) => p, staleTime: 5 * 60_000 });
  const [sharing, setSharing] = useState(false);

  if (error) return <ErrorState error={error} onRetry={() => void refetch()} />;
  if (!data) {
    return (
      <div className="px-(--pad) pt-4" aria-busy="true">
        <Skeleton className="mb-6 h-12 w-80" />
        <Skeleton className="mb-6 h-64 w-full rounded-card!" />
        <Skeleton className="h-64 w-full rounded-card!" />
      </div>
    );
  }
  const empty = data.totals.titles === 0 && !data.music.scrobbles;
  return (
    <div className="pb-16">
      <PageHeader title={`Your ${data.year} in Marquee`} subtitle={data.complete ? 'Your year in movies, shows, games and music.' : `So far this year, up to ${new Date().toLocaleDateString(undefined, { month: 'long', day: 'numeric' })}.`}>
        {data.years.length > 1 && <Select label="Year" className="w-28" value={String(data.year)} onChange={(v) => setYear(Number(v))} options={data.years.map((y) => ({ value: String(y), label: String(y) }))} />}
        <Button variant="primary" icon="share" disabled={empty} onClick={() => setSharing(true)}>
          Share
        </Button>
      </PageHeader>
      {empty ? (
        <EmptyState icon="chart" title={`Nothing logged in ${data.year} yet`}>
          Rate, watch and play things, or connect Letterboxd, Trakt, Steam and Last.fm, and your year fills in here.
        </EmptyState>
      ) : (
        <div className="mx-(--pad) flex flex-col gap-5">
          <Hero data={data} />
          <div className="grid gap-5 lg:grid-cols-2">
            <Panel icon="film" title="Movies" stat={`${n(data.movies.count)} film${data.movies.count === 1 ? '' : 's'}`} sub={`${n(data.movies.hours)} hours`}>
              <Posters cards={data.movies.best} empty="Rate movies to see your favourites here." />
            </Panel>
            <Panel icon="tv" title="TV" stat={`${n(data.tv.episodes)} episode${data.tv.episodes === 1 ? '' : 's'}`} sub={`${n(data.tv.shows)} shows · ${n(data.tv.hours)} hours`}>
              <Posters cards={data.tv.best} empty="Rate shows to see your favourites here." />
            </Panel>
            <Panel icon="gamepad" title="Games" stat={data.games.hours > 0 ? `${n(data.games.hours)} hours` : `${n(data.games.count)} game${data.games.count === 1 ? '' : 's'}`} sub={data.games.hours > 0 ? `across ${n(data.games.count)} games` : 'played this year'}>
              <Posters cards={data.games.top.map((g) => g.card)} captions={data.games.top.map((g) => (g.hours != null ? `${n(g.hours)} h` : null))} empty="Connect Steam or rate games to see what you played." />
              {data.games.hours_since && data.games.hours_since > `${data.year}-01-02` && <p className="mt-3 text-[12px] text-faint">Hours count from {new Date(data.games.hours_since).toLocaleDateString(undefined, { month: 'long', day: 'numeric' })}, when Marquee first synced your Steam playtime this year.</p>}
            </Panel>
            <Panel icon="music" title="Music" stat={data.music.scrobbles ? `${n(data.music.scrobbles)} plays` : `${n(data.music.best.length)} albums rated`} sub={data.music.hours ? `${n(data.music.hours)} hours · ${n(data.music.artists_count ?? 0)} artists` : undefined}>
              {data.music.top_artists?.length ? (
                <ol className="flex flex-col gap-1.5">
                  {data.music.top_artists.map((a, i) => (
                    <li key={a.name} className="flex items-baseline gap-3">
                      <span className="tnum w-5 text-right text-faint">{i + 1}</span>
                      <span className="flex-1 truncate font-medium">{a.name}</span>
                      <span className="tnum text-[13px] text-subtext">{n(a.plays)} plays</span>
                    </li>
                  ))}
                </ol>
              ) : (
                <Posters cards={data.music.best} empty="Connect Last.fm to see your top artists of the year." />
              )}
            </Panel>
          </div>
          <div className="grid gap-5 lg:grid-cols-3">
            <Panel icon="chart" title="Your year, month by month" stat={data.busiest ? `${MONTHS[data.busiest.month - 1]}` : '—'} sub={data.busiest ? 'was your busiest month' : undefined} className="lg:col-span-2">
              <MonthBars months={data.months} busiest={data.busiest?.month} />
            </Panel>
            <Panel icon="dna" title="What you were into" stat={data.taste[0]?.label ?? '—'} sub={data.taste[0] ? 'topped your year' : undefined}>
              <ul className="flex flex-wrap gap-1.5">
                {data.taste.map((t) => (
                  <li key={t.feature}>
                    <Badge tone={t.new ? 'accent' : 'neutral'}>
                      {t.label}
                      {t.new ? ' · new' : ''}
                    </Badge>
                  </li>
                ))}
              </ul>
              {data.taste.some((t) => t.new) && <p className="mt-3 text-[12px] text-faint">"New" means it wasn't among your top picks the year before.</p>}
            </Panel>
          </div>
          <div className="grid gap-5 lg:grid-cols-3">
            {data.creators.length > 0 && (
              <Panel icon="star" title="Creators you kept coming back to" stat={data.creators[0].name} sub={`${data.creators[0].count} titles`}>
                <ul className="flex flex-col gap-1.5">
                  {data.creators.slice(1).map((c) => (
                    <li key={c.name} className="flex items-baseline gap-2">
                      <span className="flex-1 truncate">{c.name}</span>
                      <span className="text-[13px] text-subtext">
                        {c.role} · {c.count}
                      </span>
                    </li>
                  ))}
                </ul>
              </Panel>
            )}
            {data.adventurous && (
              <Panel icon="compass" title="Your boldest pick" stat={data.adventurous.title} sub={`a ${data.adventurous.match_pct ?? '?'}% match you rated ${data.adventurous.user_rating ?? 4}★`}>
                <Posters cards={[data.adventurous]} />
              </Panel>
            )}
            {data.first && (
              <Panel icon="flame" title="Where it started" stat={data.first.title} sub={`your first of ${data.year}`}>
                <Posters cards={[data.first]} />
              </Panel>
            )}
          </div>
          <p className="text-[12px] text-faint">Watch time uses each title's runtime (110 minutes for a film and 45 for an episode when unknown). Listening time assumes 3.5 minutes a play.</p>
        </div>
      )}
      {sharing && <ShareWrapped data={data} onClose={() => setSharing(false)} />}
    </div>
  );
}

function Hero({ data }: { data: WrappedData }) {
  const reduce = useReducedMotion();
  const stats: [string, string][] = [
    [n(data.totals.hours), 'hours'],
    [n(data.totals.titles), 'titles'],
    [n(data.totals.rated), 'rated'],
  ];
  if (data.totals.avg_rating != null) stats.push([`${data.totals.avg_rating}★`, 'average rating']);
  return (
    <section aria-label="Your year in numbers" className="relative overflow-hidden rounded-card bg-surface px-7 py-8">
      <div className="pointer-events-none absolute inset-0 opacity-25" style={{ background: 'radial-gradient(60% 120% at 10% 0%, var(--accent), transparent 70%)' }} />
      <div aria-hidden="true" className="absolute inset-x-6 top-3 flex justify-between">
        {Array.from({ length: 24 }, (_, i) => (
          <span key={i} className={cx('h-2 w-2 rounded-full bg-accent', i % 2 ? 'opacity-90' : 'opacity-30')} />
        ))}
      </div>
      <div className="relative grid grid-cols-2 gap-6 pt-3 md:grid-cols-4">
        {stats.map(([value, label], i) => (
          <motion.div key={label} initial={reduce ? false : { opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: i * 0.08 }}>
            <div className="display tnum text-[44px] font-extrabold leading-none">{value}</div>
            <div className="mt-1.5 text-subtext">{label}</div>
          </motion.div>
        ))}
      </div>
    </section>
  );
}

function Panel({ icon, title, stat, sub, children, className }: { icon: IconName; title: string; stat: string; sub?: string; children?: ReactNode; className?: string }) {
  return (
    <section className={cx('rounded-card bg-surface p-5', className)} aria-label={title}>
      <h2 className="flex items-center gap-2 text-[13px] font-semibold uppercase tracking-wider text-subtext">
        <Icon name={icon} size={15} />
        {title}
      </h2>
      <p className="display mt-2 truncate text-[26px] font-bold leading-tight">{stat}</p>
      {sub && <p className="text-subtext">{sub}</p>}
      {children && <div className="mt-4">{children}</div>}
    </section>
  );
}

function Posters({ cards, captions, empty }: { cards: Card[]; captions?: (string | null)[]; empty?: string }) {
  const openItem = useApp((s) => s.openItem);
  if (!cards.length) return empty ? <p className="text-[13px] text-faint">{empty}</p> : null;
  return (
    <ol className="flex gap-3">
      {cards.slice(0, 5).map((c, i) => (
        <li key={c.id} className="w-[18%] min-w-16 max-w-28">
          <button type="button" className="block w-full overflow-hidden rounded-control shadow-md focus-visible:outline-2" onClick={() => openItem(c.id, 'wrapped')} aria-label={`Open ${c.title}`}>
            <Art card={c} className={c.media === 'album' ? 'w-full' : 'aspect-2/3 w-full'} />
          </button>
          <p className="mt-1 truncate text-[12px] font-medium">{c.title}</p>
          {captions?.[i] && <p className="tnum text-[11px] text-subtext">{captions[i]}</p>}
        </li>
      ))}
    </ol>
  );
}

function MonthBars({ months, busiest }: { months: number[]; busiest?: number }) {
  const max = Math.max(1, ...months);
  return (
    <div className="flex h-36 items-end gap-1.5" role="img" aria-label={`Activity by month: ${months.map((m, i) => `${MONTHS[i]} ${m}`).join(', ')}`}>
      {months.map((m, i) => (
        <div key={i} className="flex h-full flex-1 flex-col items-center justify-end gap-1" title={`${MONTHS[i]}: ${m} logged`}>
          <div className={cx('w-full rounded-t-[4px]', i + 1 === busiest ? 'bg-accent' : 'bg-accent/35')} style={{ height: `${Math.max(m ? 4 : 0, (m / max) * 100)}%` }} />
          <span className="text-[10px] text-faint">{MONTHS[i][0]}</span>
        </div>
      ))}
    </div>
  );
}

/** Draws the year as a 1080×1350 portrait card. */
async function drawWrapped(canvas: HTMLCanvasElement, d: WrappedData, accent: string, withArt: boolean) {
  const W = 1080;
  const H = 1350;
  canvas.width = W;
  canvas.height = H;
  const c = canvas.getContext('2d')!;
  const bg = c.createLinearGradient(0, 0, 0, H);
  bg.addColorStop(0, '#0d0d10');
  bg.addColorStop(1, '#16161b');
  c.fillStyle = bg;
  c.fillRect(0, 0, W, H);
  const glow = c.createRadialGradient(160, 120, 0, 160, 120, 900);
  glow.addColorStop(0, `${accent}55`);
  glow.addColorStop(1, `${accent}00`);
  c.fillStyle = glow;
  c.fillRect(0, 0, W, H);
  c.fillStyle = accent;
  for (let x = 54; x < W - 40; x += 44) {
    c.globalAlpha = 0.35 + 0.65 * (((x / 44) | 0) % 2);
    c.beginPath();
    c.arc(x, 40, 6, 0, Math.PI * 2);
    c.fill();
  }
  c.globalAlpha = 1;
  const font = "'Inter Variable', 'Segoe UI Variable Display', 'Segoe UI', sans-serif";
  c.textBaseline = 'alphabetic';
  c.fillStyle = accent;
  c.font = `700 30px ${font}`;
  c.fillText('MARQUEE WRAPPED', 60, 120);
  c.fillStyle = '#f6f2ea';
  c.font = `800 120px ${font}`;
  c.fillText(String(d.year), 54, 238);

  const stats: [string, string][] = [
    [n(d.totals.hours), 'hours'],
    [n(d.movies.count), 'films'],
    [n(d.tv.episodes), 'episodes'],
    [d.games.hours > 0 ? n(d.games.hours) : n(d.games.count), d.games.hours > 0 ? 'hours played' : 'games'],
  ];
  if (d.music.scrobbles) stats.push([n(d.music.scrobbles), 'songs played']);
  stats.slice(0, 4).forEach(([v, label], i) => {
    const x = 60 + (i % 2) * 500;
    const y = 360 + Math.floor(i / 2) * 140;
    c.fillStyle = '#f6f2ea';
    c.font = `800 76px ${font}`;
    c.fillText(v, x, y);
    c.fillStyle = '#b9b4aa';
    c.font = `500 30px ${font}`;
    c.fillText(label, x, y + 42);
  });

  // Favourites strip: best movie, show, game, album.
  const picks = [d.movies.best[0], d.tv.best[0], d.games.top[0]?.card ?? d.games.best[0], d.music.best[0]].filter(Boolean) as Card[];
  const loaded = await Promise.all(
    picks.map(
      (p) =>
        new Promise<HTMLImageElement | null>((resolve) => {
          const src = withArt ? img(p.poster) : undefined;
          if (!src) return resolve(null);
          const im = new Image();
          im.crossOrigin = 'anonymous';
          im.onload = () => resolve(im);
          im.onerror = () => resolve(null);
          im.src = src;
        }),
    ),
  );
  c.fillStyle = '#b9b4aa';
  c.font = `600 28px ${font}`;
  c.fillText('Favourites', 60, 690);
  const pw = 220;
  const ph = 330;
  picks.forEach((p, i) => {
    const x = 60 + i * (pw + 33);
    const y = 716;
    const h = p.media === 'album' ? pw : ph;
    c.save();
    c.beginPath();
    c.roundRect(x, y, pw, h, 16);
    c.clip();
    const im = loaded[i];
    if (im) {
      const scale = Math.max(pw / im.width, h / im.height);
      c.drawImage(im, x + (pw - im.width * scale) / 2, y + (h - im.height * scale) / 2, im.width * scale, im.height * scale);
    } else {
      c.fillStyle = '#26262d';
      c.fillRect(x, y, pw, h);
    }
    c.restore();
    c.fillStyle = '#f6f2ea';
    c.font = `600 24px ${font}`;
    let t = p.title;
    while (c.measureText(t).width > pw && t.length > 3) t = `${t.slice(0, -2).trimEnd()}…`;
    c.fillText(t, x, y + ph + 36);
  });

  // Taste line.
  const top = d.taste.slice(0, 3).map((t) => t.label);
  c.fillStyle = '#b9b4aa';
  c.font = `500 30px ${font}`;
  if (top.length) c.fillText(`Into: ${top.join(' · ')}`, 60, 1180);
  if (d.music.top_artists?.[0]) c.fillText(`Top artist: ${d.music.top_artists[0].name}`, 60, 1228);
  c.fillStyle = accent;
  c.font = `600 24px ${font}`;
  c.fillText('Made with Marquee', 60, 1300);
}

function ShareWrapped({ data, onClose }: { data: WrappedData; onClose: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const toast = useApp((s) => s.toast);
  const [ready, setReady] = useState(false);
  const drawn = useRef(false);
  const draw = async (el: HTMLCanvasElement | null) => {
    if (!el || drawn.current) return;
    drawn.current = true;
    const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim();
    const hex = /^#[0-9a-f]{6}$/i.test(accent) ? accent : '#F2C14E';
    await drawWrapped(el, data, hex, true);
    try {
      el.toDataURL();
    } catch {
      await drawWrapped(el, data, hex, false);
    }
    setReady(true);
  };
  const blob = () => new Promise<Blob>((resolve, reject) => canvas.current?.toBlob((b) => (b ? resolve(b) : reject(new Error("The image couldn't be made."))), 'image/png'));
  return (
    <Modal open onClose={onClose} title={`Share your ${data.year}`} width={560}>
      <canvas
        ref={(el) => {
          canvas.current = el;
          void draw(el);
        }}
        className="mx-auto block h-auto max-h-[60vh] w-auto rounded-control"
        role="img"
        aria-label={`Your ${data.year} in Marquee: ${n(data.totals.hours)} hours across ${n(data.totals.titles)} titles`}
      />
      <div className="mt-4 flex justify-end gap-2">
        <Button
          icon="copy"
          disabled={!ready}
          onClick={async () => {
            try {
              await navigator.clipboard.write([new ClipboardItem({ 'image/png': await blob() })]);
              toast('Image copied', 'good');
            } catch (e) {
              toast(errorMessage(e), 'bad');
            }
          }}
        >
          Copy image
        </Button>
        <Button
          variant="primary"
          icon="download"
          disabled={!ready}
          onClick={async () => {
            try {
              const path = await pickSave(`marquee-${data.year}.png`, [{ name: 'PNG image', extensions: ['png'] }]);
              if (!path) return;
              await shell('save_file', { path, contents: canvas.current!.toDataURL('image/png').split(',')[1], base64: true });
              toast('Image saved', 'good');
            } catch (e) {
              toast(errorMessage(e), 'bad');
            }
          }}
        >
          Save image
        </Button>
      </div>
    </Modal>
  );
}
