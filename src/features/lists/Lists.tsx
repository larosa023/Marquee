// Marquee's own lists: the four built-in ones plus any number of custom, mixed-media lists.
// A list can mirror a list on a service; each entry then shows whether it has reached that service.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Reorder, motion, useDragControls } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api, errorMessage, pickSave, shell } from '@/lib/api';
import { ago, credit, noun, releaseLabel, typeBadge } from '@/lib/format';
import { img } from '@/lib/img';
import { MEDIA, type ListEntry, type ListInfo, type Media } from '@/lib/types';
import { EMPTY, useApp } from '@/state/app';
import { refreshFeeds } from '@/ui/actions';
import { Art, MatchRing } from '@/ui/Card';
import { Icon, mediaIcon, type IconName } from '@/ui/icons';
import { MenuButton, Modal, type MenuItem } from '@/ui/overlays';
import { Badge, Button, Chip, EmptyState, ErrorState, IconButton, Input, PageHeader, Select, Skeleton, cx } from '@/ui/primitives';

const LIST_ICON: Record<string, IconName> = { watchlist: 'eye', 'play-later': 'gamepad', 'listen-later': 'music', favorites: 'heart' };

function mirrorName(mirror: string | null, playlists: any[]): string | null {
  if (!mirror) return null;
  if (mirror === 'trakt:watchlist') return 'Trakt watchlist';
  if (mirror === 'steam:wishlist') return 'Steam wishlist';
  if (mirror.startsWith('spotify:playlist:')) return `Spotify: ${playlists.find((p) => `spotify:playlist:${p.id}` === mirror)?.name ?? 'playlist'}`;
  return mirror;
}

const SYNC: Record<ListEntry['sync_state'], { label: string; tone: 'neutral' | 'good' | 'warn' | 'bad'; hint: string }> = {
  local: { label: 'Marquee only', tone: 'neutral', hint: 'Saved in Marquee' },
  synced: { label: 'Synced', tone: 'good', hint: 'Also on the mirrored service' },
  pending: { label: 'Pending', tone: 'warn', hint: 'Waiting for the service to confirm it' },
  failed: { label: 'Not synced', tone: 'bad', hint: "The service didn't accept it. It's still saved here." },
};

// ------------------------------------------------------------------ overview

function Covers({ list }: { list: ListInfo }) {
  const covers = list.covers.slice(0, 4);
  if (covers.length === 0) {
    return (
      <div className="grid aspect-video place-items-center bg-linear-to-br from-strong to-surface text-faint">
        <Icon name={LIST_ICON[list.slug] ?? 'list'} size={34} />
      </div>
    );
  }
  return <div className="grid aspect-video grid-cols-4 overflow-hidden bg-surface">{Array.from({ length: 4 }, (_, i) => (covers[i] ? <img key={i} src={img(covers[i])} alt="" className="h-full w-full object-cover" loading="lazy" /> : <div key={i} className="bg-strong/60" />))}</div>;
}

function Overview() {
  const navigate = useApp((s) => s.navigate);
  const toast = useApp((s) => s.toast);
  const playlists: any[] = useApp((s) => s.boot?.sources.find((x) => x.id === 'spotify')?.config?.playlists ?? EMPTY);
  const qc = useQueryClient();
  const { data, error, refetch } = useQuery({ queryKey: ['lists'], queryFn: () => api<ListInfo[]>('lists') });
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');

  const create = async () => {
    if (!name.trim()) return;
    try {
      const { id } = await api<{ id: number }>('list.create', { name: name.trim() });
      setCreating(false);
      setName('');
      await Promise.all([qc.invalidateQueries({ queryKey: ['lists'] }), useApp.getState().refreshBoot()]);
      navigate(`/lists/${id}`);
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };

  return (
    <>
      <PageHeader title="Lists" subtitle="Everything you've saved, in one place. Mix movies, shows, games and albums however you like.">
        <Button variant="primary" icon="plus" onClick={() => setCreating(true)}>
          New list
        </Button>
      </PageHeader>
      {error ? (
        <ErrorState error={error} onRetry={() => void refetch()} />
      ) : !data ? (
        <div className="grid gap-5 px-(--pad)" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))' }} aria-busy="true">
          {Array.from({ length: 4 }, (_, i) => (
            <Skeleton key={i} className="aspect-4/3 w-full rounded-card!" />
          ))}
        </div>
      ) : (
        <motion.ul initial="hidden" animate="show" variants={{ show: { transition: { staggerChildren: 0.04 } } }} className="grid gap-5 px-(--pad)" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))' }}>
          {data.map((l) => (
            <motion.li key={l.id} variants={{ hidden: { opacity: 0, y: 12 }, show: { opacity: 1, y: 0 } }}>
              <button type="button" onClick={() => navigate(`/lists/${l.id}`)} className="hairline group block w-full overflow-hidden rounded-card bg-surface text-left transition-[transform,box-shadow] duration-200 hover:-translate-y-1 hover:shadow-[0_14px_40px_rgb(0_0_0/0.4)]">
                <Covers list={l} />
                <div className="flex items-center gap-3 p-4">
                  <span className="grid h-9 w-9 shrink-0 place-items-center rounded-control bg-accent/12 text-accent">
                    <Icon name={LIST_ICON[l.slug] ?? 'list'} size={18} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[15px] font-semibold">{l.name}</span>
                    <span className="tnum block truncate text-[13px] text-subtext">
                      {l.count} {noun(l.count, 'title')}
                      {l.mirror ? ` · mirrors ${mirrorName(l.mirror, playlists)}` : ''}
                    </span>
                  </span>
                  <Icon name="chevronRight" size={18} className="text-faint transition-transform group-hover:translate-x-0.5" />
                </div>
              </button>
            </motion.li>
          ))}
        </motion.ul>
      )}
      <Modal
        open={creating}
        onClose={() => setCreating(false)}
        title="New list"
        width={420}
        footer={
          <>
            <Button onClick={() => setCreating(false)}>Cancel</Button>
            <Button variant="primary" disabled={!name.trim()} onClick={() => void create()}>
              Create
            </Button>
          </>
        }
      >
        <label className="block">
          <span className="mb-1.5 block text-[13px] text-subtext">Name</span>
          <Input data-autofocus value={name} maxLength={80} placeholder="Rainy Sunday" onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && void create()} />
        </label>
      </Modal>
    </>
  );
}

// ------------------------------------------------------------- share a list

/** Draws the list as a 1200×630 card (the size chat apps and social sites preview well). */
async function drawShareCard(canvas: HTMLCanvasElement, info: ListInfo, entries: ListEntry[], accent: string, withArt: boolean) {
  const W = 1200;
  const H = 630;
  canvas.width = W;
  canvas.height = H;
  const c = canvas.getContext('2d')!;
  const bg = c.createLinearGradient(0, 0, W, H);
  bg.addColorStop(0, '#0d0d10');
  bg.addColorStop(1, '#17171c');
  c.fillStyle = bg;
  c.fillRect(0, 0, W, H);
  const glow = c.createRadialGradient(180, 80, 0, 180, 80, 620);
  glow.addColorStop(0, `${accent}40`);
  glow.addColorStop(1, `${accent}00`);
  c.fillStyle = glow;
  c.fillRect(0, 0, W, H);

  // Marquee bulbs along the top edge.
  c.fillStyle = accent;
  for (let x = 56; x < W - 40; x += 44) {
    c.globalAlpha = 0.35 + 0.65 * (((x / 44) | 0) % 2);
    c.beginPath();
    c.arc(x, 34, 5, 0, Math.PI * 2);
    c.fill();
  }
  c.globalAlpha = 1;

  const font = "'Inter Variable', 'Segoe UI Variable Display', 'Segoe UI', sans-serif";
  c.fillStyle = '#f6f2ea';
  c.font = `800 60px ${font}`;
  c.textBaseline = 'alphabetic';
  let title = info.name;
  while (c.measureText(title).width > W - 112 && title.length > 4) title = `${title.slice(0, -2).trimEnd()}…`;
  c.fillText(title, 56, 128);
  c.fillStyle = '#b9b4aa';
  c.font = `500 26px ${font}`;
  const kinds = MEDIA.map((m) => [m, entries.filter((e) => e.media === m.id).length] as const).filter(([, n]) => n > 0);
  c.fillText(`${entries.length} ${noun(entries.length, 'title')}${kinds.length > 1 ? ` · ${kinds.map(([m, n]) => `${n} ${noun(n, { movie: 'movie', tv: 'show', game: 'game', album: 'album' }[m.id])}`).join(' · ')}` : ''}`, 56, 172);

  const shown = entries.slice(0, 7);
  const pw = 140;
  const ph = 210;
  const gap = 18;
  const top = 222;
  const rounded = (x: number, y: number, w: number, h: number, r: number) => {
    c.beginPath();
    c.roundRect(x, y, w, h, r);
  };
  const posters = await Promise.all(
    shown.map(
      (e) =>
        new Promise<HTMLImageElement | null>((resolve) => {
          const src = withArt ? img(e.poster) : undefined;
          if (!src) return resolve(null);
          const im = new Image();
          im.crossOrigin = 'anonymous';
          im.onload = () => resolve(im);
          im.onerror = () => resolve(null);
          im.src = src;
        }),
    ),
  );
  shown.forEach((e, i) => {
    const x = 56 + i * (pw + gap);
    c.save();
    rounded(x, top, pw, ph, 12);
    c.clip();
    c.fillStyle = '#28282e';
    c.fillRect(x, top, pw, ph);
    const im = posters[i];
    if (im) {
      // Cover the tile without stretching.
      const scale = Math.max(pw / im.width, ph / im.height);
      const dw = im.width * scale;
      const dh = im.height * scale;
      c.drawImage(im, x + (pw - dw) / 2, top + (ph - dh) / 2, dw, dh);
    } else {
      c.fillStyle = '#b9b4aa';
      c.font = `600 15px ${font}`;
      wrap(c, e.title, pw - 20)
        .slice(0, 5)
        .forEach((line, n) => c.fillText(line, x + 10, top + 30 + n * 20));
    }
    c.restore();
  });
  if (entries.length > shown.length) {
    const x = 56 + shown.length * (pw + gap);
    c.fillStyle = '#f6f2ea';
    c.font = `700 30px ${font}`;
    c.fillText(`+${entries.length - shown.length}`, x - 6 > W - 110 ? W - 110 : x, top + ph / 2 + 10);
  }

  // The first few titles, written out, so the card still says something without the art.
  c.font = `500 20px ${font}`;
  c.fillStyle = '#b9b4aa';
  const names = entries
    .slice(0, 12)
    .map((e) => e.title)
    .join('  ·  ');
  wrap(c, names, W - 112)
    .slice(0, 2)
    .forEach((line, n) => c.fillText(line, 56, top + ph + 52 + n * 30));

  c.fillStyle = accent;
  c.font = `700 22px ${font}`;
  c.fillText('MARQUEE', 56, H - 40);
  c.fillStyle = '#898680';
  c.font = `500 18px ${font}`;
  c.fillText(new Date().toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }), 180, H - 40);
}

function wrap(c: CanvasRenderingContext2D, text: string, width: number): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(' ')) {
    const next = line ? `${line} ${word}` : word;
    if (c.measureText(next).width > width && line) {
      lines.push(line);
      line = word;
    } else line = next;
  }
  if (line) lines.push(line);
  return lines;
}

function ShareModal({ info, entries, onClose }: { info: ListInfo; entries: ListEntry[]; onClose: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const toast = useApp((s) => s.toast);
  const [markdown, setMarkdown] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const safeName = info.name.replace(/[^\w -]+/g, '').trim() || 'list';

  useEffect(() => {
    let live = true;
    const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#F2C14E';
    const hex = /^#[0-9a-f]{6}$/i.test(accent) ? accent : '#F2C14E';
    const el = canvas.current;
    if (!el) return;
    void (async () => {
      await drawShareCard(el, info, entries, hex, true);
      try {
        // If any poster came without permission to be read back, redraw without art so the card can still be saved.
        el.toDataURL();
      } catch {
        await drawShareCard(el, info, entries, hex, false);
      }
      if (live) setReady(true);
    })();
    void api<string>('list.markdown', { id: info.id })
      .then((md) => live && setMarkdown(md))
      .catch(() => live && setMarkdown(''));
    return () => {
      live = false;
    };
  }, [info, entries]);

  const blob = () => new Promise<Blob>((resolve, reject) => canvas.current?.toBlob((b) => (b ? resolve(b) : reject(new Error("The image couldn't be made."))), 'image/png'));
  const copyImage = async () => {
    try {
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': await blob() })]);
      toast('Image copied', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  const saveImage = async () => {
    try {
      const path = await pickSave(`${safeName}.png`, [{ name: 'PNG image', extensions: ['png'] }]);
      if (!path) return;
      await shell('save_file', { path, contents: canvas.current!.toDataURL('image/png').split(',')[1], base64: true });
      toast('Image saved', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  const copyMarkdown = async () => {
    try {
      await navigator.clipboard.writeText(markdown ?? '');
      toast('Markdown copied', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  const saveMarkdown = async () => {
    try {
      const path = await pickSave(`${safeName}.md`, [{ name: 'Markdown', extensions: ['md'] }]);
      if (!path) return;
      await shell('save_file', { path, contents: markdown ?? '', base64: false });
      toast('Markdown saved', 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };

  return (
    <Modal open onClose={onClose} title={`Share ${info.name}`} width={760}>
      <div className="relative overflow-hidden rounded-card bg-surface">
        <canvas ref={canvas} className="block h-auto w-full" role="img" aria-label={`A card showing ${info.name}: ${entries.length} ${noun(entries.length, 'title')}`} />
        {!ready && <Skeleton className="absolute inset-0 rounded-none!" />}
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
        <Button variant="primary" icon="copy" disabled={!ready} onClick={() => void copyImage()}>
          Copy image
        </Button>
        <Button icon="download" disabled={!ready} onClick={() => void saveImage()}>
          Save image…
        </Button>
      </div>
      <h3 className="mb-2 mt-6 text-[15px] font-semibold">As Markdown</h3>
      {markdown == null ? <Skeleton className="h-32 w-full" /> : <pre className="hairline max-h-48 overflow-auto whitespace-pre-wrap rounded-control bg-surface p-3 text-[12px] leading-relaxed text-subtext">{markdown || 'This list is empty.'}</pre>}
      <div className="mt-3 flex flex-wrap gap-2">
        <Button icon="copy" disabled={!markdown} onClick={() => void copyMarkdown()}>
          Copy Markdown
        </Button>
        <Button icon="download" disabled={!markdown} onClick={() => void saveMarkdown()}>
          Save as .md…
        </Button>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- one list

type SortKey = 'custom' | 'added' | 'title' | 'release' | 'match';

function EntryRow({ entry, index, total, listId, mirrored, selecting, selected, onToggle, canReorder, onMove }: { entry: ListEntry; index: number; total: number; listId: number; mirrored: boolean; selecting: boolean; selected: boolean; onToggle: () => void; canReorder: boolean; onMove: (dir: -1 | 1) => void }) {
  const openItem = useApp((s) => s.openItem);
  const controls = useDragControls();
  const sync = SYNC[entry.sync_state];
  const sub = credit(entry);
  const body = (
    <div className={cx('hairline group flex items-center gap-3 rounded-card bg-surface p-2 pr-3 transition-colors hover:bg-hover', selected && 'border-accent! bg-accent/8!')}>
      {selecting ? (
        <button type="button" role="checkbox" aria-checked={selected} aria-label={`Select ${entry.title}`} onClick={onToggle} className={cx('ml-1 grid h-6 w-6 shrink-0 place-items-center rounded-md border-2 transition-colors', selected ? 'border-accent bg-accent text-on-accent' : 'border-faint text-transparent')}>
          <Icon name="check" size={14} />
        </button>
      ) : (
        canReorder && (
          <span
            className="grid h-9 w-6 shrink-0 cursor-grab touch-none place-items-center text-faint hover:text-text active:cursor-grabbing"
            onPointerDown={(e) => controls.start(e)}
            role="button"
            tabIndex={0}
            aria-label={`Reorder ${entry.title}. Position ${index + 1} of ${total}. Use the up and down arrow keys.`}
            onKeyDown={(e) => {
              if (e.key === 'ArrowUp') {
                e.preventDefault();
                onMove(-1);
              }
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                onMove(1);
              }
            }}
          >
            <Icon name="grip" size={16} />
          </span>
        )
      )}
      <button type="button" className="flex min-w-0 flex-1 items-center gap-3.5 text-left" onClick={() => (selecting ? onToggle() : openItem(entry.id, `list-${listId}`))} aria-label={selecting ? undefined : `Open ${entry.title}`}>
        <motion.span layoutId={`art-list-${listId}-${entry.id}`} className="block h-18 w-12 shrink-0 overflow-hidden rounded-md bg-strong">
          <Art card={entry} className={cx('w-full', entry.media === 'album' ? 'h-full' : 'h-full')} />
        </motion.span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[15px] font-semibold">{entry.title}</span>
          <span className="tnum mt-0.5 flex items-center gap-1.5 truncate text-[13px] text-subtext">
            <Icon name={mediaIcon[entry.media]} size={13} className="shrink-0 text-faint" />
            <span>{typeBadge(entry)}</span>
            <span className="text-faint">·</span>
            <span>{releaseLabel(entry.release_date)}</span>
            {sub && (
              <>
                <span className="text-faint">·</span>
                <span className="truncate">{sub}</span>
              </>
            )}
          </span>
          <span className="mt-0.5 block text-[12px] text-faint">Added {ago(entry.added_at)}</span>
        </span>
      </button>
      {entry.done && <Badge tone="good">Done</Badge>}
      {(mirrored || entry.sync_state !== 'local') && (
        <span title={sync.hint}>
          <Badge tone={sync.tone}>{sync.label}</Badge>
        </span>
      )}
      {entry.match_pct != null && <MatchRing pct={entry.match_pct} size={36} />}
    </div>
  );
  if (!canReorder || selecting) return <li>{body}</li>;
  return (
    <Reorder.Item value={entry.id} dragListener={false} dragControls={controls} as="li" className="relative" whileDrag={{ scale: 1.015, zIndex: 5, boxShadow: '0 16px 40px rgb(0 0 0 / 0.5)' }}>
      {body}
    </Reorder.Item>
  );
}

function ListView({ listId }: { listId: number }) {
  const navigate = useApp((s) => s.navigate);
  const toast = useApp((s) => s.toast);
  const sources = useApp((s) => s.boot?.sources ?? EMPTY);
  const allLists = useApp((s) => s.boot?.lists ?? EMPTY);
  const qc = useQueryClient();
  const { data, error, refetch } = useQuery({ queryKey: ['list', listId], queryFn: () => api<{ info: ListInfo; entries: ListEntry[] }>('list', { id: listId }) });
  const [sort, setSort] = useState<SortKey>('custom');
  const [media, setMedia] = useState<Media | 'all'>('all');
  const [text, setText] = useState('');
  const [hideDone, setHideDone] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [order, setOrder] = useState<number[] | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [sharing, setSharing] = useState(false);

  useEffect(() => {
    setOrder(null);
    setSelected(new Set());
    setSelecting(false);
  }, [listId]);
  // Whenever fresh data arrives, drop the optimistic order.
  useEffect(() => setOrder(null), [data]);

  const connected = (id: string) => sources.some((s) => s.id === id && s.status !== 'disconnected');
  const playlists: any[] = sources.find((s) => s.id === 'spotify')?.config?.playlists ?? [];
  const entries = useMemo(() => data?.entries ?? [], [data]);
  const byId = useMemo(() => new Map(entries.map((e) => [e.id, e])), [entries]);
  const custom = sort === 'custom' && media === 'all' && !text.trim() && !hideDone;

  const visible = useMemo(() => {
    let out = order && custom ? (order.map((id) => byId.get(id)).filter(Boolean) as ListEntry[]) : [...entries];
    if (media !== 'all') out = out.filter((e) => e.media === media);
    if (hideDone) out = out.filter((e) => !e.done);
    const q = text.trim().toLowerCase();
    if (q)
      out = out.filter(
        (e) =>
          e.title.toLowerCase().includes(q) ||
          String(e.meta?.artist ?? '')
            .toLowerCase()
            .includes(q),
      );
    const cmp: Record<Exclude<SortKey, 'custom'>, (a: ListEntry, b: ListEntry) => number> = {
      added: (a, b) => b.added_at.localeCompare(a.added_at),
      title: (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' }),
      release: (a, b) => (a.release_date ?? '9999').localeCompare(b.release_date ?? '9999'),
      match: (a, b) => (b.match_pct ?? -1) - (a.match_pct ?? -1),
    };
    if (sort !== 'custom') out.sort(cmp[sort]);
    return out;
  }, [entries, order, custom, byId, media, hideDone, text, sort]);

  const refresh = async () => {
    refreshFeeds(qc);
    await useApp.getState().refreshBoot();
  };
  const saveOrder = async (ids: number[]) => {
    try {
      await api('list.reorder', { id: listId, item_ids: ids });
      void qc.invalidateQueries({ queryKey: ['lists'] });
    } catch (e) {
      toast(errorMessage(e), 'bad');
      setOrder(null);
    }
  };
  const move = (id: number, dir: -1 | 1) => {
    const ids = order ?? entries.map((e) => e.id);
    const i = ids.indexOf(id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= ids.length) return;
    const next = [...ids];
    [next[i], next[j]] = [next[j], next[i]];
    setOrder(next);
    void saveOrder(next);
  };
  const bulk = async (action: 'remove' | 'copy' | 'move' | 'done', to?: ListInfo) => {
    const ids = [...selected];
    if (ids.length === 0) return;
    try {
      await api('list.bulk', { id: listId, item_ids: ids, action, to: to?.id });
      const n = `${ids.length} ${noun(ids.length, 'title')}`;
      toast(action === 'remove' ? `Removed ${n}` : action === 'done' ? `Marked ${n} as done` : `${action === 'move' ? 'Moved' : 'Copied'} ${n} to ${to?.name}`, 'good');
      setSelected(new Set());
      setSelecting(false);
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    await refresh();
  };
  const setMirror = async (mirror: string | null) => {
    try {
      await api('list.mirror', { id: listId, mirror });
      toast(mirror ? `This list now mirrors ${mirrorName(mirror, playlists)}. It will fill in on the next sync.` : 'Mirroring turned off. Everything stays in Marquee.', 'good');
      if (mirror) void api('sync', { source: mirror.split(':')[0] }).catch(() => {});
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    await refresh();
  };

  if (error) return <ErrorState error={error} onRetry={() => void refetch()} />;
  if (!data) {
    return (
      <div className="px-(--pad)" aria-busy="true">
        <Skeleton className="mb-6 h-9 w-72" />
        {Array.from({ length: 7 }, (_, i) => (
          <Skeleton key={i} className="mb-2 h-22 w-full rounded-card!" />
        ))}
      </div>
    );
  }

  const info = data.info;
  const mirrorItems: MenuItem[] = [
    { id: 'none', label: 'Marquee only', icon: 'bookmark', checked: !info.mirror, run: () => void setMirror(null) },
    { id: 'sep', label: '', separator: true },
    { id: 'trakt', label: 'Trakt watchlist', icon: 'cloud', hint: connected('trakt') ? 'Movies and shows, both ways' : 'Connect Trakt in Settings first', disabled: !connected('trakt'), checked: info.mirror === 'trakt:watchlist', run: () => void setMirror('trakt:watchlist') },
    { id: 'steam', label: 'Steam wishlist', icon: 'cloud', hint: connected('steam') ? 'Reads your wishlist; adding opens the store page' : 'Connect Steam in Settings first', disabled: !connected('steam'), checked: info.mirror === 'steam:wishlist', run: () => void setMirror('steam:wishlist') },
    ...(connected('spotify')
      ? [{ id: 'sp-h', label: 'Spotify playlists', heading: true } as MenuItem, ...playlists.slice(0, 12).map<MenuItem>((p) => ({ id: `sp-${p.id}`, label: p.name, icon: 'music', checked: info.mirror === `spotify:playlist:${p.id}`, run: () => void setMirror(`spotify:playlist:${p.id}`) }))]
      : [{ id: 'spotify', label: 'Spotify playlist', icon: 'cloud', hint: 'Connect Spotify in Settings first', disabled: true } as MenuItem]),
  ];
  const others = allLists.filter((l) => l.id !== listId);
  const moreItems: MenuItem[] = [
    { id: 'share', label: 'Share…', icon: 'share', hint: 'As an image card or Markdown', disabled: entries.length === 0, run: () => setSharing(true) },
    { id: 'select', label: selecting ? 'Stop selecting' : 'Select titles', icon: 'check', disabled: entries.length === 0, run: () => (setSelecting(!selecting), setSelected(new Set())) },
    ...(info.kind === 'custom' ? [{ id: 'sep', label: '', separator: true } as MenuItem, { id: 'rename', label: 'Rename', icon: 'tag', run: () => setRenaming(info.name) } as MenuItem, { id: 'delete', label: 'Delete list', icon: 'trash', danger: true, run: () => setDeleting(true) } as MenuItem] : []),
  ];
  const counts = MEDIA.map((m) => ({ ...m, n: entries.filter((e) => e.media === m.id).length })).filter((m) => m.n > 0);
  const waiting = entries.filter((e) => e.sync_state === 'pending').length;

  return (
    <>
      <div className="px-(--pad)">
        <button type="button" className="mb-2 inline-flex items-center gap-1.5 rounded-control text-[13px] text-subtext hover:text-text" onClick={() => navigate('/lists')}>
          <Icon name="arrowLeft" size={15} /> All lists
        </button>
      </div>
      <PageHeader
        title={info.name}
        subtitle={
          <span className="tnum">
            {info.count} {noun(info.count, 'title')}
            {info.mirror ? ` · mirrors ${mirrorName(info.mirror, playlists)}` : ''}
            {waiting > 0 ? ` · ${waiting} waiting to sync` : ''}
          </span>
        }
      >
        <MenuButton items={mirrorItems} label="Mirror this list">
          {(open, isOpen) => (
            <Button icon={info.mirror ? 'cloud' : 'link'} onClick={open} aria-haspopup="menu" aria-expanded={isOpen}>
              {info.mirror ? 'Mirrored' : 'Mirror…'}
            </Button>
          )}
        </MenuButton>
        <MenuButton items={moreItems} label="List actions">
          {(open, isOpen) => <IconButton icon="more" label="List actions" className="hairline bg-strong" onClick={open} aria-haspopup="menu" aria-expanded={isOpen} />}
        </MenuButton>
      </PageHeader>

      {entries.length === 0 ? (
        <EmptyState
          icon={LIST_ICON[info.slug] ?? 'list'}
          title="Nothing here yet"
          action={
            <Button variant="primary" icon="sparkles" onClick={() => navigate('/for-you')}>
              Find something
            </Button>
          }
        >
          Use the + on any card, or "Add to…" on a title's page.{info.mirror ? ' Titles from the mirrored list arrive on the next sync.' : ''}
        </EmptyState>
      ) : (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-2 px-(--pad)">
            <Input icon="search" className="w-56" placeholder="Filter this list" aria-label="Filter this list" value={text} onChange={(e) => setText(e.target.value)} />
            <Select
              label="Sort by"
              className="w-44"
              value={sort}
              onChange={setSort}
              options={[
                { value: 'custom', label: 'My order' },
                { value: 'added', label: 'Recently added' },
                { value: 'title', label: 'Title' },
                { value: 'release', label: 'Release date' },
                { value: 'match', label: 'Best match' },
              ]}
            />
            {counts.length > 1 && (
              <div className="flex flex-wrap gap-1.5" role="group" aria-label="Filter by type">
                <Chip selected={media === 'all'} onClick={() => setMedia('all')}>
                  All
                </Chip>
                {counts.map((m) => (
                  <Chip key={m.id} selected={media === m.id} icon={mediaIcon[m.id]} onClick={() => setMedia(media === m.id ? 'all' : m.id)}>
                    {m.plural} <span className="tnum opacity-70">{m.n}</span>
                  </Chip>
                ))}
              </div>
            )}
            <Chip selected={hideDone} onClick={() => setHideDone(!hideDone)} icon="eyeOff">
              Hide finished
            </Chip>
          </div>

          {selecting && (
            <motion.div initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0 }} className="sticky top-0 z-10 mx-(--pad) mb-3 flex flex-wrap items-center gap-2 rounded-card p-2 panel" role="toolbar" aria-label="Bulk actions">
              <span className="tnum px-2 font-medium" aria-live="polite">
                {selected.size} selected
              </span>
              <Button size="sm" onClick={() => setSelected(selected.size === visible.length ? new Set() : new Set(visible.map((e) => e.id)))}>
                {selected.size === visible.length ? 'Select none' : 'Select all'}
              </Button>
              <span className="flex-1" />
              <MenuButton label="Copy to list" items={others.map((l) => ({ id: `c-${l.id}`, label: l.name, icon: 'bookmark', run: () => void bulk('copy', l) }))}>
                {(open) => (
                  <Button size="sm" icon="copy" disabled={selected.size === 0 || others.length === 0} onClick={open}>
                    Copy to…
                  </Button>
                )}
              </MenuButton>
              <MenuButton label="Move to list" items={others.map((l) => ({ id: `m-${l.id}`, label: l.name, icon: 'bookmark', run: () => void bulk('move', l) }))}>
                {(open) => (
                  <Button size="sm" icon="external" disabled={selected.size === 0 || others.length === 0} onClick={open}>
                    Move to…
                  </Button>
                )}
              </MenuButton>
              <Button size="sm" icon="check" disabled={selected.size === 0} onClick={() => void bulk('done')}>
                Mark done
              </Button>
              <Button size="sm" variant="danger" icon="trash" disabled={selected.size === 0} onClick={() => void bulk('remove')}>
                Remove
              </Button>
              <IconButton icon="x" label="Stop selecting" onClick={() => (setSelecting(false), setSelected(new Set()))} />
            </motion.div>
          )}

          {visible.length === 0 ? (
            <EmptyState icon="filter" title="Nothing matches" action={<Button onClick={() => (setText(''), setMedia('all'), setHideDone(false))}>Clear filters</Button>}>
              Try a different filter.
            </EmptyState>
          ) : custom && !selecting ? (
            <Reorder.Group as="ul" axis="y" values={visible.map((e) => e.id)} onReorder={(ids) => setOrder(ids as number[])} className="space-y-2 px-(--pad)" onPointerUp={() => order && void saveOrder(order)} aria-label={`${info.name}, in your order. Drag the handle or use the arrow keys on it to reorder.`}>
              {visible.map((e, i) => (
                <EntryRow key={e.id} entry={e} index={i} total={visible.length} listId={listId} mirrored={!!info.mirror} selecting={false} selected={false} onToggle={() => {}} canReorder onMove={(dir) => move(e.id, dir)} />
              ))}
            </Reorder.Group>
          ) : (
            <ul className="space-y-2 px-(--pad)" aria-label={info.name}>
              {visible.map((e, i) => (
                <EntryRow
                  key={e.id}
                  entry={e}
                  index={i}
                  total={visible.length}
                  listId={listId}
                  mirrored={!!info.mirror}
                  selecting={selecting}
                  selected={selected.has(e.id)}
                  onToggle={() => {
                    const next = new Set(selected);
                    if (next.has(e.id)) next.delete(e.id);
                    else next.add(e.id);
                    setSelected(next);
                  }}
                  canReorder={false}
                  onMove={() => {}}
                />
              ))}
            </ul>
          )}
          {!custom && !selecting && sort === 'custom' && <p className="mt-3 px-(--pad) text-[12px] text-faint">Clear the filters to drag titles into a new order.</p>}
        </>
      )}

      <Modal
        open={renaming != null}
        onClose={() => setRenaming(null)}
        title="Rename list"
        width={420}
        footer={
          <>
            <Button onClick={() => setRenaming(null)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!renaming?.trim()}
              onClick={() =>
                void api('list.rename', { id: listId, name: renaming!.trim() })
                  .then(() => (setRenaming(null), refresh()))
                  .catch((e) => toast(errorMessage(e), 'bad'))
              }
            >
              Rename
            </Button>
          </>
        }
      >
        <Input data-autofocus aria-label="List name" value={renaming ?? ''} maxLength={80} onChange={(e) => setRenaming(e.target.value)} />
      </Modal>
      <Modal
        open={deleting}
        onClose={() => setDeleting(false)}
        title={`Delete ${info.name}?`}
        width={440}
        footer={
          <>
            <Button onClick={() => setDeleting(false)}>Keep it</Button>
            <Button
              variant="danger"
              icon="trash"
              onClick={() =>
                void api('list.delete', { id: listId })
                  .then(async () => {
                    setDeleting(false);
                    await refresh();
                    navigate('/lists');
                    toast(`Deleted ${info.name}`, 'info');
                  })
                  .catch((e) => toast(errorMessage(e), 'bad'))
              }
            >
              Delete list
            </Button>
          </>
        }
      >
        <p className="text-subtext">
          The list and its {info.count} {noun(info.count, 'entry', 'entries')} are removed from Marquee. Your ratings and history stay, and nothing is removed from any connected service.
        </p>
      </Modal>
      {sharing && <ShareModal info={info} entries={entries} onClose={() => setSharing(false)} />}
    </>
  );
}

export default function Lists({ listId }: { listId: number | null }) {
  return listId ? <ListView key={listId} listId={listId} /> : <Overview />;
}
