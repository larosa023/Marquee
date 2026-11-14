// Home, "Now Showing": the hero carousel and the rows.
import { useQuery } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { api } from '@/lib/api';
import { credit, reasonParts, releaseLabel } from '@/lib/format';
import { img } from '@/lib/img';
import type { Card } from '@/lib/types';
import { useApp, useSetting } from '@/state/app';
import { useQuickAdd } from '@/ui/actions';
import { MatchRing, TypeBadge } from '@/ui/Card';
import { Icon } from '@/ui/icons';
import { useCardMenu } from '@/ui/CardMenu';
import { Button, EmptyState, ErrorState, OfflineBanner, cx } from '@/ui/primitives';
import { Row, RowSkeleton } from '@/ui/Row';

interface HomeData {
  hero: Card[];
  rows: { id: string; title: string; subtitle: string; cards: Card[] }[];
  personal: boolean;
  counts: { items: number; events: number };
}

const SPARKS = [
  { kind: 'movie', label: 'A movie', icon: 'film' },
  { kind: 'tv', label: 'A show', icon: 'tv' },
  { kind: 'game', label: 'A game', icon: 'gamepad' },
  { kind: 'album', label: 'An album', icon: 'music' },
  { kind: 'any', label: 'Anything', icon: 'dice' },
] as const;

/** In December and January, a nudge toward the year in review. */
function WrappedCallout() {
  const now = new Date();
  const month = now.getMonth();
  if (month !== 11 && month !== 0) return null;
  const year = month === 0 ? now.getFullYear() - 1 : now.getFullYear();
  return (
    <button type="button" onClick={() => useApp.getState().navigate(`/wrapped/${year}`)} className="mx-(--pad) mb-5 flex w-[calc(100%-2*var(--pad))] items-center gap-4 rounded-card bg-accent/12 px-5 py-4 text-left transition-colors hover:bg-accent/18">
      <Icon name="chart" size={22} className="text-accent" />
      <span className="flex-1">
        <span className="block font-semibold">Your {year} in Marquee is ready</span>
        <span className="text-[13px] text-subtext">Hours watched, played and listened, your favourites, and what you were into.</span>
      </span>
      <Icon name="chevronRight" size={18} className="text-subtext" />
    </button>
  );
}

/** "Need a spark?": one tap rolls a random pick of each kind. */
function Spark() {
  const open = (kind: (typeof SPARKS)[number]['kind']) => useApp.getState().set({ surprise: kind });
  return (
    <section aria-labelledby="spark-title" className="mx-(--pad) mb-9 flex flex-wrap items-center gap-x-5 gap-y-3 rounded-card bg-surface px-5 py-4">
      <div className="mr-auto">
        <h2 id="spark-title" className="font-semibold">
          Need a spark?
        </h2>
        <p className="text-[13px] text-subtext">Roll a random pick you'd probably like.</p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button icon="play" onClick={() => useApp.getState().navigate('/reel')}>
          Trailer reel
        </Button>
        {SPARKS.map((s) => (
          <Button key={s.kind} icon={s.icon} variant={s.kind === 'any' ? 'primary' : 'secondary'} onClick={() => open(s.kind)}>
            {s.label}
          </Button>
        ))}
      </div>
    </section>
  );
}

/** A reason with titles in italics. */
export function Reason({ text, className }: { text: string; className?: string }) {
  return (
    <span className={className}>
      {reasonParts(text).map((p, i) =>
        p.em ? (
          <em key={i} className="font-medium text-text not-italic">
            {p.text}
          </em>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </span>
  );
}

function Trailer({ card }: { card: Card }) {
  const t = card.meta?.trailer;
  if (!t) return null;
  if (t.site === 'youtube' && t.key) {
    return (
      <iframe
        title={`${card.title} trailer`}
        className="pointer-events-none absolute left-1/2 top-1/2 aspect-video min-h-full min-w-full -translate-x-1/2 -translate-y-1/2"
        src={`https://www.youtube-nocookie.com/embed/${t.key}?autoplay=1&mute=1&controls=0&loop=1&playlist=${t.key}&modestbranding=1&playsinline=1&rel=0`}
        allow="autoplay; encrypted-media"
        referrerPolicy="strict-origin-when-cross-origin"
        tabIndex={-1}
      />
    );
  }
  if (t.url) return <video className="absolute inset-0 h-full w-full object-cover" src={t.url} autoPlay muted loop playsInline />;
  return null;
}

function Hero({ cards }: { cards: Card[] }) {
  const [index, setIndex] = useState(0);
  const [hovering, setHovering] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [autoplay] = useSetting('home.trailer_autoplay', true);
  const openItem = useApp((s) => s.openItem);
  const quickAdd = useQuickAdd();
  const card = cards[index % cards.length];
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Advance every eight seconds, unless the pointer is on it or keyboard focus is inside it.
  useEffect(() => {
    if (hovering || cards.length < 2) return;
    const t = setInterval(() => setIndex((i) => (i + 1) % cards.length), 8000);
    return () => clearInterval(t);
  }, [hovering, cards.length]);

  // Hovering for a moment starts the trailer, muted.
  useEffect(() => {
    clearTimeout(timer.current);
    setPlaying(false);
    if (hovering && autoplay && card?.meta?.trailer) timer.current = setTimeout(() => setPlaying(true), 1100);
    return () => clearTimeout(timer.current);
  }, [hovering, autoplay, card?.id, card?.meta?.trailer]);

  if (!card) return null;
  const sub = credit(card);
  return (
    <section
      aria-roledescription="carousel"
      aria-label="This week's biggest releases for you"
      className="relative mx-(--pad) mb-10 h-[min(56vh,520px)] min-h-90 overflow-hidden rounded-[calc(var(--radius)+6px)] bg-surface"
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
      onFocus={() => setHovering(true)}
      onBlur={() => setHovering(false)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowRight') setIndex((i) => (i + 1) % cards.length);
        if (e.key === 'ArrowLeft') setIndex((i) => (i - 1 + cards.length) % cards.length);
      }}
    >
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div key={card.id} className="absolute inset-0" initial={{ opacity: 0, scale: 1.04 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.7, ease: [0.2, 0.7, 0.2, 1] }}>
          {card.backdrop ? <img src={img(card.backdrop)} alt="" className="h-full w-full object-cover" /> : card.poster && <img src={img(card.poster)} alt="" className="h-full w-full scale-125 object-cover blur-3xl" />}
          {playing && <Trailer card={card} />}
          <div className="absolute inset-0 bg-linear-to-r from-black/85 via-black/45 to-transparent" />
          <div className="absolute inset-0 bg-linear-to-t from-black/70 via-transparent to-transparent" />
        </motion.div>
      </AnimatePresence>

      <div className="absolute inset-0 flex items-end p-9 text-white">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div key={card.id} className="max-w-xl" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -8 }} transition={{ duration: 0.35 }}>
            <div className="mb-3 flex items-center gap-2.5">
              <TypeBadge card={card} />
              <span className="tnum text-[13px] font-medium text-white/85">{releaseLabel(card.release_date)}</span>
              {sub && <span className="truncate text-[13px] text-white/70">· {sub}</span>}
            </div>
            <h1 className="display text-[44px] font-extrabold leading-[1.04] drop-shadow-lg">{card.title}</h1>
            {card.why[0] ? <Reason text={card.why[0]} className="mt-3 block text-[15px] text-white/80 [&_em]:text-white" /> : card.overview && <p className="clamp-2 mt-3 text-[15px] text-white/80">{card.overview}</p>}
            <div className="mt-6 flex items-center gap-2.5">
              <Button variant="primary" size="lg" icon="play" onClick={() => openItem(card.id, 'hero')}>
                Details
              </Button>
              <Button variant="glass" size="lg" icon={card.saved ? 'check' : 'plus'} onClick={() => void quickAdd(card)}>
                {card.saved ? 'Saved' : 'Save'}
              </Button>
              {card.match_pct != null && (
                <span className="ml-2 flex items-center gap-2 text-[13px] font-medium text-white/85">
                  <MatchRing pct={card.match_pct} size={44} />
                  match
                </span>
              )}
            </div>
          </motion.div>
        </AnimatePresence>
      </div>

      {cards.length > 1 && (
        <div className="absolute bottom-5 right-7 flex gap-1.5" role="tablist" aria-label="Choose a release">
          {cards.map((c, i) => (
            <button key={c.id} type="button" role="tab" aria-selected={i === index} aria-label={c.title} onClick={() => setIndex(i)} className={cx('h-1.5 rounded-full transition-all duration-300', i === index ? 'w-7 bg-(--accent)' : 'w-1.5 bg-white/45 hover:bg-white/80')} />
          ))}
        </div>
      )}
    </section>
  );
}

export default function Home() {
  const { data, error, isPending, refetch } = useQuery({ queryKey: ['home'], queryFn: () => api<HomeData>('home') });
  const offline = useApp((s) => s.offline);
  const syncing = useApp((s) => s.sync.running);
  const navigate = useApp((s) => s.navigate);
  const menu = useCardMenu();

  if (isPending) {
    return (
      <div aria-busy="true" aria-label="Loading your releases">
        <div className="skeleton mx-(--pad) mb-10 h-[min(56vh,520px)] min-h-90 rounded-[calc(var(--radius)+6px)]!" />
        <RowSkeleton titleWidth={160} />
        <RowSkeleton titleWidth={130} />
      </div>
    );
  }
  if (error) return <ErrorState error={error} onRetry={() => void refetch()} />;

  if (data.counts.items === 0) {
    return (
      <>
        {offline && <OfflineBanner />}
        <EmptyState
          icon={syncing ? 'sync' : 'film'}
          title={syncing ? 'Finding what’s new…' : 'Nothing on the marquee yet'}
          action={
            !syncing && (
              <>
                <Button variant="primary" icon="sync" onClick={() => void api('sync')}>
                  Sync now
                </Button>
                <Button icon="plug" onClick={() => navigate('/settings/connections')}>
                  Connections and API keys
                </Button>
              </>
            )
          }
        >
          {syncing ? 'Marquee is fetching new and upcoming releases. This takes a minute the first time.' : 'Marquee needs at least a TMDB key to list movies and shows, and works best once you connect the services you use.'}
        </EmptyState>
      </>
    );
  }

  return (
    <>
      {offline && <OfflineBanner />}
      {data.hero.length > 0 && <Hero cards={data.hero} />}
      {!data.personal && (
        <div className="mx-(--pad) mb-8 flex flex-wrap items-center justify-between gap-4 rounded-card bg-accent/10 px-5 py-4">
          <div>
            <div className="font-semibold">These are the popular picks, not yours yet</div>
            <div className="text-[13px] text-subtext">Connect a service or rate a few things and every row becomes personal, with a reason for each pick.</div>
          </div>
          <div className="flex gap-2">
            <Button variant="primary" icon="dice" onClick={() => navigate('/for-you/rate')}>
              Quick Rate
            </Button>
            <Button icon="plug" onClick={() => navigate('/settings/connections')}>
              Connect services
            </Button>
          </div>
        </div>
      )}
      <WrappedCallout />
      <Spark />
      {data.rows.map((r) => (
        <Row key={r.id} id={r.id} title={r.title} subtitle={r.subtitle} cards={r.cards} onContext={menu.onContext} />
      ))}
      {data.rows.length === 0 && (
        <EmptyState icon="calendar" title="A quiet week">
          Nothing new matches right now. Try the Calendar for what's further out.
        </EmptyState>
      )}
      {menu.element}
    </>
  );
}
