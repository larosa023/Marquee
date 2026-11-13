// First run. Every step can be skipped; nothing here is a dead end, and the last step shows
// the real sync as it builds the taste profile.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { api, errorMessage, pickFile } from '@/lib/api';
import { img } from '@/lib/img';
import { appearanceFrom, themeVars } from '@/lib/theme';
import { MEDIA, type Card, type Media } from '@/lib/types';
import { EMPTY, useApp, useSetting } from '@/state/app';
import { Art } from '@/ui/Card';
import { Icon, mediaIcon } from '@/ui/icons';
import { Logo } from '@/ui/Logo';
import { Button, Chip, Segmented, Select, cx } from '@/ui/primitives';
import { WindowControls } from '../shell/Chrome';
import { ConnectionsList } from '../settings/Connections';

const STEPS = ['welcome', 'connect', 'import', 'love', 'region', 'theme', 'build'] as const;
type Step = (typeof STEPS)[number];
const TITLES: Record<Step, [string, string]> = {
  welcome: ['', ''],
  connect: ['Connect what you already use', 'Marquee reads your history to learn your taste. Any order, any number, or none at all. You can add more later in Settings.'],
  import: ['Bring in the rest', 'Have ratings somewhere without a sign-in? Import a file: IMDb, Backloggd, RateYourMusic, Serializd, or any spreadsheet.'],
  love: ['Pick 10 things you love', "The fastest way to a good first feed. Pick across movies, shows, games and music; it doesn't have to be exactly ten."],
  region: ['Where you are, and what you subscribe to', 'So release dates are right and Marquee knows which services you can already watch on.'],
  theme: ['Make it yours', 'Pick a look. Everything here can be changed later in Settings → Appearance.'],
  build: ['', ''],
};
const REGIONS: [string, string][] = [
  ['US', 'United States'],
  ['CA', 'Canada'],
  ['GB', 'United Kingdom'],
  ['IE', 'Ireland'],
  ['AU', 'Australia'],
  ['NZ', 'New Zealand'],
  ['DE', 'Germany'],
  ['FR', 'France'],
  ['ES', 'Spain'],
  ['IT', 'Italy'],
  ['NL', 'Netherlands'],
  ['SE', 'Sweden'],
  ['NO', 'Norway'],
  ['DK', 'Denmark'],
  ['PL', 'Poland'],
  ['MX', 'Mexico'],
  ['BR', 'Brazil'],
  ['JP', 'Japan'],
  ['KR', 'South Korea'],
  ['IN', 'India'],
];

function Welcome({ next }: { next: () => void }) {
  const toast = useApp((s) => s.toast);
  const restore = async () => {
    const path = await pickFile([{ name: 'Marquee backup', extensions: ['marqueebackup'] }]);
    if (!path) return;
    try {
      await api('backup.restore', { path });
      await api('settings.set', { key: 'onboarded', value: true });
      await useApp.getState().refreshBoot();
      toast('Restored from your backup. Reconnect your services in Settings to resume syncing.', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  return (
    <div className="flex h-full flex-col items-center justify-center text-center">
      <motion.div initial={{ scale: 0.7, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} transition={{ type: 'spring', stiffness: 160, damping: 16 }}>
        <Logo size={112} active />
      </motion.div>
      <motion.h1 className="display mt-8 text-[46px] font-extrabold leading-none" initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.15 }}>
        Welcome to Marquee
      </motion.h1>
      <motion.p className="mt-4 max-w-xl text-[17px] text-subtext" initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.25 }}>
        Every new movie, show, game and album worth knowing about, picked for your taste, with one click to save it where it belongs.
      </motion.p>
      <motion.ul className="mt-8 flex flex-wrap justify-center gap-x-8 gap-y-3 text-[13px] text-subtext" initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.4 }}>
        {[
          ['database', 'Everything stays on this PC'],
          ['eyeOff', 'No account, no telemetry'],
          ['sparkles', 'Every pick comes with a reason'],
        ].map(([icon, text]) => (
          <li key={text} className="flex items-center gap-2">
            <Icon name={icon as 'database'} size={16} className="text-accent" /> {text}
          </li>
        ))}
      </motion.ul>
      <motion.div className="mt-10 flex flex-col items-center gap-3" initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.5 }}>
        <Button variant="primary" size="lg" data-autofocus autoFocus onClick={next}>
          Get started
          <Icon name="chevronRight" size={18} />
        </Button>
        <button type="button" className="rounded-md text-[13px] text-faint hover:text-text" onClick={() => void restore()}>
          I have a Marquee backup
        </button>
      </motion.div>
    </div>
  );
}

function Love({ picked, setPicked }: { picked: Set<number>; setPicked: (s: Set<number>) => void }) {
  const [tab, setTab] = useState<Media | 'all'>('all');
  const { data, error, isPending, refetch } = useQuery({ queryKey: ['seeds'], queryFn: () => api<{ cards: Card[]; missing: string[] }>('taste.seeds'), staleTime: Infinity, retry: false });
  const cards = useMemo(() => {
    const all = data?.cards ?? [];
    if (tab !== 'all') return all.filter((c) => c.media === tab);
    // Interleave the media so the first screen shows a bit of everything.
    const by: Record<string, Card[]> = {};
    for (const c of all) (by[c.media] ??= []).push(c);
    const out: Card[] = [];
    for (let i = 0; out.length < all.length; i++) for (const m of MEDIA) if (by[m.id]?.[i]) out.push(by[m.id][i]);
    return out;
  }, [data, tab]);
  const toggle = (id: number) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setPicked(next);
  };
  if (isPending) {
    return (
      <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(112px, 1fr))' }} aria-busy="true">
        {Array.from({ length: 24 }, (_, i) => (
          <div key={i} className="skeleton aspect-2/3 rounded-card!" />
        ))}
      </div>
    );
  }
  if (error || (data && data.cards.length === 0)) {
    const offline = (error as { kind?: string } | null)?.kind === 'offline';
    return (
      <div className="hairline rounded-card bg-surface p-6 text-center">
        <Icon name={offline ? 'wifiOff' : 'plug'} size={28} className="mx-auto text-faint" />
        <p className="mt-3 font-medium">{offline ? "You're offline, so the catalog can't be reached" : "The catalogs aren't set up yet"}</p>
        <p className="mx-auto mt-1 max-w-md text-[13px] text-subtext">{offline ? 'Skip this for now. Quick Rate does the same job once you are back online.' : 'This step needs the TMDB, IGDB or Last.fm keys (Settings → API keys). Skip it for now; Quick Rate does the same job later.'}</p>
        <Button className="mt-4" icon="sync" onClick={() => void refetch()}>
          Try again
        </Button>
      </div>
    );
  }
  return (
    <>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Chip selected={tab === 'all'} onClick={() => setTab('all')}>
          Everything
        </Chip>
        {MEDIA.map((m) => (
          <Chip key={m.id} selected={tab === m.id} icon={mediaIcon[m.id]} onClick={() => setTab(m.id)}>
            {m.plural}
          </Chip>
        ))}
        <span className={cx('tnum ml-auto rounded-full px-3 py-1 text-[13px] font-semibold', picked.size >= 10 ? 'bg-good/15 text-good' : 'bg-strong text-subtext')} aria-live="polite">
          {picked.size} picked{picked.size >= 10 ? ' · plenty to go on' : picked.size > 0 ? ` · ${10 - picked.size} more would be ideal` : ''}
        </span>
      </div>
      {data && data.missing.length > 0 && <p className="mb-3 text-[12px] text-faint">Not shown because a key is missing: {data.missing.map((m) => ({ tmdb: 'movies and TV', igdb: 'games', lastfm: 'music' })[m] ?? m).join(', ')}.</p>}
      <ul className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(112px, 1fr))' }}>
        {cards.map((c) => {
          const on = picked.has(c.id);
          return (
            <li key={c.id}>
              <button
                type="button"
                role="checkbox"
                aria-checked={on}
                aria-label={`${c.title}${c.year ? `, ${c.year}` : ''}`}
                title={c.title}
                onClick={() => toggle(c.id)}
                className={cx('group relative block w-full overflow-hidden rounded-card transition-transform duration-150 active:scale-95', on ? 'ring-3 ring-accent' : 'hover:-translate-y-1')}
              >
                <Art card={c} className="aspect-2/3 w-full" />
                <span className={cx('absolute inset-0 transition-colors', on ? 'bg-black/45' : 'bg-transparent group-hover:bg-black/20')} />
                <AnimatePresence>
                  {on && (
                    <motion.span initial={{ scale: 0 }} animate={{ scale: 1 }} exit={{ scale: 0 }} transition={{ type: 'spring', stiffness: 500, damping: 22 }} className="absolute inset-0 m-auto grid h-11 w-11 place-items-center rounded-full bg-accent text-on-accent shadow-lg">
                      <Icon name="heart" size={22} filled />
                    </motion.span>
                  )}
                </AnimatePresence>
                <span className="absolute left-1.5 top-1.5 grid h-5.5 w-5.5 place-items-center rounded-md bg-black/60 text-white">
                  <Icon name={mediaIcon[c.media]} size={12} />
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </>
  );
}

function RegionStep() {
  const [region, setRegion] = useSetting('region', 'US');
  const [mine, setMine] = useSetting<number[]>('my_providers', []);
  const { data, isPending } = useQuery({ queryKey: ['providers', region], queryFn: () => api<{ id: number; name: string; logo: string | null }[]>('providers'), staleTime: 3_600_000, retry: false });
  return (
    <>
      <label className="mb-6 flex max-w-sm flex-col gap-1.5">
        <span className="text-[13px] font-medium">Country</span>
        <Select label="Country" value={region} onChange={setRegion} options={REGIONS.map(([value, label]) => ({ value, label }))} />
      </label>
      <h3 className="mb-2 text-[13px] font-medium">Streaming services you have</h3>
      {isPending ? (
        <div className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))' }} aria-busy="true">
          {Array.from({ length: 12 }, (_, i) => (
            <div key={i} className="skeleton h-13" />
          ))}
        </div>
      ) : !data || data.length === 0 ? (
        <p className="hairline rounded-card bg-surface p-4 text-[13px] text-subtext">The list of services couldn't be loaded right now. You can choose them later in Settings → Streaming services.</p>
      ) : (
        <ul className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))' }}>
          {data.slice(0, 24).map((p) => {
            const on = mine.includes(p.id);
            return (
              <li key={p.id}>
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={on}
                  onClick={() => setMine(on ? mine.filter((x) => x !== p.id) : [...mine, p.id])}
                  className={cx('hairline flex w-full items-center gap-2.5 rounded-control p-2 text-left transition-colors', on ? 'border-accent! bg-accent/10' : 'bg-surface hover:bg-hover')}
                >
                  {p.logo ? (
                    <img src={img(p.logo)} alt="" className="h-9 w-9 rounded-md" />
                  ) : (
                    <span className="grid h-9 w-9 place-items-center rounded-md bg-strong">
                      <Icon name="tv" size={16} />
                    </span>
                  )}
                  <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{p.name}</span>
                  {on && <Icon name="check" size={16} className="text-accent" />}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}

function ThemeStep() {
  const boot = useApp((s) => s.boot)!;
  const sys = useApp((s) => s.sys);
  const setSetting = useApp((s) => s.setSetting);
  const a = appearanceFrom(boot.settings);
  const set = (patch: Partial<typeof a>) => void setSetting('appearance', { ...a, ...patch });
  const followingSuite = a.useSuiteTheme && !!sys?.hangar_installed && !!boot.settings['hangar.theme'];
  return (
    <>
      {sys?.hangar_installed && (
        <div className="mb-4">
          <Chip selected={followingSuite} icon="link" onClick={() => set({ useSuiteTheme: !a.useSuiteTheme })}>
            Use the Hangar suite theme
          </Chip>
        </div>
      )}
      <ul className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))' }} role="radiogroup" aria-label="Theme">
        {boot.themes.map((t) => {
          const selected = !followingSuite && a.themeId === t.id;
          const vars = themeVars(t, null, sys?.accent ?? null);
          return (
            <li key={t.id}>
              <button
                type="button"
                role="radio"
                aria-checked={selected}
                data-theme-option={t.id}
                onClick={() => set({ themeId: t.id, useSuiteTheme: false })}
                className={cx('hairline block w-full rounded-card bg-surface p-2 text-left transition-transform hover:-translate-y-0.5', selected && 'border-accent! ring-1 ring-accent')}
              >
                <span className="relative block aspect-video overflow-hidden rounded-[10px]" style={{ background: t.colors.background }} aria-hidden="true">
                  <span className="absolute inset-y-0 left-0 w-1/5" style={{ background: t.colors.sidebar }} />
                  <span className="absolute left-[27%] top-[20%] h-1.5 w-2/5 rounded-full" style={{ background: t.colors.text }} />
                  {[0, 1, 2].map((i) => (
                    <span key={i} className="absolute bottom-[14%] h-[36%] w-[17%] rounded-[4px]" style={{ left: `${27 + i * 21}%`, background: i === 0 ? vars['--accent'] : t.colors.surfaceStrong }} />
                  ))}
                </span>
                <span className="mt-2 flex items-center gap-1.5 px-1 text-[13px] font-semibold">
                  {selected && <Icon name="check" size={13} className="text-accent" />}
                  {t.name}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-6 flex flex-wrap items-center gap-3">
        <span className="text-[13px] font-medium">Motion</span>
        <Segmented
          label="Motion"
          size="sm"
          value={a.motion}
          onChange={(motion) => set({ motion })}
          options={[
            { value: 'full', label: 'Full' },
            { value: 'reduced', label: 'Reduced' },
            { value: 'off', label: 'Off' },
          ]}
        />
      </div>
    </>
  );
}

/** The last step: seeds the picks, runs a real sync, and shows its actual progress. */
function Build({ picked, finish }: { picked: Set<number>; finish: () => void }) {
  const sync = useApp((s) => s.sync);
  const qc = useQueryClient();
  const [phase, setPhase] = useState<'seeding' | 'syncing' | 'done' | 'failed'>('seeding');
  const [log, setLog] = useState<string[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const [summary, setSummary] = useState<{ has_taste: boolean; connected: number } | null>(null);
  const started = useRef(false);

  const run = async () => {
    setProblem(null);
    setPhase('seeding');
    try {
      if (picked.size > 0) {
        setLog((l) => [...l, `Learning from the ${picked.size} things you picked`]);
        await api('taste.seed', { ids: [...picked] });
      }
      setPhase('syncing');
      useApp.getState().set({ sync: { running: true, label: 'Starting', pct: 0.01 } });
      const out = await api<{ offline?: boolean; warnings?: string[] }>('sync');
      if (out?.offline) setProblem("You're offline, so only what you picked could be used. Marquee will sync by itself when you're back.");
      await useApp.getState().refreshBoot();
      await qc.invalidateQueries();
      const boot = useApp.getState().boot!;
      setSummary({ has_taste: boot.has_taste, connected: boot.sources.filter((s) => s.status !== 'disconnected').length });
      setPhase('done');
    } catch (e) {
      setProblem(errorMessage(e));
      setPhase('failed');
    }
  };
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void run();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // Keep a short trail of what the sync has been doing.
  useEffect(() => {
    if (phase === 'syncing' && sync.label) setLog((l) => (l[l.length - 1] === sync.label ? l : [...l, sync.label].slice(-6)));
  }, [sync.label, phase]);

  const pct = phase === 'done' ? 1 : phase === 'seeding' ? 0.04 : Math.max(0.06, sync.pct);
  return (
    <div className="flex h-full flex-col items-center justify-center text-center">
      <Logo size={96} active={phase === 'seeding' || phase === 'syncing'} />
      <h1 className="display mt-7 text-[34px] font-extrabold leading-tight">{phase === 'done' ? "You're all set" : phase === 'failed' ? "That didn't finish" : 'Building your taste profile…'}</h1>
      <p className="mt-2 max-w-lg text-subtext" aria-live="polite">
        {phase === 'done'
          ? summary?.has_taste
            ? 'Your Home feed is ready, with picks across movies, shows, games and music, each with its reason.'
            : 'Marquee is ready. It has no history to go on yet, so Home shows what is popular; rate a few things or connect a service and it becomes yours.'
          : phase === 'failed'
            ? problem
            : 'Reading your history, fetching new releases and scoring them against your taste. This is the real sync, not a loading screen.'}
      </p>
      {phase !== 'failed' && (
        <div className="mt-8 w-full max-w-md">
          <div className="h-2 overflow-hidden rounded-full bg-strong" role="progressbar" aria-label="Sync progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct * 100)}>
            <motion.div className="h-full rounded-full bg-accent" animate={{ width: `${pct * 100}%` }} transition={{ ease: 'easeOut', duration: 0.5 }} />
          </div>
          <ul className="mt-4 min-h-28 space-y-1 text-left text-[13px]">
            <AnimatePresence initial={false}>
              {log.map((line, i) => (
                <motion.li key={`${i}-${line}`} initial={{ opacity: 0, y: 6 }} animate={{ opacity: i === log.length - 1 && phase !== 'done' ? 1 : 0.55, y: 0 }} className="flex items-center gap-2">
                  <Icon name={i === log.length - 1 && phase !== 'done' ? 'sync' : 'check'} size={14} className={i === log.length - 1 && phase !== 'done' ? 'text-accent' : 'text-good'} />
                  {line}
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        </div>
      )}
      {phase === 'done' && problem && <p className="mb-2 max-w-md rounded-control bg-warn/12 px-3 py-2 text-[13px] text-warn">{problem}</p>}
      <div className="mt-4 flex gap-2">
        {phase === 'failed' && (
          <Button icon="sync" onClick={() => void run()}>
            Try again
          </Button>
        )}
        {(phase === 'done' || phase === 'failed') && (
          <Button variant="primary" size="lg" autoFocus onClick={finish}>
            Open Marquee
            <Icon name="chevronRight" size={18} />
          </Button>
        )}
      </div>
    </div>
  );
}

export default function Onboarding() {
  const [index, setIndex] = useState(0);
  const [dir, setDir] = useState(1);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const sources = useApp((s) => s.boot?.sources ?? EMPTY);
  const step = STEPS[index];
  const go = (to: number) => {
    setDir(to > index ? 1 : -1);
    setIndex(Math.max(0, Math.min(STEPS.length - 1, to)));
  };
  const finish = async () => {
    await api('settings.set', { key: 'onboarded', value: true }).catch(() => {});
    location.hash = '#/home';
    await useApp.getState().refreshBoot();
  };
  const connected = sources.filter((s) => s.status !== 'disconnected' && !['serializd', 'import'].includes(s.id)).length;
  const framed = step !== 'welcome' && step !== 'build';
  const [title, subtitle] = TITLES[step];

  let body: ReactNode;
  if (step === 'welcome') body = <Welcome next={() => go(1)} />;
  else if (step === 'connect') body = <ConnectionsList compact only={['steam', 'trakt', 'spotify', 'lastfm', 'letterboxd']} />;
  else if (step === 'import') body = <ConnectionsList compact only={['import', 'serializd']} />;
  else if (step === 'love') body = <Love picked={picked} setPicked={setPicked} />;
  else if (step === 'region') body = <RegionStep />;
  else if (step === 'theme') body = <ThemeStep />;
  else body = <Build picked={picked} finish={() => void finish()} />;

  const nextLabel = step === 'connect' ? (connected > 0 ? 'Continue' : 'Skip for now') : step === 'import' ? 'Continue' : step === 'love' ? (picked.size > 0 ? 'Continue' : 'Skip for now') : step === 'theme' ? 'Build my feed' : 'Continue';

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-bg">
      <div className="pointer-events-none absolute inset-0 opacity-60" style={{ background: 'radial-gradient(60% 50% at 50% 0%, color-mix(in srgb, var(--accent) 16%, transparent), transparent 70%)' }} />
      <header className="drag relative z-10 flex h-11 shrink-0 items-center justify-between pl-4">
        <span className="flex items-center gap-2 text-[13px] font-semibold text-subtext">
          <Logo size={18} /> Marquee
        </span>
        <WindowControls />
      </header>
      <main className="relative z-10 flex min-h-0 flex-1 flex-col">
        <AnimatePresence mode="wait" custom={dir} initial={false}>
          <motion.div
            key={step}
            custom={dir}
            variants={{ enter: (d: number) => ({ opacity: 0, x: d * 40 }), center: { opacity: 1, x: 0 }, exit: (d: number) => ({ opacity: 0, x: d * -40 }) }}
            initial="enter"
            animate="center"
            exit="exit"
            transition={{ duration: 0.22, ease: [0.2, 0.7, 0.2, 1] }}
            className="flex min-h-0 flex-1 flex-col"
          >
            {framed ? (
              <>
                <div className="mx-auto w-full max-w-4xl shrink-0 px-8 pb-5 pt-6">
                  <h1 className="display text-[30px] font-bold leading-tight">{title}</h1>
                  <p className="mt-1.5 max-w-2xl text-subtext">{subtitle}</p>
                </div>
                <div className="min-h-0 flex-1 overflow-y-auto">
                  <div className="mx-auto w-full max-w-4xl px-8 pb-6">{body}</div>
                </div>
              </>
            ) : (
              <div className="min-h-0 flex-1 px-8">{body}</div>
            )}
          </motion.div>
        </AnimatePresence>
      </main>
      {framed && (
        <footer className="relative z-10 flex shrink-0 items-center justify-between gap-4 border-t border-white/8 px-8 py-4">
          <Button variant="ghost" icon="arrowLeft" onClick={() => go(index - 1)}>
            Back
          </Button>
          <ol className="flex items-center gap-2" aria-label={`Step ${index} of ${STEPS.length - 2}`}>
            {STEPS.slice(1, -1).map((s, i) => (
              <li key={s} className={cx('h-1.5 rounded-full transition-all duration-300', i + 1 === index ? 'w-7 bg-accent' : i + 1 < index ? 'w-1.5 bg-accent/60' : 'w-1.5 bg-strong')} />
            ))}
          </ol>
          <Button variant="primary" onClick={() => go(index + 1)}>
            {nextLabel}
            <Icon name="chevronRight" size={16} />
          </Button>
        </footer>
      )}
    </div>
  );
}
