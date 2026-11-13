// "Your Taste": what Marquee has learned, shown plainly, and every lever to correct it.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { api, errorMessage } from '@/lib/api';
import { noun } from '@/lib/format';
import { MEDIA, type Card, type Media } from '@/lib/types';
import { useApp } from '@/state/app';
import { refreshFeeds } from '@/ui/actions';
import { useCardMenu } from '@/ui/CardMenu';
import { Icon, mediaIcon, type IconName } from '@/ui/icons';
import { MenuButton, Modal, type MenuItem } from '@/ui/overlays';
import { Button, EmptyState, ErrorState, PageHeader, Select, Skeleton, cx } from '@/ui/primitives';
import { Row } from '@/ui/Row';

interface Feature {
  feature: string;
  label: string;
  weight: number;
}
export interface TasteData {
  empty: boolean;
  media: Record<Media, { genres: Feature[]; themes: Feature[] }>;
  dna: (Feature & { support: number; media: Record<Media, number>; spans: number })[];
  creators: (Feature & { role: string })[];
  decades: { decade: string; media: Partial<Record<Media, number>>; total: number }[];
  adventurous: number | null;
  dial: number;
  source_weights: Record<string, number>;
  sources: { source: string; events: number }[];
  overrides: { feature: string; label: string; mode: 'pin' | 'block' }[];
  disliked: Feature[];
  stats: {
    monthly: { month: string; watched: number; listened: number }[];
    played_hours: number;
    played_recent_hours: number;
    top_year: Record<Media, Card[]>;
    year: number;
    streak_weeks: number;
    longest_streak_weeks: number;
    counts: { rated: number; watched: number; games: number; artists: number };
  };
  vocabulary: { feature: string; label: string }[];
}

/** One fixed colour per medium, the same in every chart on this page. */
const SERIES: Record<Media, string> = { movie: 'var(--series-1)', tv: 'var(--series-2)', game: 'var(--series-3)', album: 'var(--series-4)' };
const ORDER: Media[] = ['movie', 'tv', 'game', 'album'];
const NAME: Record<Media, string> = { movie: 'Movies', tv: 'TV', game: 'Games', album: 'Music' };
const SOURCE_NAME: Record<string, string> = { marquee: 'Ratings and feedback in Marquee', steam: 'Steam', trakt: 'Trakt', letterboxd: 'Letterboxd', spotify: 'Spotify', lastfm: 'Last.fm', import: 'Imported files', serializd: 'Serializd' };
const ROLE: Record<string, string> = { director: 'Director', writer: 'Writer', creator: 'Creator', composer: 'Composer', studio: 'Studio', network: 'Network', developer: 'Developer', publisher: 'Publisher', artist: 'Artist', label: 'Label', cast: 'Actor' };

function Panel({ title, hint, children, action, className }: { title: string; hint?: ReactNode; children: ReactNode; action?: ReactNode; className?: string }) {
  return (
    <section aria-label={title} className={cx('hairline rounded-card bg-surface p-5', className)}>
      <div className="mb-4 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[17px] font-semibold">{title}</h2>
          {hint && <p className="mt-0.5 text-[13px] text-subtext">{hint}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function Legend({ media }: { media: Media[] }) {
  return (
    <ul className="flex flex-wrap gap-x-4 gap-y-1 text-[12px] text-subtext" aria-label="Legend">
      {media.map((m) => (
        <li key={m} className="inline-flex items-center gap-1.5">
          <span className="h-2.5 w-2.5 rounded-[3px]" style={{ background: SERIES[m] }} />
          {NAME[m]}
        </li>
      ))}
    </ul>
  );
}

function Stat({ icon, value, label, sub }: { icon: IconName; value: ReactNode; label: string; sub?: string }) {
  return (
    <div className="hairline rounded-card bg-surface p-4">
      <div className="flex items-center gap-2 text-[13px] text-subtext">
        <Icon name={icon} size={15} className="text-accent" />
        {label}
      </div>
      <div className="tnum display mt-1.5 text-[28px] font-bold leading-none">{value}</div>
      {sub && <div className="tnum mt-1.5 text-[12px] text-faint">{sub}</div>}
    </div>
  );
}

/** Pin or block a tag, genre or creator. */
export function useOverride() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return async (feature: string, label: string, mode: 'pin' | 'block' | null) => {
    try {
      await api('taste.override', { feature, mode });
      toast(mode === 'pin' ? `Pinned ${label}. You'll see more of it.` : mode === 'block' ? `Blocked ${label}. It won't be recommended.` : `${label} is back to normal`, mode === 'block' ? 'info' : 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    void qc.invalidateQueries({ queryKey: ['taste'] });
    refreshFeeds(qc);
  };
}

function overrideItems(f: { feature: string; label: string }, current: 'pin' | 'block' | undefined, set: ReturnType<typeof useOverride>): MenuItem[] {
  return [
    { id: 'pin', label: current === 'pin' ? 'Unpin' : 'Pin: show me more', icon: 'pin', checked: current === 'pin', run: () => void set(f.feature, f.label, current === 'pin' ? null : 'pin') },
    { id: 'block', label: current === 'block' ? 'Unblock' : 'Block: never recommend', icon: 'ban', checked: current === 'block', danger: current !== 'block', run: () => void set(f.feature, f.label, current === 'block' ? null : 'block') },
  ];
}

/** The cross-media chart: each row is a mood or theme; the bar's length is how strongly you like it,
    and its segments show which media that liking comes from. */
function Dna({ data, modes, setOverride }: { data: TasteData['dna']; modes: Map<string, 'pin' | 'block'>; setOverride: ReturnType<typeof useOverride> }) {
  const [table, setTable] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const max = Math.max(...data.map((d) => d.weight), 0.0001);
  const used = ORDER.filter((m) => data.some((d) => (d.media[m] ?? 0) > 0.005));
  const pct = (v: number) => `${Math.round(v * 100)}%`;
  return (
    <Panel
      title="Taste DNA"
      hint="The moods and themes you come back to, and which media they come from. This is what lets a film you love suggest a game or an album."
      action={
        <Button size="sm" variant="ghost" icon={table ? 'chart' : 'list'} onClick={() => setTable(!table)} aria-pressed={table}>
          {table ? 'Chart' : 'Table'}
        </Button>
      }
    >
      <div className="mb-3">
        <Legend media={used} />
      </div>
      {table ? (
        <table className="tnum w-full text-left text-[13px]">
          <thead className="text-faint">
            <tr>
              <th scope="col" className="py-1.5 font-medium">
                Mood or theme
              </th>
              <th scope="col" className="py-1.5 text-right font-medium">
                Strength
              </th>
              {used.map((m) => (
                <th key={m} scope="col" className="py-1.5 text-right font-medium">
                  {NAME[m]}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.map((d) => (
              <tr key={d.feature} className="border-t border-white/6">
                <th scope="row" className="py-1.5 font-medium">
                  {d.label}
                </th>
                <td className="py-1.5 text-right text-subtext">{pct(d.weight / max)}</td>
                {used.map((m) => (
                  <td key={m} className="py-1.5 text-right text-subtext">
                    {pct(d.media[m] ?? 0)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <ul className="space-y-1.5">
          {data.map((d, i) => {
            const mode = modes.get(d.feature);
            return (
              <li key={d.feature} className="group grid grid-cols-[minmax(7rem,11rem)_1fr_auto] items-center gap-3" onMouseEnter={() => setActive(d.feature)} onMouseLeave={() => setActive(null)}>
                <MenuButton items={overrideItems(d, mode, setOverride)} label={`Options for ${d.label}`}>
                  {(open) => (
                    <button type="button" onClick={open} className="flex min-w-0 items-center gap-1.5 rounded-md py-1 text-left text-[13px] font-medium hover:text-accent" aria-label={`${d.label}${mode ? `, ${mode === 'pin' ? 'pinned' : 'blocked'}` : ''}. Pin or block.`}>
                      {mode && <Icon name={mode === 'pin' ? 'pin' : 'ban'} size={13} className={mode === 'pin' ? 'text-accent' : 'text-bad'} />}
                      <span className="truncate">{d.label}</span>
                    </button>
                  )}
                </MenuButton>
                <div
                  className="flex h-5 items-center"
                  role="img"
                  aria-label={`${d.label}: strength ${pct(d.weight / max)}. ${used
                    .filter((m) => (d.media[m] ?? 0) > 0.005)
                    .map((m) => `${NAME[m]} ${pct(d.media[m])}`)
                    .join(', ')}.`}
                >
                  <motion.div className="flex h-3.5 gap-0.5" initial={{ width: 0 }} animate={{ width: `${Math.max(4, (d.weight / max) * 100)}%` }} transition={{ duration: 0.6, delay: i * 0.03, ease: [0.2, 0.8, 0.2, 1] }}>
                    {used
                      .filter((m) => (d.media[m] ?? 0) > 0.005)
                      .map((m, n, arr) => (
                        <span key={m} title={`${NAME[m]}: ${pct(d.media[m])}`} className={cx('h-full min-w-1', n === 0 && 'rounded-l-sm', n === arr.length - 1 && 'rounded-r-sm')} style={{ width: `${d.media[m] * 100}%`, background: SERIES[m] }} />
                      ))}
                  </motion.div>
                </div>
                <span className="tnum w-36 truncate text-right text-[12px] text-faint">
                  {active === d.feature
                    ? used
                        .filter((m) => (d.media[m] ?? 0) > 0.005)
                        .map((m) => `${NAME[m]} ${pct(d.media[m])}`)
                        .join(' · ')
                    : d.spans > 1
                      ? `across ${d.spans} media`
                      : `${NAME[used.find((m) => (d.media[m] ?? 0) > 0.5) ?? used[0]]} only`}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}

function MediaColumn({ media, data, modes, setOverride }: { media: Media; data: TasteData['media'][Media]; modes: Map<string, 'pin' | 'block'>; setOverride: ReturnType<typeof useOverride> }) {
  const bars = (list: Feature[], offset: number) =>
    list.map((f, i) => {
      const mode = modes.get(f.feature);
      return (
        <li key={f.feature}>
          <MenuButton items={overrideItems(f, mode, setOverride)} label={`Options for ${f.label}`}>
            {(open) => (
              <button type="button" onClick={open} className="group block w-full rounded-md py-0.5 text-left" aria-label={`${f.label}, ${Math.round(f.weight * 100)}% of your top ${NAME[media].toLowerCase()} taste${mode ? `, ${mode === 'pin' ? 'pinned' : 'blocked'}` : ''}. Pin or block.`}>
                <span className="flex items-center justify-between gap-2 text-[13px]">
                  <span className="flex min-w-0 items-center gap-1.5">
                    {mode && <Icon name={mode === 'pin' ? 'pin' : 'ban'} size={12} className={mode === 'pin' ? 'text-accent' : 'text-bad'} />}
                    <span className="truncate group-hover:text-accent">{f.label}</span>
                  </span>
                  <span className="tnum text-[11px] text-faint">{Math.round(f.weight * 100)}</span>
                </span>
                <span className="mt-1 block h-1.5 overflow-hidden rounded-full bg-strong">
                  <motion.span className="block h-full rounded-full" style={{ background: SERIES[media] }} initial={{ width: 0 }} animate={{ width: `${Math.max(3, f.weight * 100)}%` }} transition={{ duration: 0.5, delay: (i + offset) * 0.03 }} />
                </span>
              </button>
            )}
          </MenuButton>
        </li>
      );
    });
  const empty = data.genres.length === 0 && data.themes.length === 0;
  return (
    <Panel title={NAME[media]} action={<Icon name={mediaIcon[media]} size={18} className="text-faint" />}>
      {empty ? (
        <p className="text-[13px] text-subtext">Nothing yet. Rate or connect something and this fills in.</p>
      ) : (
        <>
          <h3 className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-faint">Genres</h3>
          <ul className="space-y-1.5">{bars(data.genres, 0)}</ul>
          {data.themes.length > 0 && (
            <>
              <h3 className="mb-1.5 mt-4 text-[11px] font-semibold uppercase tracking-wider text-faint">Moods and themes</h3>
              <ul className="space-y-1.5">{bars(data.themes, data.genres.length)}</ul>
            </>
          )}
        </>
      )}
    </Panel>
  );
}

function Decades({ data }: { data: TasteData['decades'] }) {
  const [active, setActive] = useState<string | null>(null);
  const max = Math.max(...data.map((d) => d.total), 0.0001);
  const used = ORDER.filter((m) => data.some((d) => (d.media[m] ?? 0) > 0));
  const shown = data.find((d) => d.decade === active);
  return (
    <Panel title="By decade" hint="When the things you like were made.">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <Legend media={used} />
        <span className="tnum text-[12px] text-faint" aria-live="polite">
          {shown
            ? `${shown.decade}: ${used
                .filter((m) => (shown.media[m] ?? 0) > 0)
                .map((m) => `${NAME[m]} ${Math.round(((shown.media[m] ?? 0) / shown.total) * 100)}%`)
                .join(' · ')}`
            : 'Hover a decade for the split'}
        </span>
      </div>
      <ol className="flex h-40 items-end gap-1.5" aria-label="How much of what you like comes from each decade">
        {data.map((d, i) => (
          <li key={d.decade} className="flex h-full min-w-0 flex-1 flex-col justify-end" onMouseEnter={() => setActive(d.decade)} onMouseLeave={() => setActive(null)}>
            <button
              type="button"
              className="flex min-h-1 w-full flex-col-reverse gap-0.5 rounded-t-sm"
              style={{ height: `${Math.max(3, (d.total / max) * 100)}%` }}
              onFocus={() => setActive(d.decade)}
              onBlur={() => setActive(null)}
              aria-label={`${d.decade}: ${Math.round((d.total / max) * 100)}% of your strongest decade. ${used
                .filter((m) => (d.media[m] ?? 0) > 0)
                .map((m) => `${NAME[m]} ${Math.round(((d.media[m] ?? 0) / d.total) * 100)}%`)
                .join(', ')}.`}
            >
              {used
                .filter((m) => (d.media[m] ?? 0) > 0)
                .map((m, n, arr) => (
                  <motion.span key={m} className={cx('block w-full', n === arr.length - 1 && 'rounded-t-sm')} style={{ background: SERIES[m], flexGrow: d.media[m], flexBasis: 0, originY: 1 }} initial={{ scaleY: 0 }} animate={{ scaleY: 1 }} transition={{ duration: 0.45, delay: i * 0.03 }} />
                ))}
            </button>
          </li>
        ))}
      </ol>
      <ol className="tnum mt-1.5 flex gap-1.5 text-center text-[11px] text-faint" aria-hidden="true">
        {data.map((d) => (
          <li key={d.decade} className="min-w-0 flex-1 truncate">
            {d.decade.replace(/^(19|20)/, "'")}
          </li>
        ))}
      </ol>
    </Panel>
  );
}

function Monthly({ data }: { data: TasteData['stats']['monthly'] }) {
  const [active, setActive] = useState<string | null>(null);
  const max = Math.max(...data.map((d) => d.watched + d.listened), 1);
  const shown = data.find((d) => d.month === active) ?? data[data.length - 1];
  const monthName = (m: string) => new Date(`${m}-15T00:00:00`).toLocaleDateString(undefined, { month: 'short' });
  const hasWatched = data.some((d) => d.watched > 0);
  const hasListened = data.some((d) => d.listened > 0);
  return (
    <Panel title="Hours per month" hint="Watching from your history; listening estimated from Last.fm scrobbles at 3.5 minutes a track.">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <ul className="flex gap-4 text-[12px] text-subtext" aria-label="Legend">
          {hasWatched && (
            <li className="inline-flex items-center gap-1.5">
              <span className="h-2.5 w-2.5 rounded-[3px]" style={{ background: 'var(--series-1)' }} /> Watched
            </li>
          )}
          {hasListened && (
            <li className="inline-flex items-center gap-1.5">
              <span className="h-2.5 w-2.5 rounded-[3px]" style={{ background: 'var(--series-4)' }} /> Listened
            </li>
          )}
        </ul>
        {shown && (
          <span className="tnum text-[12px] text-faint" aria-live="polite">
            {new Date(`${shown.month}-15T00:00:00`).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}: {hasWatched ? `${shown.watched} h watched` : ''}
            {hasWatched && hasListened ? ' · ' : ''}
            {hasListened ? `${shown.listened} h listened` : ''}
          </span>
        )}
      </div>
      <ol className="flex h-36 items-end gap-1.5">
        {data.map((d, i) => (
          <li key={d.month} className="flex h-full min-w-0 flex-1 flex-col justify-end" onMouseEnter={() => setActive(d.month)} onMouseLeave={() => setActive(null)}>
            <button
              type="button"
              className="flex w-full flex-col-reverse gap-0.5"
              style={{ height: `${Math.max(2, ((d.watched + d.listened) / max) * 100)}%` }}
              onFocus={() => setActive(d.month)}
              onBlur={() => setActive(null)}
              aria-label={`${monthName(d.month)}: ${d.watched} hours watched, ${d.listened} hours listened`}
            >
              {d.watched > 0 && <motion.span className={cx('block w-full', d.listened <= 0 && 'rounded-t-sm')} style={{ background: 'var(--series-1)', flexGrow: d.watched, flexBasis: 0, originY: 1 }} initial={{ scaleY: 0 }} animate={{ scaleY: 1 }} transition={{ duration: 0.4, delay: i * 0.025 }} />}
              {d.listened > 0 && <motion.span className="block w-full rounded-t-sm" style={{ background: 'var(--series-4)', flexGrow: d.listened, flexBasis: 0, originY: 1 }} initial={{ scaleY: 0 }} animate={{ scaleY: 1 }} transition={{ duration: 0.4, delay: i * 0.025 }} />}
            </button>
          </li>
        ))}
      </ol>
      <ol className="mt-1.5 flex gap-1.5 text-center text-[11px] text-faint" aria-hidden="true">
        {data.map((d) => (
          <li key={d.month} className="min-w-0 flex-1 truncate">
            {monthName(d.month)}
          </li>
        ))}
      </ol>
    </Panel>
  );
}

export function Dial({ value }: { value: number }) {
  const [local, setLocal] = useState(value);
  const setSetting = useApp((s) => s.setSetting);
  const settings = useApp((s) => s.boot?.settings['taste']);
  const qc = useQueryClient();
  useEffect(() => setLocal(value), [value]);
  const commit = (v: number) => {
    void setSetting('taste', { ...(settings ?? {}), dial: v }).then(() => {
      // The engine rescores a moment after the setting lands.
      setTimeout(() => {
        refreshFeeds(qc);
        void qc.invalidateQueries({ queryKey: ['taste'] });
      }, 1400);
    });
  };
  const word = local < 0.2 ? 'Comfort: close to what you already love' : local < 0.45 ? 'Mostly familiar, with a few surprises' : local < 0.7 ? 'Balanced: half familiar, half discovery' : local < 0.9 ? 'Adventurous: wider and more varied' : 'Very adventurous: surprise me';
  return (
    <Panel title="Taste dial" hint="How far from your history the picks are allowed to wander.">
      <div className="flex items-center gap-4">
        <span className="text-[13px] font-medium text-subtext">Comfort</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={local}
          aria-label="Taste dial, from Comfort to Adventurous"
          aria-valuetext={word}
          className="h-2 flex-1"
          onChange={(e) => setLocal(Number(e.target.value))}
          onPointerUp={() => commit(local)}
          onKeyUp={() => commit(local)}
          onBlur={() => local !== value && commit(local)}
        />
        <span className="text-[13px] font-medium text-subtext">Adventurous</span>
      </div>
      <p className="mt-2.5 text-[13px] text-accent" aria-live="polite">
        {word}
      </p>
    </Panel>
  );
}

function Adventurous({ value }: { value: number | null }) {
  const words = value == null ? null : value < 30 ? 'You know what you like and stick close to it.' : value < 55 ? 'A clear home base, with regular trips outside it.' : value < 78 ? 'Your taste is spread across a lot of genres.' : 'You go almost everywhere. Genre barely predicts what you like.';
  return (
    <Panel title="How adventurous you are" hint="How evenly your liking is spread across genres.">
      {value == null ? (
        <p className="text-[13px] text-subtext">Not enough history to say yet.</p>
      ) : (
        <>
          <div className="flex items-baseline gap-2">
            <span className="tnum display text-[40px] font-bold leading-none">{value}</span>
            <span className="text-subtext">out of 100</span>
          </div>
          <div className="mt-3 h-2 overflow-hidden rounded-full bg-strong" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={value} aria-label="Adventurousness">
            <motion.div className="h-full rounded-full bg-accent" initial={{ width: 0 }} animate={{ width: `${value}%` }} transition={{ duration: 0.8, ease: [0.2, 0.8, 0.2, 1] }} />
          </div>
          <p className="mt-3 text-[13px] text-subtext">{words}</p>
        </>
      )}
    </Panel>
  );
}

export function Editing({ data, setOverride }: { data: TasteData; setOverride: ReturnType<typeof useOverride> }) {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  const setSetting = useApp((s) => s.setSetting);
  const [pick, setPick] = useState('');
  const [weights, setWeights] = useState(data.source_weights);
  const [confirm, setConfirm] = useState<{ source: string | null; label: string } | null>(null);
  useEffect(() => setWeights(data.source_weights), [data.source_weights]);
  const pins = data.overrides.filter((o) => o.mode === 'pin');
  const blocks = data.overrides.filter((o) => o.mode === 'block');
  const taken = new Set(data.overrides.map((o) => o.feature));
  const options = [{ value: '', label: 'Choose a mood or theme…' }, ...data.vocabulary.filter((v) => !taken.has(v.feature)).map((v) => ({ value: v.feature, label: v.label }))];
  const label = (f: string) => data.vocabulary.find((v) => v.feature === f)?.label ?? f;
  const commitWeights = (next: Record<string, number>) => {
    void setSetting('taste', { dial: data.dial, source_weights: next }).then(() =>
      setTimeout(() => {
        refreshFeeds(qc);
        void qc.invalidateQueries({ queryKey: ['taste'] });
      }, 1400),
    );
  };
  const reset = async (source: string | null) => {
    try {
      const { removed } = await api<{ removed: number }>('taste.reset', source ? { source } : {});
      toast(source ? `Forgot ${removed.toLocaleString()} ${noun(removed, 'signal')} from ${SOURCE_NAME[source] ?? source}` : 'Pins, blocks and "not interested" choices cleared', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    setConfirm(null);
    void qc.invalidateQueries({ queryKey: ['taste'] });
    refreshFeeds(qc);
  };
  const chip = (o: TasteData['overrides'][number]) => (
    <li key={o.feature}>
      <button type="button" className="hairline group inline-flex h-8 items-center gap-1.5 rounded-full bg-strong pl-3 pr-2 text-[13px] hover:bg-hover" onClick={() => void setOverride(o.feature, o.label, null)} aria-label={`${o.label}: remove ${o.mode === 'pin' ? 'pin' : 'block'}`}>
        {o.label}
        <Icon name="x" size={13} className="text-faint group-hover:text-text" />
      </button>
    </li>
  );
  return (
    <>
      <div className="grid gap-4 lg:grid-cols-2">
        <Panel title="Pins and blocks" hint="Pin what you love to see more of it. Block a genre, mood or creator and it's never recommended.">
          <h3 className="mb-2 flex items-center gap-1.5 text-[13px] font-semibold">
            <Icon name="pin" size={14} className="text-accent" /> Pinned
          </h3>
          {pins.length ? <ul className="flex flex-wrap gap-2">{pins.map(chip)}</ul> : <p className="text-[13px] text-faint">Nothing pinned. Click any genre, mood or creator on this page to pin it.</p>}
          <h3 className="mb-2 mt-5 flex items-center gap-1.5 text-[13px] font-semibold">
            <Icon name="ban" size={14} className="text-bad" /> Blocked
          </h3>
          {blocks.length ? <ul className="flex flex-wrap gap-2">{blocks.map(chip)}</ul> : <p className="text-[13px] text-faint">Nothing blocked.</p>}
          <div className="mt-5 flex flex-wrap items-center gap-2">
            <Select label="Mood or theme to pin or block" className="min-w-52 flex-1" value={pick} onChange={setPick} options={options} />
            <Button icon="pin" disabled={!pick} onClick={() => (void setOverride(pick, label(pick), 'pin'), setPick(''))}>
              Pin
            </Button>
            <Button icon="ban" disabled={!pick} onClick={() => (void setOverride(pick, label(pick), 'block'), setPick(''))}>
              Block
            </Button>
          </div>
          {data.disliked.length > 0 && (
            <>
              <h3 className="mb-2 mt-6 text-[13px] font-semibold">You tend to steer away from</h3>
              <ul className="flex flex-wrap gap-2">
                {data.disliked.map((f) => (
                  <li key={f.feature}>
                    <button type="button" className="hairline inline-flex h-8 items-center gap-1.5 rounded-full bg-surface px-3 text-[13px] text-subtext hover:bg-hover hover:text-text" title="Block it outright" onClick={() => void setOverride(f.feature, f.label, 'block')}>
                      {f.label} <Icon name="ban" size={13} />
                    </button>
                  </li>
                ))}
              </ul>
              <p className="mt-2 text-[12px] text-faint">Learned from low ratings and "not interested". Click one to block it completely.</p>
            </>
          )}
        </Panel>

        <Panel title="Where your taste comes from" hint="Turn a source up or down, or forget what it told Marquee.">
          {data.sources.length === 0 ? (
            <p className="text-[13px] text-subtext">No history yet.</p>
          ) : (
            <ul className="space-y-4">
              {data.sources.map((s) => {
                const w = weights[s.source] ?? 1;
                return (
                  <li key={s.source}>
                    <div className="flex items-baseline justify-between gap-3">
                      <span className="font-medium">{SOURCE_NAME[s.source] ?? s.source}</span>
                      <span className="tnum text-[12px] text-faint">
                        {s.events.toLocaleString()} {noun(s.events, 'signal')}
                      </span>
                    </div>
                    <div className="mt-1.5 flex items-center gap-3">
                      <input
                        type="range"
                        min={0}
                        max={2}
                        step={0.1}
                        value={w}
                        className="h-2 flex-1"
                        aria-label={`How much ${SOURCE_NAME[s.source] ?? s.source} counts`}
                        aria-valuetext={w === 0 ? 'Ignored' : `${w.toFixed(1)} times`}
                        onChange={(e) => setWeights({ ...weights, [s.source]: Number(e.target.value) })}
                        onPointerUp={() => commitWeights(weights)}
                        onKeyUp={() => commitWeights(weights)}
                      />
                      <span className="tnum w-14 text-right text-[13px] text-subtext">{w === 0 ? 'Ignored' : `×${w.toFixed(1)}`}</span>
                      <Button size="sm" variant="ghost" icon="trash" onClick={() => setConfirm({ source: s.source, label: SOURCE_NAME[s.source] ?? s.source })}>
                        Forget
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          <div className="mt-5 border-t border-white/8 pt-4">
            <Button icon="undo" onClick={() => setConfirm({ source: null, label: 'your pins, blocks and feedback' })}>
              Reset pins, blocks and feedback
            </Button>
          </div>
        </Panel>
      </div>
      <Modal
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title={confirm?.source ? `Forget what ${confirm.label} told Marquee?` : 'Reset your feedback?'}
        width={460}
        footer={
          <>
            <Button onClick={() => setConfirm(null)}>Cancel</Button>
            <Button variant="danger" onClick={() => void reset(confirm?.source ?? null)}>
              {confirm?.source ? 'Forget it' : 'Reset'}
            </Button>
          </>
        }
      >
        <p className="text-subtext">
          {confirm?.source
            ? confirm.source === 'marquee'
              ? 'Every rating, "watched" mark and piece of feedback you gave inside Marquee is removed. Connected services are not touched.'
              : `Its ratings, plays and history are removed from your taste profile. Nothing changes on ${confirm.label} itself, and the next sync brings it back unless you disconnect it.`
            : 'Clears every pin and block, and every "not interested" and "more like this". Your ratings and history stay.'}
        </p>
      </Modal>
    </>
  );
}

export default function Taste() {
  const navigate = useApp((s) => s.navigate);
  const { data, error, refetch } = useQuery({ queryKey: ['taste'], queryFn: () => api<TasteData>('taste') });
  const setOverride = useOverride();
  const menu = useCardMenu();
  const modes = useMemo(() => new Map((data?.overrides ?? []).map((o) => [o.feature, o.mode])), [data]);

  if (error) return <ErrorState error={error} onRetry={() => void refetch()} />;
  if (!data) {
    return (
      <div className="px-(--pad)" aria-busy="true">
        <Skeleton className="mb-6 h-9 w-56" />
        <div className="mb-4 grid grid-cols-4 gap-4">
          {Array.from({ length: 4 }, (_, i) => (
            <Skeleton key={i} className="h-24 rounded-card!" />
          ))}
        </div>
        <Skeleton className="h-96 w-full rounded-card!" />
      </div>
    );
  }
  if (data.empty) {
    return (
      <>
        <PageHeader title="Your Taste" />
        <EmptyState
          icon="dna"
          title="Marquee doesn't know your taste yet"
          action={
            <>
              <Button variant="primary" icon="plug" onClick={() => navigate('/settings/connections')}>
                Connect a service
              </Button>
              <Button icon="star" onClick={() => navigate('/for-you/rate')}>
                Rate a few things
              </Button>
            </>
          }
        >
          Connect Steam, Trakt, Letterboxd, Spotify or Last.fm, import a file, or rate a handful of titles. Your profile builds from there and never leaves this PC.
        </EmptyState>
      </>
    );
  }
  const s = data.stats;
  const top = ORDER.filter((m) => (s.top_year[m] ?? []).length > 0);
  return (
    <>
      <PageHeader title="Your Taste" subtitle="What Marquee has learned from your history. It lives on this PC and nowhere else." />
      <div className="space-y-4 px-(--pad)">
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-5">
          <Stat icon="star" label="Rated" value={s.counts.rated.toLocaleString()} sub="titles across all media" />
          <Stat icon="eye" label="Watched" value={s.counts.watched.toLocaleString()} sub="movies and shows" />
          <Stat icon="gamepad" label="Played" value={`${s.played_hours.toLocaleString()} h`} sub={`${s.counts.games.toLocaleString()} games · ${s.played_recent_hours} h in the last 2 weeks`} />
          <Stat icon="music" label="Artists" value={s.counts.artists.toLocaleString()} sub="in your listening history" />
          <Stat icon="flame" label="Streak" value={`${s.streak_weeks} ${noun(s.streak_weeks, 'wk')}`} sub={`Longest: ${s.longest_streak_weeks} ${noun(s.longest_streak_weeks, 'week')}`} />
        </div>

        <div className="grid gap-4 lg:grid-cols-[2fr_1fr]">
          <Dial value={data.dial} />
          <Adventurous value={data.adventurous} />
        </div>

        {data.dna.length > 0 && <Dna data={data.dna} modes={modes} setOverride={setOverride} />}

        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          {ORDER.map((m) => (
            <MediaColumn key={m} media={m} data={data.media[m] ?? { genres: [], themes: [] }} modes={modes} setOverride={setOverride} />
          ))}
        </div>

        {data.creators.length > 0 && (
          <Panel title="Favorite creators" hint="The people, studios and artists behind what you rate highest. Click one to pin or block.">
            <ul className="flex flex-wrap gap-2">
              {data.creators.map((c) => {
                const mode = modes.get(c.feature);
                return (
                  <li key={c.feature}>
                    <MenuButton items={overrideItems(c, mode, setOverride)} label={`Options for ${c.label}`}>
                      {(open) => (
                        <button type="button" onClick={open} className={cx('hairline inline-flex h-11 items-center gap-2.5 rounded-control bg-strong px-3 text-left hover:bg-hover', mode === 'block' && 'opacity-60')}>
                          {mode && <Icon name={mode === 'pin' ? 'pin' : 'ban'} size={14} className={mode === 'pin' ? 'text-accent' : 'text-bad'} />}
                          <span>
                            <span className="block text-[13px] font-semibold leading-tight">{c.label}</span>
                            <span className="block text-[11px] leading-tight text-faint">{ROLE[c.role] ?? c.role}</span>
                          </span>
                        </button>
                      )}
                    </MenuButton>
                  </li>
                );
              })}
            </ul>
          </Panel>
        )}

        <div className="grid gap-4 lg:grid-cols-2">
          {data.decades.length > 0 && <Decades data={data.decades} />}
          {s.monthly.length > 0 && <Monthly data={s.monthly} />}
        </div>
      </div>

      {top.length > 0 && (
        <div className="mt-9">
          <h2 className="display mb-4 px-(--pad) text-[22px] font-bold">Your top of {s.year}</h2>
          {top.map((m) => (
            <Row key={m} id={`top-${m}`} title={MEDIA.find((x) => x.id === m)?.plural ?? m} subtitle="Rated highest this year" cards={s.top_year[m]} onContext={menu.onContext} />
          ))}
        </div>
      )}

      <div className="mt-6 px-(--pad)">
        <h2 className="display mb-4 text-[22px] font-bold">Tune it</h2>
        <Editing data={data} setOverride={setOverride} />
      </div>
      {menu.element}
    </>
  );
}
