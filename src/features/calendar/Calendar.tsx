// The release calendar: month, week and agenda views of what's coming, filtered to your taste,
// your lists, or everything.
import { useQuery } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { useMemo, useState } from 'react';
import { api, errorMessage, pickSave, shell } from '@/lib/api';
import { isoDay, longDate } from '@/lib/format';
import { img } from '@/lib/img';
import type { CalendarEntry } from '@/lib/types';
import { useApp, useSetting } from '@/state/app';
import { MatchRing } from '@/ui/Card';
import { Icon, mediaIcon } from '@/ui/icons';
import { Button, EmptyState, ErrorState, IconButton, OfflineBanner, PageHeader, Segmented, Skeleton, Toggle, cx } from '@/ui/primitives';
import { Modal } from '@/ui/overlays';

type View = 'month' | 'week' | 'agenda';
type Scope = 'taste' | 'lists' | 'all';

const DAY = 86_400_000;
const startOfWeek = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate() - ((d.getDay() + 6) % 7));
const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);

function range(view: View, anchor: Date): [Date, Date] {
  if (view === 'week') {
    const s = startOfWeek(anchor);
    return [s, addDays(s, 6)];
  }
  if (view === 'agenda') return [anchor, addDays(anchor, 60)];
  const first = startOfWeek(new Date(anchor.getFullYear(), anchor.getMonth(), 1));
  return [first, addDays(first, 41)];
}

const KIND_TONE: Record<string, string> = { release: 'bg-accent', premiere: 'bg-good', finale: 'bg-warn', episode: 'bg-faint' };

function EntryRow({ entry, compact }: { entry: CalendarEntry; compact?: boolean }) {
  const openItem = useApp((s) => s.openItem);
  const c = entry.card;
  if (compact) {
    return (
      <button type="button" onClick={() => openItem(c.id)} title={`${c.title} · ${entry.label}`} className="flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left text-[12px] transition-colors hover:bg-hover">
        <span className={cx('h-1.5 w-1.5 shrink-0 rounded-full', KIND_TONE[entry.kind])} />
        <Icon name={mediaIcon[c.media]} size={11} className="shrink-0 text-faint" />
        <span className={cx('truncate', entry.pinned ? 'font-semibold text-text' : 'text-subtext')}>{c.title}</span>
      </button>
    );
  }
  return (
    <button type="button" onClick={() => openItem(c.id)} className="hairline flex w-full items-center gap-3 rounded-control bg-surface p-2 text-left transition-colors hover:bg-hover">
      {c.poster ? (
        <img src={img(c.poster)} alt="" className={cx('w-10 shrink-0 rounded object-cover', c.media === 'album' ? 'h-10' : 'h-15')} loading="lazy" />
      ) : (
        <span className="grid h-15 w-10 shrink-0 place-items-center rounded bg-strong text-faint">
          <Icon name={mediaIcon[c.media]} size={16} />
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate font-semibold">{c.title}</span>
          {entry.pinned && <Icon name="pin" size={13} className="shrink-0 text-accent" />}
        </span>
        <span className="flex items-center gap-1.5 truncate text-[13px] text-subtext">
          <span className={cx('h-1.5 w-1.5 shrink-0 rounded-full', KIND_TONE[entry.kind])} />
          {entry.label}
        </span>
      </span>
      {c.match_pct != null && <MatchRing pct={c.match_pct} size={34} />}
    </button>
  );
}

function Month({ anchor, byDay, onPick }: { anchor: Date; byDay: Map<string, CalendarEntry[]>; onPick: (day: string) => void }) {
  const [first] = range('month', anchor);
  const today = isoDay(new Date());
  return (
    <div className="px-(--pad)">
      <div className="mb-1.5 grid grid-cols-7 gap-1.5 text-center text-[12px] font-semibold uppercase tracking-wider text-faint" aria-hidden="true">
        {['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'].map((d) => (
          <div key={d}>{d}</div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-1.5" role="grid" aria-label={anchor.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}>
        {Array.from({ length: 42 }, (_, i) => {
          const d = addDays(first, i);
          const key = isoDay(d);
          const entries = byDay.get(key) ?? [];
          const outside = d.getMonth() !== anchor.getMonth();
          return (
            <div key={key} role="gridcell" className={cx('hairline flex min-h-28 flex-col rounded-control p-1.5', outside ? 'bg-transparent opacity-45' : 'bg-surface', key === today && 'border-accent! ring-1 ring-accent')}>
              <button
                type="button"
                onClick={() => entries.length && onPick(key)}
                aria-label={`${longDate(key)}, ${entries.length} releases`}
                className={cx('tnum mb-1 flex h-6 w-6 items-center justify-center self-end rounded-full text-[12px] font-semibold', key === today ? 'bg-accent text-on-accent' : 'text-subtext', entries.length > 0 && 'hover:bg-hover')}
              >
                {d.getDate()}
              </button>
              <div className="space-y-px">
                {entries.slice(0, 3).map((e) => (
                  <EntryRow key={`${e.card.id}-${e.kind}`} entry={e} compact />
                ))}
                {entries.length > 3 && (
                  <button type="button" onClick={() => onPick(key)} className="tnum w-full rounded px-1 py-0.5 text-left text-[12px] font-medium text-accent hover:bg-hover">
                    +{entries.length - 3} more
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Week({ anchor, byDay }: { anchor: Date; byDay: Map<string, CalendarEntry[]> }) {
  const [first] = range('week', anchor);
  const today = isoDay(new Date());
  return (
    <div className="grid grid-cols-7 gap-2.5 px-(--pad)">
      {Array.from({ length: 7 }, (_, i) => {
        const d = addDays(first, i);
        const key = isoDay(d);
        const entries = byDay.get(key) ?? [];
        return (
          <section key={key} aria-label={longDate(key)} className="min-w-0">
            <header className={cx('mb-2 rounded-control px-2 py-1.5 text-center', key === today ? 'bg-accent text-on-accent' : 'bg-surface')}>
              <div className="text-[11px] font-semibold uppercase tracking-wider opacity-80">{d.toLocaleDateString(undefined, { weekday: 'short' })}</div>
              <div className="tnum text-lg font-bold leading-tight">{d.getDate()}</div>
            </header>
            <div className="space-y-1.5">
              {entries.map((e) => (
                <WeekCard key={`${e.card.id}-${e.kind}`} entry={e} />
              ))}
              {entries.length === 0 && <div className="rounded-control border border-dashed border-white/10 py-6 text-center text-[12px] text-faint">Nothing</div>}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function WeekCard({ entry }: { entry: CalendarEntry }) {
  const openItem = useApp((s) => s.openItem);
  const c = entry.card;
  return (
    <motion.button type="button" whileHover={{ y: -3 }} onClick={() => openItem(c.id)} className="hairline block w-full overflow-hidden rounded-control bg-surface text-left">
      {c.poster && <img src={img(c.poster)} alt="" className={cx('w-full object-cover', c.media === 'album' ? 'aspect-square' : 'aspect-2/3')} loading="lazy" />}
      <div className="p-2">
        <div className="clamp-2 text-[13px] font-semibold leading-snug">{c.title}</div>
        <div className="mt-0.5 flex items-center gap-1 truncate text-[11px] text-subtext">
          <span className={cx('h-1.5 w-1.5 shrink-0 rounded-full', KIND_TONE[entry.kind])} />
          {entry.label}
        </div>
      </div>
    </motion.button>
  );
}

function Agenda({ entries }: { entries: CalendarEntry[] }) {
  const groups = useMemo(() => {
    const m = new Map<string, CalendarEntry[]>();
    for (const e of entries) m.set(e.date, [...(m.get(e.date) ?? []), e]);
    return [...m.entries()];
  }, [entries]);
  const today = isoDay(new Date());
  return (
    <div className="mx-auto max-w-3xl space-y-6 px-(--pad)">
      {groups.map(([day, list]) => (
        <section key={day} aria-label={longDate(day)}>
          <h2 className="tnum sticky top-0 z-10 mb-2 flex items-baseline gap-2 bg-bg/90 py-1.5 font-semibold backdrop-blur">
            {longDate(day)}
            {day === today && <span className="rounded bg-accent px-1.5 py-0.5 text-[11px] font-bold text-on-accent">Today</span>}
            <span className="text-[12px] font-normal text-faint">{Math.round((new Date(`${day}T00:00`).getTime() - new Date(`${today}T00:00`).getTime()) / DAY) > 1 ? `in ${Math.round((new Date(`${day}T00:00`).getTime() - new Date(`${today}T00:00`).getTime()) / DAY)} days` : ''}</span>
          </h2>
          <div className="space-y-1.5">
            {list.map((e) => (
              <EntryRow key={`${e.card.id}-${e.kind}`} entry={e} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

export default function Calendar() {
  const [view, setView] = useSetting<View>('calendar.view', 'month');
  const [scope, setScope] = useSetting<Scope>('calendar.scope', 'taste');
  const [autoExport, setAutoExport] = useSetting('calendar.auto_export', false);
  const [anchor, setAnchor] = useState(() => new Date());
  const [picked, setPicked] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const offline = useApp((s) => s.offline);
  const toast = useApp((s) => s.toast);
  const [from, to] = range(view, anchor);
  const { data, error, isPending, refetch } = useQuery({ queryKey: ['calendar', isoDay(from), isoDay(to), scope], queryFn: () => api<CalendarEntry[]>('calendar', { from: isoDay(from), to: isoDay(to), scope }) });
  const byDay = useMemo(() => {
    const m = new Map<string, CalendarEntry[]>();
    for (const e of data ?? []) m.set(e.date, [...(m.get(e.date) ?? []), e]);
    return m;
  }, [data]);

  const step = (dir: 1 | -1) => setAnchor((a) => (view === 'month' ? new Date(a.getFullYear(), a.getMonth() + dir, 1) : addDays(a, dir * (view === 'week' ? 7 : 30))));
  const heading =
    view === 'month'
      ? anchor.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
      : view === 'week'
        ? `${from.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })} – ${to.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })}`
        : `Next 60 days from ${from.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}`;

  const exportIcs = async () => {
    const path = await pickSave('marquee-releases.ics', [{ name: 'Calendar', extensions: ['ics'] }]);
    if (!path) return;
    try {
      await api('export.ics', { path, scope: scope === 'taste' ? 'taste' : scope });
      toast('Calendar exported. Open the .ics file in any calendar app.', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };

  return (
    <>
      <PageHeader title="Release Calendar" subtitle="Follow a movie, show, game, artist or franchise and its releases are pinned here">
        <Segmented
          label="What to show"
          value={scope}
          onChange={setScope}
          options={[
            { value: 'taste', label: 'My taste' },
            { value: 'lists', label: 'My lists' },
            { value: 'all', label: 'Everything' },
          ]}
        />
        <Segmented
          label="View"
          value={view}
          onChange={setView}
          options={[
            { value: 'month', label: 'Month' },
            { value: 'week', label: 'Week' },
            { value: 'agenda', label: 'Agenda' },
          ]}
        />
        <Button icon="share" onClick={() => setExporting(true)}>
          Export
        </Button>
      </PageHeader>
      {offline && <OfflineBanner />}
      <div className="mb-4 flex items-center gap-2 px-(--pad)">
        <IconButton icon="chevronLeft" label="Earlier" onClick={() => step(-1)} />
        <IconButton icon="chevronRight" label="Later" onClick={() => step(1)} />
        <Button size="sm" onClick={() => setAnchor(new Date())}>
          Today
        </Button>
        <h2 className="tnum ml-2 text-lg font-semibold" aria-live="polite">
          {heading}
        </h2>
        <div className="flex-1" />
        <div className="hidden items-center gap-3 text-[12px] text-faint lg:flex">
          {[
            ['release', 'Release'],
            ['premiere', 'Premiere'],
            ['finale', 'Finale'],
            ['episode', 'Episode'],
          ].map(([k, label]) => (
            <span key={k} className="flex items-center gap-1.5">
              <span className={cx('h-2 w-2 rounded-full', KIND_TONE[k])} />
              {label}
            </span>
          ))}
        </div>
      </div>
      {isPending ? (
        <div className="px-(--pad)" aria-busy="true">
          <Skeleton className="h-[60vh] w-full rounded-card!" />
        </div>
      ) : error ? (
        <ErrorState error={error} onRetry={() => void refetch()} />
      ) : data.length === 0 ? (
        <EmptyState icon="calendar" title="Nothing in this stretch" action={scope !== 'all' && <Button onClick={() => setScope('all')}>Show everything</Button>}>
          {scope === 'lists' ? 'Follow or save something and its release date shows up here.' : scope === 'taste' ? 'No strong matches in this window. Try a later month, or show everything.' : 'No releases are known for these dates yet.'}
        </EmptyState>
      ) : view === 'month' ? (
        <Month anchor={anchor} byDay={byDay} onPick={setPicked} />
      ) : view === 'week' ? (
        <Week anchor={anchor} byDay={byDay} />
      ) : (
        <Agenda entries={data} />
      )}

      <Modal open={!!picked} onClose={() => setPicked(null)} title={picked ? longDate(picked) : ''} width={480}>
        <div className="space-y-1.5">
          {(picked ? (byDay.get(picked) ?? []) : []).map((e) => (
            <div key={`${e.card.id}-${e.kind}`} onClick={() => setPicked(null)}>
              <EntryRow entry={e} />
            </div>
          ))}
        </div>
      </Modal>

      <Modal open={exporting} onClose={() => setExporting(false)} title="Export the calendar" width={520}>
        <p className="text-subtext">Save your releases as an .ics file to open in Outlook, Google Calendar or any other calendar app.</p>
        <div className="mt-4">
          <Button variant="primary" icon="download" onClick={() => void exportIcs()}>
            Save an .ics file…
          </Button>
        </div>
        <div className="mt-5 border-t border-white/8 pt-3">
          <Toggle
            checked={autoExport}
            onChange={(v) => {
              setAutoExport(v);
              if (v)
                void api('export.calendar_file')
                  .then((r) => toast(`Calendar file kept at ${r.path}`, 'good'))
                  .catch((e) => toast(errorMessage(e), 'bad'));
            }}
            label="Keep a calendar file up to date"
            description="Marquee rewrites marquee.ics in its data folder after every sync, so a calendar app that subscribes to that file stays current."
          />
          {autoExport && (
            <Button size="sm" className="mt-2" icon="external" onClick={() => void shell('open_folder', { which: 'data' })}>
              Show the file
            </Button>
          )}
        </div>
      </Modal>
    </>
  );
}
