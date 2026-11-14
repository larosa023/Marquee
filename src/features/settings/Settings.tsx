// Settings. One page per section, picked from the list on the left.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState, type ReactNode } from 'react';
import changelog from '../../../CHANGELOG.md?raw';
import { api, errorMessage, inTauri, openUrl, shell } from '@/lib/api';
import { SHORTCUTS, bindings, comboFrom, prettyCombo } from '@/lib/hotkeys';
import { img } from '@/lib/img';
import { EMPTY, useApp, useSetting } from '@/state/app';
import { refreshFeeds } from '@/ui/actions';
import { Icon, type IconName } from '@/ui/icons';
import { Logo } from '@/ui/Logo';
import { Modal } from '@/ui/overlays';
import { Badge, Button, ErrorState, Field, Input, Kbd, Select, Skeleton, Toggle, cx } from '@/ui/primitives';
import { Dial, Editing, useOverride, type TasteData } from '../taste/Taste';
import { NAV } from '../shell/Chrome';
import { AppearanceSection } from './Appearance';
import { ApiKeys, ConnectionsList } from './Connections';
import { BackupSection, Notes, StorageSection, UpdatesSection } from './DataSections';

const SECTIONS: { id: string; label: string; icon: IconName; blurb: string }[] = [
  { id: 'general', label: 'General', icon: 'settings', blurb: 'Region, startup and syncing.' },
  { id: 'connections', label: 'Connections', icon: 'plug', blurb: 'The services Marquee learns your taste from, and where it saves things for you.' },
  { id: 'keys', label: 'API keys', icon: 'link', blurb: 'The keys Marquee uses to talk to each service.' },
  { id: 'taste', label: 'Taste', icon: 'dna', blurb: 'How picks are made: the dial, pins and blocks, and how much each source counts.' },
  { id: 'streaming', label: 'Streaming services', icon: 'tv', blurb: "Tick what you subscribe to. Marquee uses it for 'on my services' filters and where-to-watch." },
  { id: 'notifications', label: 'Notifications', icon: 'bell', blurb: 'What Marquee tells you about, and when it stays quiet.' },
  { id: 'appearance', label: 'Appearance', icon: 'palette', blurb: 'Themes, accent, motion, density and scale.' },
  { id: 'ai', label: 'AI', icon: 'cpu', blurb: 'Optional, and entirely on this PC, through Ollama.' },
  { id: 'storage', label: 'Storage', icon: 'database', blurb: 'What Marquee keeps on disk, and where.' },
  { id: 'backup', label: 'Backup & Restore', icon: 'download', blurb: 'Keep a copy of everything, and bring it back.' },
  { id: 'updates', label: 'Updates', icon: 'rocket', blurb: 'New versions of Marquee.' },
  { id: 'shortcuts', label: 'Keyboard shortcuts', icon: 'keyboard', blurb: 'Every shortcut, and a way to change each one.' },
  { id: 'about', label: 'About', icon: 'info', blurb: '' },
];

const REGIONS: [string, string][] = [
  ['US', 'United States'],
  ['CA', 'Canada'],
  ['GB', 'United Kingdom'],
  ['IE', 'Ireland'],
  ['AU', 'Australia'],
  ['NZ', 'New Zealand'],
  ['DE', 'Germany'],
  ['AT', 'Austria'],
  ['CH', 'Switzerland'],
  ['FR', 'France'],
  ['BE', 'Belgium'],
  ['NL', 'Netherlands'],
  ['ES', 'Spain'],
  ['PT', 'Portugal'],
  ['IT', 'Italy'],
  ['SE', 'Sweden'],
  ['NO', 'Norway'],
  ['DK', 'Denmark'],
  ['FI', 'Finland'],
  ['PL', 'Poland'],
  ['CZ', 'Czechia'],
  ['HU', 'Hungary'],
  ['RO', 'Romania'],
  ['GR', 'Greece'],
  ['TR', 'Türkiye'],
  ['MX', 'Mexico'],
  ['BR', 'Brazil'],
  ['AR', 'Argentina'],
  ['CL', 'Chile'],
  ['CO', 'Colombia'],
  ['JP', 'Japan'],
  ['KR', 'South Korea'],
  ['IN', 'India'],
  ['SG', 'Singapore'],
  ['PH', 'Philippines'],
  ['ID', 'Indonesia'],
  ['TH', 'Thailand'],
  ['ZA', 'South Africa'],
  ['IL', 'Israel'],
  ['AE', 'United Arab Emirates'],
];
const LANGUAGES: [string, string][] = [
  ['en-US', 'English (US)'],
  ['en-GB', 'English (UK)'],
  ['es-ES', 'Español'],
  ['es-MX', 'Español (México)'],
  ['fr-FR', 'Français'],
  ['de-DE', 'Deutsch'],
  ['it-IT', 'Italiano'],
  ['pt-BR', 'Português (Brasil)'],
  ['nl-NL', 'Nederlands'],
  ['sv-SE', 'Svenska'],
  ['pl-PL', 'Polski'],
  ['ja-JP', '日本語'],
  ['ko-KR', '한국어'],
];

function Group({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-white/8">{children}</div>;
}

/** Changing region or language changes what the catalogs return, so refresh them. */
function useResync() {
  const qc = useQueryClient();
  return () => {
    setTimeout(() => {
      useApp.getState().set({ sync: { running: true, label: 'Refreshing the catalog', pct: 0.02 } });
      void api('sync')
        .catch(() => {})
        .finally(() => refreshFeeds(qc));
    }, 400);
  };
}

function General() {
  const toast = useApp((s) => s.toast);
  const resync = useResync();
  const [region, setRegion] = useSetting('region', 'US');
  const [language, setLanguage] = useSetting('language', 'en-US');
  const [startInTray, setStartInTray] = useSetting('start_in_tray', false);
  const [closeToTray, setCloseToTray] = useSetting('close_to_tray', true);
  const [release, setRelease] = useSetting('tray.release_window', true);
  const [homeTab, setHomeTab] = useSetting('general.home_tab', '/home');
  const [onLaunch, setOnLaunch] = useSetting('sync.on_launch', true);
  const [interval, setIntervalHours] = useSetting<number>('sync.interval_hours', 6);
  const [autoplay, setAutoplay] = useSetting('home.trailer_autoplay', true);
  const [imdb, setImdb] = useSetting('imdb.enabled', true);
  const [autostart, setAutostart] = useState<boolean | null>(null);

  useEffect(() => {
    if (!inTauri) return setAutostart(false);
    import('@tauri-apps/plugin-autostart')
      .then((m) => m.isEnabled())
      .then(setAutostart)
      .catch(() => setAutostart(false));
  }, []);
  const toggleAutostart = async (v: boolean) => {
    try {
      const m = await import('@tauri-apps/plugin-autostart');
      await (v ? m.enable() : m.disable());
      setAutostart(v);
    } catch (e) {
      toast(inTauri ? errorMessage(e) : 'Startup can only be changed in the desktop app.', 'bad');
    }
  };

  return (
    <Group>
      <Field label="Region" description="Release dates, prices and where-to-watch are shown for this country.">
        <Select label="Region" className="w-56" value={region} onChange={(v) => (setRegion(v), resync())} options={REGIONS.map(([value, label]) => ({ value, label }))} />
      </Field>
      <Field label="Language" description="The language for titles and descriptions, where the catalogs have them.">
        <Select label="Language" className="w-56" value={language} onChange={(v) => (setLanguage(v), resync())} options={LANGUAGES.map(([value, label]) => ({ value, label }))} />
      </Field>
      <Field label="Open on" description="The screen Marquee starts on.">
        <Select label="Default screen" className="w-56" value={homeTab} onChange={setHomeTab} options={NAV.filter((n) => n.path !== '/settings').map((n) => ({ value: n.path, label: n.label }))} />
      </Field>
      <Toggle checked={!!autostart} disabled={autostart == null} onChange={(v) => void toggleAutostart(v)} label="Launch at startup" description="Start Marquee when you sign in to Windows." />
      <Toggle checked={startInTray} onChange={setStartInTray} disabled={!autostart} label="Start minimized to the tray" description="When launched at startup, stay in the tray until you open it." />
      <Toggle checked={closeToTray} onChange={setCloseToTray} label="Keep running in the tray when closed" description="Closing the window keeps Marquee syncing and able to notify you. Quit from the tray icon." />
      <Toggle checked={release} onChange={setRelease} label="Free memory while in the tray" description="After five minutes hidden, Marquee lets go of its window and rebuilds it when you come back." />
      <Toggle checked={onLaunch} onChange={setOnLaunch} label="Sync when Marquee starts" />
      <Field label="Sync every" description="How often Marquee checks your services and the release catalogs while it's running.">
        <Select
          label="Sync interval"
          className="w-44"
          value={String(interval)}
          onChange={(v) => setIntervalHours(Number(v))}
          options={[
            { value: '1', label: 'Hour' },
            { value: '3', label: '3 hours' },
            { value: '6', label: '6 hours' },
            { value: '12', label: '12 hours' },
            { value: '24', label: 'Day' },
            { value: '0', label: 'Never (only when I ask)' },
          ]}
        />
      </Field>
      <Toggle checked={autoplay} onChange={setAutoplay} label="Play trailers on hover" description="On Home, the hero plays its trailer, muted, when you hover it." />
      <Toggle checked={imdb} onChange={setImdb} label="Episode ratings from IMDb" description="The ratings SeriesGraph charts. Marquee downloads IMDb's public rating files (about 60 MB) once a week and keeps the episode ratings on this PC (about 25 MB). Off: episode heatmaps use TMDB's ratings." />
    </Group>
  );
}

function TasteSettings() {
  const navigate = useApp((s) => s.navigate);
  const { data, error, refetch } = useQuery({ queryKey: ['taste'], queryFn: () => api<TasteData>('taste') });
  const setOverride = useOverride();
  if (error) return <ErrorState error={error} onRetry={() => void refetch()} />;
  if (!data) return <Skeleton className="h-64 w-full rounded-card!" />;
  return (
    <div className="space-y-4">
      <Dial value={data.dial} />
      <Editing data={data} setOverride={setOverride} />
      <Button variant="ghost" icon="dna" onClick={() => navigate('/taste')}>
        See your full taste profile
      </Button>
    </div>
  );
}

function Streaming() {
  const [mine, setMine] = useSetting<number[]>('my_providers', []);
  const region = useApp((s) => s.boot?.settings['region'] ?? 'US');
  const qc = useQueryClient();
  const [filter, setFilter] = useState('');
  const { data, error, refetch } = useQuery({ queryKey: ['providers', region], queryFn: () => api<{ id: number; name: string; logo: string | null }[]>('providers'), staleTime: 24 * 3_600_000 });
  const toggle = (id: number) => {
    setMine(mine.includes(id) ? mine.filter((x) => x !== id) : [...mine, id]);
    setTimeout(() => refreshFeeds(qc), 300);
  };
  if (error) return <ErrorState error={error} onRetry={() => void refetch()} />;
  if (!data)
    return (
      <div className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))' }} aria-busy="true">
        {Array.from({ length: 12 }, (_, i) => (
          <Skeleton key={i} className="h-13" />
        ))}
      </div>
    );
  const q = filter.trim().toLowerCase();
  const shown = data.filter((p) => !q || p.name.toLowerCase().includes(q)).sort((a, b) => Number(mine.includes(b.id)) - Number(mine.includes(a.id)));
  return (
    <>
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <Input icon="search" className="w-64" placeholder="Find a service" aria-label="Find a streaming service" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <span className="tnum text-[13px] text-subtext" aria-live="polite">
          {mine.length} selected · showing services in {REGIONS.find((r) => r[0] === region)?.[1] ?? region}
        </span>
      </div>
      <ul className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))' }}>
        {shown.map((p) => {
          const on = mine.includes(p.id);
          return (
            <li key={p.id}>
              <button type="button" role="checkbox" aria-checked={on} onClick={() => toggle(p.id)} className={cx('hairline flex w-full items-center gap-2.5 rounded-control p-2 text-left transition-colors', on ? 'border-accent! bg-accent/10' : 'bg-surface hover:bg-hover')}>
                {p.logo ? (
                  <img src={img(p.logo)} alt="" className="h-9 w-9 rounded-md" loading="lazy" />
                ) : (
                  <span className="grid h-9 w-9 place-items-center rounded-md bg-strong">
                    <Icon name="tv" size={16} />
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{p.name}</span>
                <span className={cx('grid h-5 w-5 shrink-0 place-items-center rounded-md border-2', on ? 'border-accent bg-accent text-on-accent' : 'border-faint text-transparent')}>
                  <Icon name="check" size={12} />
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <p className="mt-3 text-[12px] text-faint">Service list and availability from JustWatch, via TMDB.</p>
    </>
  );
}

interface Notify {
  enabled: boolean;
  releases: boolean;
  seasons: boolean;
  episodes: boolean;
  albums: boolean;
  sales: boolean;
  sale_threshold: number;
  digest: boolean;
  deals: boolean;
  digest_day: number;
  quiet_enabled: boolean;
  quiet_from: string;
  quiet_to: string;
}
const NOTIFY_DEFAULT: Notify = { enabled: true, releases: true, seasons: true, episodes: true, albums: true, sales: true, sale_threshold: 25, digest: true, deals: true, digest_day: 4, quiet_enabled: true, quiet_from: '22:00', quiet_to: '08:00' };

function Notifications() {
  const [stored, setStored] = useSetting<Partial<Notify>>('notifications', {});
  const n = { ...NOTIFY_DEFAULT, ...stored };
  const set = (patch: Partial<Notify>) => setStored({ ...n, ...patch });
  const off = !n.enabled;
  const time = (key: 'quiet_from' | 'quiet_to', label: string) => <input type="time" aria-label={label} value={n[key]} disabled={off || !n.quiet_enabled} onChange={(e) => e.target.value && set({ [key]: e.target.value })} className="hairline tnum h-9.5 rounded-control bg-surface px-2.5 disabled:opacity-50" />;
  return (
    <Group>
      <Toggle checked={n.enabled} onChange={(enabled) => set({ enabled })} label="Windows notifications" description="Show a Windows toast when something happens. Everything still appears in the bell inside Marquee." />
      <Toggle checked={n.releases} disabled={off} onChange={(releases) => set({ releases })} label="Release day" description="When something you follow or saved comes out." />
      <Toggle checked={n.seasons} disabled={off} onChange={(seasons) => set({ seasons })} label="New seasons" description="Premieres and finales of shows you watch." />
      <Toggle checked={n.episodes} disabled={off} onChange={(episodes) => set({ episodes })} label="New episodes" description="Every new episode of a show you're watching (seen in the last 60 days, or followed), with where to watch it." />
      <Toggle checked={n.albums} disabled={off} onChange={(albums) => set({ albums })} label="New music" description="New albums, EPs and singles from artists you follow." />
      <Toggle checked={n.sales} disabled={off} onChange={(sales) => set({ sales })} label="Steam wishlist sales" description="When a wishlisted game goes on sale." />
      <Field label="Only tell me about sales of at least" description="Smaller discounts are still shown on the Games screen.">
        <div className="flex items-center gap-3">
          <input type="range" min={5} max={90} step={5} value={n.sale_threshold} disabled={off || !n.sales} aria-label="Minimum discount" aria-valuetext={`${n.sale_threshold}% off`} onChange={(e) => set({ sale_threshold: Number(e.target.value) })} className="h-2 w-44" />
          <span className="tnum w-16 text-right text-subtext">{n.sale_threshold}% off</span>
        </div>
      </Field>
      <Toggle checked={n.digest} disabled={off} onChange={(digest) => set({ digest })} label='Weekly "Your Marquee" digest' description="One summary of the week's releases that match your taste." />
      <Toggle checked={n.deals} disabled={off} onChange={(deals) => set({ deals })} label="Weekly wishlist deals" description="On digest day: wishlisted games at or near the lowest price on record." />
      <Field label="Digest day">
        <Select label="Digest day" className="w-44" value={String(n.digest_day)} onChange={(v) => set({ digest_day: Number(v) })} options={['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'].map((d, i) => ({ value: String(i), label: d }))} />
      </Field>
      <Toggle checked={n.quiet_enabled} disabled={off} onChange={(quiet_enabled) => set({ quiet_enabled })} label="Quiet hours" description="No toasts during these hours. Anything that arrives is waiting in the bell." />
      <Field label="Quiet from">
        <div className="flex items-center gap-2">
          {time('quiet_from', 'Quiet hours start')}
          <span className="text-subtext">to</span>
          {time('quiet_to', 'Quiet hours end')}
        </div>
      </Field>
    </Group>
  );
}

interface AiStatus {
  enabled: boolean;
  running: boolean;
  endpoint: string;
  models: { name: string; size: number | null; params: string | null; embedding: boolean }[];
  chat_model: string | null;
  embed_model: string | null;
  error: string | null;
}

function Ai() {
  const [stored, setStored] = useSetting<{ enabled?: boolean; endpoint?: string; chat_model?: string; embed_model?: string }>('ai', {});
  const s = { enabled: true, endpoint: 'http://localhost:11434', chat_model: '', embed_model: '', ...stored };
  const [endpoint, setEndpoint] = useState(s.endpoint);
  const { data, refetch, isFetching } = useQuery({ queryKey: ['ai.status', s.endpoint, s.enabled], queryFn: () => api<AiStatus>('ai.status'), staleTime: 0 });
  const set = (patch: Partial<typeof s>) => {
    setStored({ ...s, ...patch });
    setTimeout(() => void refetch(), 400);
  };
  const chat = (data?.models ?? []).filter((m) => !m.embedding);
  const embed = (data?.models ?? []).filter((m) => m.embedding);
  const size = (m: AiStatus['models'][number]) => [m.params, m.size ? `${(m.size / 1e9).toFixed(1)} GB` : null].filter(Boolean).join(', ');
  return (
    <>
      <div className="hairline mb-3 flex flex-wrap items-center gap-3 rounded-card bg-surface p-4" role="status">
        <span className={cx('grid h-11 w-11 place-items-center rounded-control', data?.running && s.enabled ? 'bg-good/15 text-good' : 'bg-strong text-subtext')}>
          <Icon name="cpu" size={21} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="font-semibold">{!data ? 'Looking for Ollama…' : !s.enabled ? 'Local AI is off' : data.error ? "That address can't be used" : data.running ? `Ollama is running · ${data.models.length} model${data.models.length === 1 ? '' : 's'} installed` : "Ollama isn't running"}</div>
          <div className="text-[13px] text-subtext">{data?.error ?? (data?.running ? 'Vibe search understands full sentences, picks get a written "why", and similarity uses local embeddings.' : 'Marquee works fully without it: vibe search matches mood and theme tags instead.')}</div>
        </div>
        <Button icon="sync" busy={isFetching} onClick={() => void refetch()}>
          Check again
        </Button>
        {data && !data.running && (
          <Button icon="external" onClick={() => void openUrl('https://ollama.com/download')}>
            Get Ollama
          </Button>
        )}
      </div>
      <Group>
        <Toggle checked={s.enabled} onChange={(enabled) => set({ enabled })} label="Use local AI when Ollama is running" description="Everything runs on this PC. Marquee only ever talks to an address on this computer, and nothing about you is sent anywhere." />
        <Field label="Ollama address" description="Must be on this computer (localhost).">
          <div className="flex gap-2">
            <Input className="w-64" aria-label="Ollama address" value={endpoint} spellCheck={false} onChange={(e) => setEndpoint(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && set({ endpoint: endpoint.trim() })} />
            <Button disabled={endpoint.trim() === s.endpoint} onClick={() => set({ endpoint: endpoint.trim() })}>
              Save
            </Button>
          </div>
        </Field>
        <Field label="Model for vibe search and write-ups" description={chat.length === 0 && data?.running ? 'No chat model installed. Try: ollama pull llama3.2' : 'Smaller models answer faster.'}>
          <Select label="Chat model" className="w-64" value={s.chat_model} onChange={(chat_model) => set({ chat_model })} options={[{ value: '', label: data?.chat_model ? `Automatic (${data.chat_model})` : 'Automatic' }, ...chat.map((m) => ({ value: m.name, label: size(m) ? `${m.name} (${size(m)})` : m.name }))]} />
        </Field>
        <Field label="Model for similarity" description={embed.length === 0 && data?.running ? 'No embedding model installed. Try: ollama pull nomic-embed-text' : 'An embedding model, such as nomic-embed-text.'}>
          <Select label="Embedding model" className="w-64" value={s.embed_model} onChange={(embed_model) => set({ embed_model })} options={[{ value: '', label: data?.embed_model ? `Automatic (${data.embed_model})` : 'Automatic' }, ...embed.map((m) => ({ value: m.name, label: m.name }))]} />
        </Field>
      </Group>
    </>
  );
}

function Shortcuts() {
  const [overrides, setOverrides] = useSetting<Record<string, string>>('shortcuts', {});
  const [recording, setRecording] = useState<string | null>(null);
  const [clash, setClash] = useState<string | null>(null);
  const map = bindings(overrides);
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') return setRecording(null);
      const combo = comboFrom(e);
      if (!combo) return;
      // A bare letter would fire while typing, so shortcuts need a modifier or a function key.
      if (!(e.ctrlKey || e.metaKey || e.altKey) && !/^F\d{1,2}$/.test(e.key)) return setClash('Use Ctrl or Alt with a key, or a function key.');
      const taken = SHORTCUTS.find((s) => s.id !== recording && map[s.id] === combo);
      if (taken) return setClash(`${prettyCombo(combo).join(' + ')} is already "${taken.label}".`);
      setOverrides({ ...overrides, [recording]: combo });
      setClash(null);
      setRecording(null);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [recording, map, overrides, setOverrides]);
  return (
    <>
      <ul className="divide-y divide-white/8">
        {SHORTCUTS.map((s) => {
          const changed = !!overrides[s.id] && overrides[s.id] !== s.combo;
          return (
            <li key={s.id} className="flex items-center gap-3 py-2">
              <span className="min-w-0 flex-1 font-medium">{s.label}</span>
              {recording === s.id ? (
                <span className="rounded-control bg-accent/15 px-3 py-1.5 text-[13px] font-medium text-accent" role="status">
                  Press the new shortcut… (Esc to cancel)
                </span>
              ) : (
                <span className="flex items-center gap-1" aria-label={prettyCombo(map[s.id]).join(' plus ')}>
                  {prettyCombo(map[s.id]).map((k, i) => (
                    <Kbd key={i}>{k}</Kbd>
                  ))}
                </span>
              )}
              <Button size="sm" onClick={() => (setClash(null), setRecording(recording === s.id ? null : s.id))} aria-label={`Change the shortcut for ${s.label}`}>
                {recording === s.id ? 'Cancel' : 'Change'}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={!changed}
                aria-label={`Reset the shortcut for ${s.label}`}
                onClick={() => {
                  const next = { ...overrides };
                  delete next[s.id];
                  setOverrides(next);
                }}
              >
                Reset
              </Button>
            </li>
          );
        })}
      </ul>
      {clash && (
        <p role="alert" className="mt-3 rounded-control bg-warn/12 px-3 py-2 text-[13px] text-warn">
          {clash}
        </p>
      )}
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Button icon="undo" disabled={Object.keys(overrides).length === 0} onClick={() => setOverrides({})}>
          Reset all
        </Button>
        <span className="text-[12px] text-faint">Also built in: / for search, F5 to sync, Esc to close, right-click or Shift+F10 for a card's menu.</span>
      </div>
    </>
  );
}

const ATTRIBUTIONS: { name: string; text: string; url: string }[] = [
  { name: 'TVmaze', text: 'TV listings, posters and episode ratings from TVmaze, licensed CC BY-SA. Used for TV when no TMDB key is set.', url: 'https://www.tvmaze.com' },
  { name: 'TMDB', text: 'This product uses the TMDB API but is not endorsed or certified by TMDB. Movie and TV metadata, images and trailers come from TMDB.', url: 'https://www.themoviedb.org' },
  { name: 'JustWatch', text: 'Streaming, rental and purchase availability is provided by JustWatch, through TMDB.', url: 'https://www.justwatch.com' },
  { name: 'IGDB', text: 'Game data is provided by IGDB.com, through the Twitch developer API.', url: 'https://www.igdb.com' },
  { name: 'Steam', text: 'Prices, reviews and Steam Deck status come from Steam. Marquee is not affiliated with Valve. Steam and the Steam logo are trademarks of Valve Corporation.', url: 'https://store.steampowered.com' },
  { name: 'MusicBrainz', text: 'Release and artist data comes from MusicBrainz, available under CC0 and CC BY-NC-SA.', url: 'https://musicbrainz.org' },
  { name: 'Cover Art Archive', text: 'Album artwork comes from the Cover Art Archive, a joint project of the Internet Archive and MusicBrainz.', url: 'https://coverartarchive.org' },
  { name: 'Last.fm', text: 'Similar artists, tags and listening history are powered by Last.fm (AudioScrobbler).', url: 'https://www.last.fm' },
  { name: 'Spotify', text: 'Library, playlists and previews are provided by Spotify. Marquee is not affiliated with Spotify AB.', url: 'https://www.spotify.com' },
  { name: 'Trakt', text: 'Watch history, ratings and watchlist sync is provided by Trakt.', url: 'https://trakt.tv' },
  { name: 'IsThereAnyDeal', text: 'Historical low prices are provided by IsThereAnyDeal, when you add a key.', url: 'https://isthereanydeal.com' },
  { name: 'IMDb', text: "Episode and show ratings: information courtesy of IMDb (https://www.imdb.com). Used with permission. From IMDb's non-commercial datasets, kept on this PC.", url: 'https://developer.imdb.com/non-commercial-datasets/' },
  { name: 'MyAnimeList', text: 'Anime lists, scores and seasonal anime come from the MyAnimeList API. Marquee is not affiliated with MyAnimeList.', url: 'https://myanimelist.net' },
  { name: 'Letterboxd · SeriesGraph', text: 'Marquee reads your own public Letterboxd RSS feed and data export, and links to SeriesGraph. It is not affiliated with either.', url: 'https://letterboxd.com' },
];
const LICENSES: [string, string][] = [
  ['Tauri, tauri-plugins', 'MIT / Apache-2.0'],
  ['React, React DOM', 'MIT'],
  ['Framer Motion', 'MIT'],
  ['TanStack Query', 'MIT'],
  ['Zustand', 'MIT'],
  ['Tailwind CSS', 'MIT'],
  ['Vite', 'MIT'],
  ['Inter (typeface)', 'SIL Open Font License 1.1'],
  ['SQLite (bundled via rusqlite)', 'Public domain / MIT'],
  ['reqwest, tokio, serde', 'MIT / Apache-2.0'],
  ['rustls', 'Apache-2.0 / ISC / MIT'],
  ['keyring', 'MIT / Apache-2.0'],
  ['zip, csv, quick-xml, chrono', 'MIT / Apache-2.0'],
];

function About() {
  const toast = useApp((s) => s.toast);
  const sys = useApp((s) => s.sys);
  const version = useApp((s) => s.boot?.version);
  const [open, setOpen] = useState<'changelog' | 'licenses' | null>(null);
  return (
    <>
      <div className="flex flex-wrap items-center gap-5">
        <Logo size={72} />
        <div className="min-w-0 flex-1">
          <div className="display text-[28px] font-extrabold leading-tight">Marquee</div>
          <div className="tnum text-subtext">
            Version {version}
            {sys?.portable ? ' · portable' : ''}
          </div>
          <div className="mt-1 text-[13px] text-faint">New movies, shows, games and albums, picked for your taste. Part of the Hangar suite.</div>
        </div>
      </div>
      <div className="mt-5 flex flex-wrap gap-2">
        <Button icon="list" onClick={() => setOpen('changelog')}>
          Changelog
        </Button>
        <Button icon="info" onClick={() => setOpen('licenses')}>
          Open-source licenses
        </Button>
        <Button icon="external" onClick={() => void openUrl('https://github.com/larosa023/Marquee')}>
          GitHub
        </Button>
      </div>

      <h3 className="mb-2 mt-7 text-[15px] font-semibold">Diagnostics</h3>
      <p className="text-[13px] text-subtext">Marquee has no telemetry and never reports anything by itself. Logs stay in a folder on this PC; "Copy diagnostics" puts a short summary on your clipboard (versions, counts and connection states; never keys, tokens or titles) for you to paste wherever you choose.</p>
      <div className="mt-3 flex flex-wrap gap-2">
        <Button
          variant="primary"
          icon="copy"
          onClick={() =>
            void api<string>('diagnostics')
              .then((text) => navigator.clipboard.writeText(`${text}\nWindow: ${window.innerWidth}×${window.innerHeight} @${window.devicePixelRatio}x\n${navigator.userAgent}\n`))
              .then(() => toast('Diagnostics copied', 'good'))
              .catch((e) => toast(errorMessage(e), 'bad'))
          }
        >
          Copy diagnostics
        </Button>
        <Button icon="external" onClick={() => void shell('open_folder', { which: 'logs' })}>
          Open the log folder
        </Button>
      </div>
      {sys && <p className="selectable mt-2 break-all text-[12px] text-faint">Logs: {sys.log_dir}</p>}

      <h3 className="mb-2 mt-7 text-[15px] font-semibold">Data and attributions</h3>
      <ul className="hairline divide-y divide-white/8 rounded-card bg-surface">
        {ATTRIBUTIONS.map((a) => (
          <li key={a.name} className="flex items-start gap-3 px-4 py-3">
            <div className="min-w-0 flex-1">
              <div className="font-semibold">{a.name}</div>
              <div className="text-[13px] text-subtext">{a.text}</div>
            </div>
            <Button size="sm" variant="ghost" icon="external" aria-label={`Open ${a.name}`} onClick={() => void openUrl(a.url)} />
          </li>
        ))}
      </ul>
      <p className="mt-4 text-[12px] text-faint">Marquee is released under the MIT License. © 2026 Anthony LaRosa.</p>

      <Modal open={open === 'changelog'} onClose={() => setOpen(null)} title="Changelog" width={680}>
        <Notes text={changelog.replace(/^# .*\n/, '')} />
      </Modal>
      <Modal open={open === 'licenses'} onClose={() => setOpen(null)} title="Open-source licenses" width={560}>
        <p className="text-[13px] text-subtext">Marquee is built on these projects, with thanks. Full license texts ship with each project's source; the complete dependency list is in the repository's Cargo.lock and package-lock.json.</p>
        <table className="mt-3 w-full text-left text-[13px]">
          <tbody>
            {LICENSES.map(([name, license]) => (
              <tr key={name} className="border-t border-white/8">
                <th scope="row" className="py-1.5 font-medium">
                  {name}
                </th>
                <td className="py-1.5 text-right text-subtext">{license}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Modal>
    </>
  );
}

export default function Settings({ section }: { section: string }) {
  const navigate = useApp((s) => s.navigate);
  const sources = useApp((s) => s.boot?.sources ?? EMPTY);
  const update = useApp((s) => s.update);
  const current = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];
  const needsAttention = sources.some((s) => s.status === 'error');
  let body: ReactNode;
  switch (current.id) {
    case 'connections':
      body = <ConnectionsList />;
      break;
    case 'keys':
      body = <ApiKeys />;
      break;
    case 'taste':
      body = <TasteSettings />;
      break;
    case 'streaming':
      body = <Streaming />;
      break;
    case 'notifications':
      body = <Notifications />;
      break;
    case 'appearance':
      body = <AppearanceSection />;
      break;
    case 'ai':
      body = <Ai />;
      break;
    case 'storage':
      body = <StorageSection />;
      break;
    case 'backup':
      body = <BackupSection />;
      break;
    case 'updates':
      body = <UpdatesSection />;
      break;
    case 'shortcuts':
      body = <Shortcuts />;
      break;
    case 'about':
      body = <About />;
      break;
    default:
      body = <General />;
  }
  return (
    <div className="flex min-h-full gap-8 px-(--pad)">
      <nav aria-label="Settings sections" className="sticky top-0 w-56 shrink-0 self-start pb-8">
        <h1 className="display mb-4 text-[30px] font-bold leading-tight">Settings</h1>
        <ul className="space-y-0.5">
          {SECTIONS.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                aria-current={s.id === current.id ? 'page' : undefined}
                onClick={() => navigate(`/settings/${s.id}`)}
                className={cx('flex h-9.5 w-full items-center gap-2.5 rounded-control px-3 text-left transition-colors', s.id === current.id ? 'bg-strong font-semibold text-text' : 'text-subtext hover:bg-hover hover:text-text')}
              >
                <Icon name={s.icon} size={17} className={s.id === current.id ? 'text-accent' : undefined} />
                <span className="flex-1 truncate">{s.label}</span>
                {s.id === 'connections' && needsAttention && <span className="h-2 w-2 rounded-full bg-bad" aria-label="Needs attention" />}
                {s.id === 'updates' && update && <Badge tone="accent">New</Badge>}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <section key={current.id} aria-label={current.label} className="min-w-0 max-w-4xl flex-1 pb-10 pt-1">
        <h2 className="display text-[22px] font-bold">{current.label}</h2>
        {current.blurb && <p className="mb-5 mt-1 text-subtext">{current.blurb}</p>}
        {!current.blurb && <div className="mb-5" />}
        {body}
      </section>
    </div>
  );
}
