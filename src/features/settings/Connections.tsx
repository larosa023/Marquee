// Connections: every service Marquee can learn from, each with its status and its own way of signing in.
// Used by Settings → Connections and by the first-run flow.
import { useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { api, errorMessage, openUrl, pickFile } from '@/lib/api';
import { ago, noun } from '@/lib/format';
import type { KeyStatus, SourceStatus } from '@/lib/types';
import { EMPTY, useApp } from '@/state/app';
import { refreshFeeds } from '@/ui/actions';
import { Icon, type IconName } from '@/ui/icons';
import { Modal } from '@/ui/overlays';
import { Badge, Button, Input, Toggle, cx } from '@/ui/primitives';
import { useImporter } from './Import';

const SOURCE_ICON: Record<string, IconName> = { steam: 'gamepad', trakt: 'tv', spotify: 'music', lastfm: 'music', letterboxd: 'film', mal: 'tv', serializd: 'tv', import: 'upload' };
const SOURCE_KEYS: Record<string, string[]> = { steam: ['steam'], trakt: ['trakt_client_id', 'trakt_client_secret'], spotify: ['spotify_client_id'], lastfm: ['lastfm_key'], letterboxd: [], mal: ['mal_client_id'], serializd: [], import: [] };

function useAfterConnect() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return async (source: string, name: string, account?: string | null) => {
    toast(`${name} connected${account ? ` as ${account}` : ''}. Syncing now.`, 'good');
    await useApp.getState().refreshBoot();
    useApp.getState().set({ sync: { running: true, label: `Reading ${name}`, pct: 0.02 } });
    void api('sync', { source })
      .catch((e) => toast(errorMessage(e), 'bad'))
      .finally(() => refreshFeeds(qc));
  };
}

function StatusBadge({ s }: { s: SourceStatus }) {
  if (s.status === 'syncing') return <Badge tone="accent">Syncing</Badge>;
  if (s.status === 'error') return <Badge tone="bad">Needs attention</Badge>;
  if (s.status === 'connected') return <Badge tone="good">Connected</Badge>;
  return <Badge>Not connected</Badge>;
}

// ------------------------------------------------------------ sign-in flows

function TraktFlow({ onDone }: { onDone: (account: string) => void }) {
  const toast = useApp((s) => s.toast);
  const [code, setCode] = useState<{ user_code: string; verification_url: string; device_code: string; interval: number; expires_in: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const done = useRef(onDone);
  done.current = onDone;

  const start = () => {
    setError(null);
    setCode(null);
    api('connect.trakt.start')
      .then((c) => {
        setCode(c);
        setLeft(c.expires_in);
      })
      .catch((e) => setError(errorMessage(e)));
  };
  useEffect(start, []);
  useEffect(() => {
    if (!code) return;
    let live = true;
    const poll = setInterval(
      () => {
        api<{ account: string | null }>('connect.trakt.poll', { device_code: code.device_code })
          .then((r) => {
            if (live && r.account) {
              clearInterval(poll);
              done.current(r.account);
            }
          })
          .catch((e) => {
            if (!live) return;
            clearInterval(poll);
            setCode(null);
            setError(errorMessage(e));
          });
      },
      Math.max(5, code.interval) * 1000,
    );
    const tick = setInterval(() => setLeft((s) => Math.max(0, s - 1)), 1000);
    return () => {
      live = false;
      clearInterval(poll);
      clearInterval(tick);
    };
  }, [code]);

  if (error) {
    return (
      <div>
        <p className="rounded-control bg-bad/10 p-3 text-[13px] text-bad">{error}</p>
        <Button className="mt-3" icon="sync" onClick={start}>
          Get a new code
        </Button>
      </div>
    );
  }
  if (!code) return <div className="skeleton h-28 w-full" aria-busy="true" />;
  return (
    <div className="text-center">
      <p className="text-subtext">
        Open <span className="font-semibold text-text">{code.verification_url.replace(/^https?:\/\//, '')}</span> and enter this code:
      </p>
      <div className="selectable tnum display my-4 text-[40px] font-extrabold tracking-[0.18em] text-accent" aria-label={`Code: ${code.user_code.split('').join(' ')}`}>
        {code.user_code}
      </div>
      <div className="flex flex-wrap justify-center gap-2">
        <Button variant="primary" icon="external" onClick={() => void openUrl(code.verification_url)}>
          Open Trakt
        </Button>
        <Button icon="copy" onClick={() => void navigator.clipboard.writeText(code.user_code).then(() => toast('Code copied', 'good'))}>
          Copy code
        </Button>
      </div>
      <p className="tnum mt-4 text-[12px] text-faint" aria-live="off">
        Waiting for you to approve it on Trakt · the code lasts another {Math.floor(left / 60)}:{String(left % 60).padStart(2, '0')}
      </p>
    </div>
  );
}

function Waiting({ service, onCancel }: { service: string; onCancel: () => void }) {
  return (
    <div className="text-center">
      <div className="mx-auto mb-4 h-1.5 w-40 overflow-hidden rounded-full bg-strong">
        <div className="skeleton h-full w-full rounded-full!" />
      </div>
      <p className="font-medium">Finish signing in to {service} in your browser</p>
      <p className="mt-1 text-[13px] text-subtext">Marquee is waiting for {service} to send you back. This window updates by itself.</p>
      <Button className="mt-4" onClick={onCancel}>
        Close
      </Button>
    </div>
  );
}

function ConnectModal({ source, onClose }: { source: SourceStatus; onClose: () => void }) {
  const after = useAfterConnect();
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState(source.config?.username ?? '');
  const [steps, setSteps] = useState<string[]>([]);
  const [lastfmToken, setLastfmToken] = useState<string | null>(null);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    if (source.id === 'letterboxd')
      api<string[]>('letterboxd.export_steps')
        .then(setSteps)
        .catch(() => {});
    return () => {
      live.current = false;
    };
  }, [source.id]);

  // Only the latest attempt reports back: clicking "Sign in" again replaces the earlier one.
  const attempt = useRef(0);
  const run = async (label: string, call: () => Promise<{ account?: string | null }>, close = true) => {
    const mine = ++attempt.current;
    setBusy(label);
    setError(null);
    try {
      const out = await call();
      // The sign-in may finish after this dialog was closed; still report it.
      await after(source.id, source.name, out.account);
      if (close && live.current) onClose();
    } catch (e) {
      const msg = errorMessage(e);
      if (msg.startsWith('A newer sign-in replaced')) return;
      if (mine !== attempt.current) return;
      if (live.current) setError(msg);
      else toast(msg, 'bad');
    }
    if (live.current && mine === attempt.current) setBusy(null);
  };

  let body: ReactNode = null;
  if (source.id === 'trakt') {
    body = <TraktFlow onDone={(account) => void after('trakt', 'Trakt', account).then(onClose)} />;
  } else if (source.id === 'steam') {
    body =
      busy === 'openid' ? (
        <Waiting service="Steam" onCancel={onClose} />
      ) : (
        <>
          <p className="text-subtext">Sign in on Steam's own site. Marquee only receives your public SteamID, never your password.</p>
          <Button className="mt-4" variant="primary" icon="external" onClick={() => void run('openid', () => api('connect.steam'))}>
            Sign in through Steam
          </Button>
          <div className="my-5 flex items-center gap-3 text-[12px] text-faint">
            <span className="h-px flex-1 bg-white/10" /> or enter your SteamID <span className="h-px flex-1 bg-white/10" />
          </div>
          <div className="flex gap-2">
            <Input className="flex-1" aria-label="SteamID (17 digits)" placeholder="76561198000000000" inputMode="numeric" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && text && void run('id', () => api('connect.steam_id', { steamid: text }))} />
            <Button busy={busy === 'id'} disabled={!text.trim()} onClick={() => void run('id', () => api('connect.steam_id', { steamid: text }))}>
              Connect
            </Button>
          </div>
          <p className="mt-4 text-[13px] text-subtext">
            Your profile's <span className="font-medium text-text">Game details</span> must be public for Steam to share your library: Steam → your profile → Edit Profile → Privacy Settings.
          </p>
        </>
      );
  } else if (source.id === 'spotify') {
    body =
      busy === 'pkce' ? (
        <Waiting service="Spotify" onCancel={onClose} />
      ) : (
        <>
          <p className="text-subtext">Sign in on Spotify's own site and approve what Marquee asks for: reading your top artists, saved albums and playlists, and adding to playlists you choose.</p>
          <Button className="mt-4" variant="primary" icon="external" onClick={() => void run('pkce', () => api('connect.spotify'))}>
            Sign in with Spotify
          </Button>
          <p className="mt-4 text-[13px] text-subtext">{source.note}</p>
        </>
      );
  } else if (source.id === 'lastfm') {
    const connected = source.status !== 'disconnected';
    body = (
      <>
        <p className="text-subtext">Your username is all Marquee needs to read your scrobbles.</p>
        <div className="mt-3 flex gap-2">
          <Input data-autofocus className="flex-1" aria-label="Last.fm username" placeholder="Last.fm username" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && text && void run('user', () => api('connect.lastfm', { username: text }), false)} />
          <Button variant={connected ? 'secondary' : 'primary'} busy={busy === 'user'} disabled={!text.trim()} onClick={() => void run('user', () => api('connect.lastfm', { username: text }), false)}>
            {connected ? 'Change' : 'Connect'}
          </Button>
        </div>
        <div className="mt-6 rounded-control bg-surface p-4">
          <p className="font-medium">Optional: let Marquee love tracks for you</p>
          <p className="mt-1 text-[13px] text-subtext">Rating an album 4½ stars or more in Marquee can love it on Last.fm. That needs you to approve Marquee on Last.fm once.</p>
          {source.config?.can_write ? (
            <p className="mt-3 flex items-center gap-2 text-[13px] text-good">
              <Icon name="check" size={15} /> Approved
            </p>
          ) : lastfmToken ? (
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <span className="text-[13px] text-subtext">Click "Yes, allow access" on the Last.fm page, then:</span>
              <Button variant="primary" busy={busy === 'finish'} onClick={() => void run('finish', () => api('connect.lastfm.auth_finish', { token: lastfmToken }))}>
                I've approved it
              </Button>
            </div>
          ) : (
            <Button
              className="mt-3"
              icon="external"
              busy={busy === 'start'}
              onClick={() => {
                setBusy('start');
                setError(null);
                api<{ token: string }>('connect.lastfm.auth_start')
                  .then((r) => setLastfmToken(r.token))
                  .catch((e) => setError(errorMessage(e)))
                  .finally(() => setBusy(null));
              }}
            >
              Approve on Last.fm
            </Button>
          )}
        </div>
      </>
    );
  } else if (source.id === 'mal') {
    const connected = source.status !== 'disconnected';
    body =
      busy === 'signin' ? (
        <Waiting service="MyAnimeList" onCancel={onClose} />
      ) : (
        <>
          <p className="text-subtext">Your username is enough to read a public anime list: scores, what you've watched, and your Plan to Watch.</p>
          <div className="mt-3 flex gap-2">
            <Input data-autofocus className="flex-1" aria-label="MyAnimeList username" placeholder="MyAnimeList username" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && text && void run('user', () => api('connect.mal', { username: text }), false)} />
            <Button variant={connected ? 'secondary' : 'primary'} busy={busy === 'user'} disabled={!text.trim()} onClick={() => void run('user', () => api('connect.mal', { username: text }), false)}>
              {connected ? 'Change' : 'Connect'}
            </Button>
          </div>
          <div className="mt-6 rounded-control bg-surface p-4">
            <p className="font-medium">Let Marquee update your list</p>
            <p className="mt-1 text-[13px] text-subtext">Sign in on MyAnimeList's own site to add anime to Plan to Watch, mark them completed and score them from Marquee. Signing in also reads a private list.</p>
            {source.config?.can_write ? (
              <p className="mt-3 flex items-center gap-2 text-[13px] text-good">
                <Icon name="check" size={15} /> Signed in
              </p>
            ) : (
              <Button className="mt-3" icon="external" onClick={() => void run('signin', () => api('connect.mal.signin'))}>
                Sign in with MyAnimeList
              </Button>
            )}
          </div>
        </>
      );
  } else if (source.id === 'letterboxd') {
    const counts = source.config?.export_counts as Record<string, number> | undefined;
    body = (
      <>
        <h3 className="font-semibold">Recent activity: your public RSS feed</h3>
        <p className="mt-1 text-[13px] text-subtext">Keeps Marquee up to date with what you log from now on.</p>
        <div className="mt-2 flex gap-2">
          <Input className="flex-1" aria-label="Letterboxd username" placeholder="Letterboxd username" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && text && void run('user', () => api('connect.letterboxd', { username: text }), false)} />
          <Button variant="primary" busy={busy === 'user'} disabled={!text.trim()} onClick={() => void run('user', () => api('connect.letterboxd', { username: text }), false)}>
            {source.config?.username ? 'Change' : 'Connect'}
          </Button>
        </div>
        <h3 className="mt-6 font-semibold">Full history: your data export</h3>
        <p className="mt-1 text-[13px] text-subtext">The RSS feed only holds your latest entries. The export has every rating, like, diary entry and your watchlist.</p>
        <ol className="mt-3 space-y-1.5 text-[13px]">
          {steps.map((s, i) => (
            <li key={i} className="flex gap-2.5">
              <span className="tnum grid h-5 w-5 shrink-0 place-items-center rounded-full bg-accent/15 text-[11px] font-bold text-accent">{i + 1}</span>
              <span className="text-subtext">{s}</span>
            </li>
          ))}
        </ol>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button icon="external" onClick={() => void openUrl('https://letterboxd.com/settings/data/')}>
            Open Letterboxd's export page
          </Button>
          <Button
            variant="primary"
            icon="upload"
            busy={busy === 'zip'}
            onClick={async () => {
              const path = await pickFile([{ name: 'Letterboxd export', extensions: ['zip'] }]);
              if (!path) return;
              setBusy('zip');
              setError(null);
              try {
                const out = await api<{ counts: Record<string, number> }>('import.letterboxd', { path });
                const total = Object.values(out.counts).reduce((a, b) => a + b, 0);
                toast(`Imported ${total.toLocaleString()} Letterboxd ${noun(total, 'entry', 'entries')}. Matching them to the catalog now.`, 'good');
                await useApp.getState().refreshBoot();
              } catch (e) {
                setError(errorMessage(e));
              }
              setBusy(null);
            }}
          >
            Choose the export zip…
          </Button>
        </div>
        {counts && (
          <p className="tnum mt-3 text-[13px] text-good">
            <Icon name="check" size={14} className="mr-1 inline" />
            Last export imported {ago(source.config.export_imported_at)}:{' '}
            {Object.entries(counts)
              .filter(([, n]) => n > 0)
              .map(([k, n]) => `${n.toLocaleString()} ${k}`)
              .join(', ')}
          </p>
        )}
      </>
    );
  }

  return (
    <Modal open onClose={onClose} title={source.status === 'disconnected' ? `Connect ${source.name}` : source.name} width={520}>
      {body}
      {error && (
        <p role="alert" className="mt-4 rounded-control bg-bad/10 p-3 text-[13px] text-bad">
          {error}
        </p>
      )}
    </Modal>
  );
}

// ------------------------------------------------------------------ the list

function SourceCard({ s, keys, compact, onConnect, onKeys }: { s: SourceStatus; keys: KeyStatus[]; compact?: boolean; onConnect: () => void; onKeys: () => void }) {
  const qc = useQueryClient();
  const toast = useApp((st) => st.toast);
  const syncing = useApp((st) => st.sync.running);
  const [open, setOpen] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [keep, setKeep] = useState(false);
  const importer = useImporter(s.id === 'serializd' ? 'serializd' : 'import');
  const isFile = s.id === 'serializd' || s.id === 'import';
  const connected = s.status !== 'disconnected';
  const missing = (SOURCE_KEYS[s.id] ?? []).filter((k) => !keys.find((x) => x.name === k)?.set);
  const sub = isFile ? (s.config?.last_file ? `Last file: ${s.config.last_file}, imported ${ago(s.config.imported_at)}` : null) : connected ? [s.account, s.last_sync ? `synced ${ago(s.last_sync)}` : 'not synced yet'].filter(Boolean).join(' · ') : null;

  return (
    <li className="hairline rounded-card bg-surface">
      <div className="flex items-center gap-3.5 p-4">
        <span className={cx('grid h-11 w-11 shrink-0 place-items-center rounded-control', connected ? 'bg-accent/15 text-accent' : 'bg-strong text-subtext')}>
          <Icon name={SOURCE_ICON[s.id] ?? 'plug'} size={21} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[15px] font-semibold">{s.name}</span>
            {!isFile && <StatusBadge s={s} />}
          </div>
          {sub && <p className="tnum mt-0.5 truncate text-[13px] text-subtext">{sub}</p>}
          {!sub && <p className="clamp-2 mt-0.5 text-[13px] text-subtext">{s.note}</p>}
          {s.error && (
            <p role="alert" className="mt-1 text-[13px] text-bad">
              {s.error}
            </p>
          )}
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {isFile ? (
            <Button variant={connected ? 'secondary' : 'primary'} icon="upload" onClick={() => void importer.start()}>
              {s.id === 'serializd' ? 'Import a Serializd file…' : 'Import a file…'}
            </Button>
          ) : missing.length > 0 && !connected ? (
            <Button icon="plug" onClick={onKeys} title="This service needs an API key first">
              Add API key
            </Button>
          ) : connected ? (
            <>
              <Button
                icon="sync"
                disabled={syncing}
                onClick={() => {
                  useApp.getState().set({ sync: { running: true, label: `Reading ${s.name}`, pct: 0.02 } });
                  void api('sync', { source: s.id })
                    .catch((e) => toast(errorMessage(e), 'bad'))
                    .finally(() => refreshFeeds(qc));
                }}
              >
                Sync
              </Button>
              <Button onClick={onConnect}>{s.status === 'error' ? 'Reconnect' : s.id === 'letterboxd' || s.id === 'lastfm' || s.id === 'mal' ? 'Manage' : 'Reconnect'}</Button>
            </>
          ) : (
            <Button variant="primary" onClick={onConnect}>
              Connect
            </Button>
          )}
          {!compact && <Button variant="ghost" size="sm" aria-expanded={open} onClick={() => setOpen(!open)} icon={open ? 'chevronDown' : 'chevronRight'} aria-label={`${open ? 'Hide' : 'Show'} details for ${s.name}`} />}
        </div>
      </div>
      <AnimatePresence initial={false}>
        {open && !compact && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <div className="border-t border-white/8 p-4 text-[13px]">
              <p className="text-subtext">{s.note}</p>
              <div className="mt-3 grid gap-4 sm:grid-cols-2">
                <div>
                  <h4 className="mb-1 font-semibold">Marquee reads</h4>
                  <ul className="space-y-0.5 text-subtext">
                    {s.reads.map((r) => (
                      <li key={r} className="flex items-center gap-1.5">
                        <Icon name="download" size={13} className="text-faint" /> {r}
                      </li>
                    ))}
                  </ul>
                </div>
                <div>
                  <h4 className="mb-1 font-semibold">Marquee can write</h4>
                  {s.writes.length === 0 ? (
                    <p className="text-faint">Nothing. Read-only.</p>
                  ) : (
                    <ul className="space-y-0.5 text-subtext">
                      {s.writes.map((w) => (
                        <li key={w} className="flex items-center gap-1.5">
                          <Icon name="upload" size={13} className="text-faint" /> {w}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </div>
              {connected && (
                <Button className="mt-4" variant="danger" size="sm" icon="x" onClick={() => setDisconnecting(true)}>
                  {isFile ? 'Forget imported history' : 'Disconnect'}
                </Button>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      <Modal
        open={disconnecting}
        onClose={() => setDisconnecting(false)}
        title={isFile ? 'Forget imported history?' : `Disconnect ${s.name}?`}
        width={460}
        footer={
          <>
            <Button onClick={() => setDisconnecting(false)}>Cancel</Button>
            <Button
              variant="danger"
              onClick={() =>
                void api('disconnect', { source: s.id, keep_history: isFile ? false : keep })
                  .then(async () => {
                    setDisconnecting(false);
                    toast(isFile ? 'Imported history forgotten' : `${s.name} disconnected`, 'info');
                    await useApp.getState().refreshBoot();
                    refreshFeeds(qc);
                  })
                  .catch((e) => toast(errorMessage(e), 'bad'))
              }
            >
              {isFile ? 'Forget it' : 'Disconnect'}
            </Button>
          </>
        }
      >
        {isFile ? (
          <p className="text-subtext">Everything brought in from files is removed from your taste profile. Your lists stay.</p>
        ) : (
          <>
            <p className="text-subtext">
              Marquee deletes its sign-in for {s.name} from Windows Credential Manager and stops syncing. Nothing is changed on {s.name} itself.
            </p>
            <Toggle checked={keep} onChange={setKeep} label="Keep what it already told Marquee" description="Leave its ratings and history in your taste profile." />
          </>
        )}
      </Modal>
      {importer.element}
    </li>
  );
}

export function ConnectionsList({ compact, only }: { compact?: boolean; only?: string[] }) {
  const sources = useApp((s) => s.boot?.sources ?? EMPTY);
  const keys = useApp((s) => s.boot?.keys ?? EMPTY);
  const navigate = useApp((s) => s.navigate);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [keysOpen, setKeysOpen] = useState(false);
  const shown = only ? sources.filter((s) => only.includes(s.id)) : sources;
  const active = sources.find((s) => s.id === connecting);
  return (
    <>
      <ul className="space-y-3">
        {shown.map((s) => (
          <SourceCard key={s.id} s={s} keys={keys} compact={compact} onConnect={() => setConnecting(s.id)} onKeys={() => (compact ? setKeysOpen(true) : navigate('/settings/keys'))} />
        ))}
      </ul>
      {active && <ConnectModal source={active} onClose={() => setConnecting(null)} />}
      <Modal open={keysOpen} onClose={() => setKeysOpen(false)} title="API keys" width={640}>
        <ApiKeys />
      </Modal>
    </>
  );
}

// ------------------------------------------------------------------ API keys

/** Keys whose absence turns nothing off. */
const OPTIONAL_KEYS = ['itad', 'mal_client_secret'];

const KEY_LABEL: Record<string, string> = {
  tmdb: 'TMDB API key',
  igdb_client_id: 'Twitch client ID (for IGDB)',
  igdb_client_secret: 'Twitch client secret (for IGDB)',
  lastfm_key: 'Last.fm API key',
  lastfm_secret: 'Last.fm shared secret',
  spotify_client_id: 'Spotify client ID',
  trakt_client_id: 'Trakt client ID',
  trakt_client_secret: 'Trakt client secret',
  steam: 'Steam Web API key',
  itad: 'IsThereAnyDeal API key',
  mal_client_id: 'MyAnimeList client ID',
  mal_client_secret: 'MyAnimeList client secret',
};

const CATALOG_KEYS = ['tmdb', 'igdb_client_id', 'igdb_client_secret', 'lastfm_key'];

function KeyRow({ k }: { k: KeyStatus }) {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState('');
  const save = async (v: string) => {
    try {
      await api('keys.set', { name: k.name, value: v });
      await useApp.getState().refreshBoot();
      // A catalog key unlocks posters, dates and details: fetch them right away.
      if (v && CATALOG_KEYS.includes(k.name)) void api('sync').finally(() => refreshFeeds(qc));
      toast(v ? `${KEY_LABEL[k.name] ?? k.name} saved to Windows Credential Manager` : k.built_in ? 'Back to the built-in key' : 'Key removed', 'good');
      setEditing(false);
      setValue('');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  return (
    <li className="py-3">
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{KEY_LABEL[k.name] ?? k.name}</span>
            {k.custom ? <Badge tone="good">Your key</Badge> : k.set ? <Badge tone="accent">Built in</Badge> : <Badge tone={OPTIONAL_KEYS.includes(k.name) ? 'neutral' : 'warn'}>{OPTIONAL_KEYS.includes(k.name) ? 'Optional' : 'Missing'}</Badge>}
          </div>
          <p className="mt-0.5 text-[13px] text-subtext">{k.what}</p>
        </div>
        <Button size="sm" variant="ghost" icon="external" onClick={() => void openUrl(k.url)}>
          Get one
        </Button>
        {k.custom && (
          <Button size="sm" variant="ghost" icon="trash" onClick={() => void save('')}>
            Remove
          </Button>
        )}
        <Button size="sm" onClick={() => setEditing(!editing)} aria-expanded={editing}>
          {k.custom ? 'Replace' : 'Enter key'}
        </Button>
      </div>
      {editing && (
        <div className="mt-2 flex gap-2">
          <Input data-autofocus autoFocus type="password" className="flex-1" aria-label={KEY_LABEL[k.name] ?? k.name} placeholder="Paste the key" autoComplete="off" spellCheck={false} value={value} onChange={(e) => setValue(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && value.trim() && void save(value)} />
          <Button variant="primary" disabled={!value.trim()} onClick={() => void save(value)}>
            Save
          </Button>
        </div>
      )}
    </li>
  );
}

export function ApiKeys() {
  const keys = useApp((s) => s.boot?.keys ?? EMPTY);
  return (
    <>
      <p className="text-[13px] text-subtext">Marquee talks to each service directly from this PC, using an API key that identifies the app. Keys you enter here are stored in Windows Credential Manager, never in a file or the database, and are never shown again after saving.</p>
      <ul className="mt-2 divide-y divide-white/8">
        {keys.map((k) => (
          <KeyRow key={k.name} k={k} />
        ))}
      </ul>
    </>
  );
}
