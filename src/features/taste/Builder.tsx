/** Build your taste: shape the profile directly. Love, like, dislike or never-show genres, moods
 * and creators; rate titles you know; set how adventurous picks should be. Every change re-scores,
 * and the preview on the right shows what your picks become. */
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';
import { api, errorMessage } from '@/lib/api';
import { typeBadge } from '@/lib/format';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { useRate } from '@/ui/actions';
import { Art, MatchRing } from '@/ui/Card';
import { Icon, type IconName } from '@/ui/icons';
import { Button, ErrorState, Input, PageHeader, Segmented, Skeleton, cx } from '@/ui/primitives';
import { Stars } from '@/ui/Stars';
import { Dial } from './Taste';

type Choice = 'love' | 'like' | 'dislike' | 'block';
type Section = 'movie' | 'tv' | 'anime' | 'game' | 'album';
type Step = 'genres' | 'moods' | 'creators' | 'rate' | 'dial';

interface Chip {
  feature: string;
  label: string;
  count: number;
  choice: Choice | null;
  learned: number | null;
  role?: string;
  yours?: number;
}

interface BuilderData {
  sections: Record<Section, { genres: Chip[]; rated: number }>;
  themes: Chip[];
  creators: Chip[];
  other: Chip[];
  counts: { loved: number; liked: number; disliked: number; blocked: number };
  dial: number;
}

const STEPS: { id: Step; label: string; icon: IconName }[] = [
  { id: 'genres', label: 'Genres', icon: 'film' },
  { id: 'moods', label: 'Moods & themes', icon: 'sparkles' },
  { id: 'creators', label: 'Creators', icon: 'user' },
  { id: 'rate', label: 'Rate titles', icon: 'star' },
  { id: 'dial', label: 'Adventure', icon: 'compass' },
];

const SECTIONS: { value: Section; label: string }[] = [
  { value: 'movie', label: 'Movies' },
  { value: 'tv', label: 'TV' },
  { value: 'anime', label: 'Anime' },
  { value: 'game', label: 'Games' },
  { value: 'album', label: 'Music' },
];

/** Click cycles: nothing → like → love → dislike → never → nothing. */
const NEXT: Record<string, Choice | null> = { none: 'like', like: 'love', love: 'dislike', dislike: 'block', block: null };
const LOOK: Record<Choice, { icon: IconName; text: string; cls: string }> = {
  like: { icon: 'thumbUp', text: 'Like', cls: 'bg-good/15 text-good ring-1 ring-good/40' },
  love: { icon: 'heart', text: 'Love', cls: 'bg-accent text-on-accent' },
  dislike: { icon: 'thumbDown', text: 'Dislike', cls: 'bg-warn/15 text-warn ring-1 ring-warn/40' },
  block: { icon: 'x', text: 'Never', cls: 'bg-bad/15 text-bad line-through ring-1 ring-bad/40' },
};

export default function Builder({ step: initial }: { step?: string }) {
  const [step, setStep] = useState<Step>(STEPS.some((s) => s.id === initial) ? (initial as Step) : 'genres');
  const navigate = useApp((s) => s.navigate);
  const toast = useApp((s) => s.toast);
  const qc = useQueryClient();
  const { data, error, refetch } = useQuery({ queryKey: ['builder'], queryFn: () => api<BuilderData>('taste.builder') });
  // Choices show at once; the engine catches up a moment later.
  const [local, setLocal] = useState<Record<string, Choice | null>>({});
  const choiceOf = (c: Chip) => (c.feature in local ? local[c.feature] : c.choice);
  const choose = async (c: Chip, next: Choice | null) => {
    setLocal((l) => ({ ...l, [c.feature]: next }));
    try {
      await api('taste.override', { feature: c.feature, mode: next === 'love' ? 'pin' : next, later: true });
      void qc.invalidateQueries({ queryKey: ['builder'] });
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };

  const index = STEPS.findIndex((s) => s.id === step);
  return (
    <div className="pb-14">
      <PageHeader title="Build your taste" subtitle="Tell Marquee what you're into, straight. It sits on top of what it learns from your history.">
        <Button icon="star" onClick={() => navigate('/ratings')}>
          Your ratings
        </Button>
        <Button icon="arrowLeft" onClick={() => navigate('/taste')}>
          Your taste
        </Button>
      </PageHeader>
      <nav aria-label="Steps" className="mx-(--pad) mb-6 flex flex-wrap gap-2">
        {STEPS.map((s, i) => (
          <button
            key={s.id}
            type="button"
            aria-current={s.id === step ? 'step' : undefined}
            onClick={() => setStep(s.id)}
            className={cx('flex items-center gap-2 rounded-full px-3.5 py-2 text-[14px] font-medium transition-colors', s.id === step ? 'bg-accent text-on-accent' : 'bg-surface text-subtext hover:bg-hover hover:text-text')}
          >
            <span className={cx('tnum grid h-5 w-5 place-items-center rounded-full text-[11px] font-bold', s.id === step ? 'bg-black/15' : 'bg-white/8')}>{i + 1}</span>
            {s.label}
          </button>
        ))}
      </nav>
      {error ? (
        <ErrorState error={error} onRetry={() => void refetch()} />
      ) : !data ? (
        <div className="mx-(--pad) grid gap-3">
          <Skeleton className="h-10 w-96" />
          <Skeleton className="h-64 w-full rounded-card!" />
        </div>
      ) : (
        <div className="mx-(--pad) grid gap-8 xl:grid-cols-[1fr_340px]">
          <div className="min-w-0">
            {step !== 'rate' && step !== 'dial' && <Legend counts={data.counts} />}
            {step === 'genres' && <Genres data={data} choiceOf={choiceOf} choose={choose} />}
            {step === 'moods' && <ChipGroup title="Moods, themes and styles" hint="These cross every medium: a mood you love lifts films, shows, games and music that share it." chips={[...data.themes, ...data.other]} choiceOf={choiceOf} choose={choose} />}
            {step === 'creators' && <Creators data={data} choiceOf={choiceOf} choose={choose} />}
            {step === 'rate' && <RateTitles />}
            {step === 'dial' && (
              <section aria-label="How adventurous">
                <h2 className="mb-1 text-[18px] font-semibold">How adventurous should picks be?</h2>
                <p className="mb-5 text-subtext">Comfort sticks close to what you already love. Adventurous reaches further out, so expect more surprises (and a few misses).</p>
                <Dial value={data.dial} />
              </section>
            )}
            <div className="mt-8 flex justify-between">
              <Button icon="chevronLeft" disabled={index === 0} onClick={() => setStep(STEPS[Math.max(0, index - 1)].id)}>
                Back
              </Button>
              {index < STEPS.length - 1 ? (
                <Button variant="primary" icon="chevronRight" onClick={() => setStep(STEPS[index + 1].id)}>
                  Next: {STEPS[index + 1].label}
                </Button>
              ) : (
                <Button variant="primary" icon="sparkles" onClick={() => navigate('/for-you')}>
                  See your picks
                </Button>
              )}
            </div>
          </div>
          <Preview />
        </div>
      )}
    </div>
  );
}

function Legend({ counts }: { counts: BuilderData['counts'] }) {
  return (
    <p className="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2 text-[13px] text-subtext">
      <span>Click to cycle:</span>
      {(['like', 'love', 'dislike', 'block'] as Choice[]).map((c) => (
        <span key={c} className={cx('inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[12px] font-semibold', LOOK[c].cls)}>
          <Icon name={LOOK[c].icon} size={12} />
          {LOOK[c].text}
        </span>
      ))}
      <span className="text-faint">
        {counts.loved} loved · {counts.liked} liked · {counts.disliked} disliked · {counts.blocked} never
      </span>
    </p>
  );
}

function TasteChip({ chip, choice, onChoose }: { chip: Chip; choice: Choice | null; onChoose: (c: Choice | null) => void }) {
  const next = NEXT[choice ?? 'none'];
  const look = choice ? LOOK[choice] : null;
  const learned = chip.learned ?? 0;
  return (
    <button
      type="button"
      onClick={() => onChoose(next)}
      onContextMenu={(e) => {
        e.preventDefault();
        onChoose(null);
      }}
      aria-label={`${chip.label}: ${look ? look.text.toLowerCase() : 'no preference'}. Click for ${next ? LOOK[next].text.toLowerCase() : 'no preference'}.`}
      title={learned > 0.25 ? 'Your history already leans toward this' : learned < -0.25 ? 'Your history leans away from this' : undefined}
      className={cx('inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[14px] font-medium capitalize transition-all active:scale-95', look ? look.cls : 'bg-surface text-text hover:bg-hover')}
    >
      {look && <Icon name={look.icon} size={13} />}
      <span>{chip.label}</span>
      {!choice && learned > 0.25 && <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-accent" />}
      {chip.role && <span className="text-[11px] font-normal normal-case opacity-60">{chip.role}</span>}
    </button>
  );
}

type ChooseFn = (c: Chip, next: Choice | null) => void;

function ChipGroup({ title, hint, chips, choiceOf, choose }: { title: string; hint?: string; chips: Chip[]; choiceOf: (c: Chip) => Choice | null; choose: ChooseFn }) {
  return (
    <section aria-label={title}>
      <h2 className="mb-1 text-[18px] font-semibold">{title}</h2>
      {hint && <p className="mb-4 text-subtext">{hint}</p>}
      {chips.length === 0 ? (
        <p className="text-faint">Nothing here yet. Sync to fill the catalog, then come back.</p>
      ) : (
        <div className="flex flex-wrap gap-2">
          {chips.map((c) => (
            <TasteChip key={c.feature} chip={c} choice={choiceOf(c)} onChoose={(n) => choose(c, n)} />
          ))}
        </div>
      )}
      <p className="mt-4 text-[12px] text-faint">A dot means your history already leans that way. Right-click a chip to clear it.</p>
    </section>
  );
}

function Genres({ data, choiceOf, choose }: { data: BuilderData; choiceOf: (c: Chip) => Choice | null; choose: ChooseFn }) {
  const [section, setSection] = useState<Section>('movie');
  return (
    <>
      <div className="mb-5">
        <Segmented label="Medium" size="sm" value={section} onChange={setSection} options={SECTIONS} />
      </div>
      <ChipGroup title={`${SECTIONS.find((s) => s.value === section)!.label} genres`} chips={data.sections[section].genres} choiceOf={choiceOf} choose={choose} />
    </>
  );
}

function Creators({ data, choiceOf, choose }: { data: BuilderData; choiceOf: (c: Chip) => Choice | null; choose: ChooseFn }) {
  const [q, setQ] = useState('');
  const [debounced, setDebounced] = useState('');
  useEffect(() => {
    const t = setTimeout(() => setDebounced(q.trim()), 250);
    return () => clearTimeout(t);
  }, [q]);
  const search = useQuery({ queryKey: ['builder-creators', debounced], queryFn: () => api<Chip[]>('taste.creators', { q: debounced }), enabled: debounced.length >= 2 });
  const chips = debounced.length >= 2 ? (search.data ?? []) : data.creators;
  return (
    <>
      <div className="mb-5 max-w-md">
        <Input aria-label="Find a director, developer, studio or artist" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Find a director, developer, studio or artist…" icon="search" />
      </div>
      <ChipGroup title={debounced.length >= 2 ? `Creators matching “${debounced}”` : 'Creators'} hint="Love a director and their next film rises to the top; dislike a studio and its games sink." chips={chips} choiceOf={choiceOf} choose={choose} />
    </>
  );
}

function RateTitles() {
  const [section, setSection] = useState<Section>('movie');
  const [skip, setSkip] = useState<number[]>([]);
  const [rated, setRated] = useState<Record<number, number>>({});
  const reduce = useReducedMotion();
  const rate = useRate();
  const { data, isPending } = useQuery({ queryKey: ['to-rate', section, skip.length > 0 && Math.floor(skip.length / 12)], queryFn: () => api<Card[]>('taste.to_rate', { section, skip, limit: 24 }), placeholderData: (p) => p });
  const visible = useMemo(() => (data ?? []).filter((c) => !skip.includes(c.id)), [data, skip]);
  const count = Object.keys(rated).length;
  return (
    <section aria-label="Rate titles">
      <div className="mb-2 flex flex-wrap items-center gap-3">
        <Segmented
          label="Medium"
          size="sm"
          value={section}
          onChange={(s) => {
            setSection(s);
            setSkip([]);
          }}
          options={SECTIONS}
        />
        {count > 0 && <span className="text-[13px] text-subtext">{count} rated this visit</span>}
      </div>
      <p className="mb-5 text-subtext">Things you've watched or played but not rated come first, then popular titles. Rate what you know; "Haven't seen" skips one.</p>
      {isPending && !data ? (
        <div className="grid gap-4" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))' }}>
          {Array.from({ length: 10 }, (_, i) => (
            <Skeleton key={i} className="aspect-2/3 w-full rounded-card!" />
          ))}
        </div>
      ) : visible.length === 0 ? (
        <p className="text-faint">Nothing left to rate here. Try another medium, or sync for more titles.</p>
      ) : (
        <ul className="grid gap-x-4 gap-y-6" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))' }}>
          <AnimatePresence initial={false}>
            {visible.map((c) => (
              <motion.li key={c.id} layout={!reduce} exit={reduce ? { opacity: 0 } : { opacity: 0, scale: 0.9 }} transition={{ duration: 0.18 }}>
                <div className="overflow-hidden rounded-card shadow-md">
                  <Art card={c} className={c.media === 'album' ? 'aspect-square w-full' : 'aspect-2/3 w-full'} />
                </div>
                <p className="mt-2 truncate text-[13px] font-semibold">{c.title}</p>
                <p className="truncate text-[11px] text-subtext">
                  {typeBadge(c)}
                  {c.release_date ? ` · ${c.release_date.slice(0, 4)}` : ''}
                </p>
                <div className="mt-1">
                  <Stars
                    size={20}
                    value={rated[c.id] ?? null}
                    label={`Rate ${c.title}`}
                    onChange={(v) => {
                      setRated((r) => ({ ...r, [c.id]: v }));
                      void rate(c, v, true);
                      // Let the stars show for a moment, then make room for the next title.
                      if (v > 0) setTimeout(() => setSkip((s) => [...s, c.id]), 650);
                    }}
                  />
                </div>
                <button type="button" className="mt-0.5 text-[12px] text-faint hover:text-text" onClick={() => setSkip((s) => [...s, c.id])}>
                  Haven't seen it
                </button>
              </motion.li>
            ))}
          </AnimatePresence>
        </ul>
      )}
    </section>
  );
}

/** What your picks look like right now; refreshes as the engine re-scores. */
function Preview() {
  const openItem = useApp((s) => s.openItem);
  const { data, isFetching } = useQuery({ queryKey: ['for_you', 'builder-preview'], queryFn: () => api<{ cards: Card[] }>('for_you', { offset: 0, limit: 8 }) });
  return (
    <aside aria-label="Your top picks now" className="self-start rounded-card bg-surface p-4 xl:sticky xl:top-4">
      <h2 className="mb-3 flex items-center gap-2 text-[13px] font-semibold uppercase tracking-wider text-subtext">
        Your top picks now
        {isFetching && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" aria-label="Updating" />}
      </h2>
      {!data ? (
        <Skeleton className="h-80 w-full" />
      ) : data.cards.length === 0 ? (
        <p className="text-[13px] text-faint">Pick a few things you love and your picks appear here.</p>
      ) : (
        <ol className="flex flex-col gap-2">
          {data.cards.map((c) => (
            <li key={c.id}>
              <button type="button" onClick={() => openItem(c.id, 'builder')} className="flex w-full items-center gap-3 rounded-control p-1 text-left hover:bg-hover">
                <span className="w-9 shrink-0 overflow-hidden rounded">
                  <Art card={c} className={c.media === 'album' ? 'aspect-square w-full' : 'aspect-2/3 w-full'} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] font-semibold">{c.title}</span>
                  <span className="block truncate text-[11px] text-subtext">{c.why[0]?.replaceAll('*', '') ?? typeBadge(c)}</span>
                </span>
                {c.match_pct != null && <MatchRing pct={c.match_pct} size={30} stroke={3} />}
              </button>
            </li>
          ))}
        </ol>
      )}
    </aside>
  );
}
