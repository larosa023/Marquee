// Settings sections that deal with what's on disk: storage, backup and restore, updates.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState, type ReactNode } from 'react';
import { api, errorMessage, on, pickFile, pickFolder, pickSave, shell } from '@/lib/api';
import { ago, bytes, longDate } from '@/lib/format';
import { useApp, useSetting, type UpdateInfo } from '@/state/app';
import { Icon } from '@/ui/icons';
import { Modal } from '@/ui/overlays';
import { Badge, Button, Field, Input, Segmented, Select, Skeleton, Toggle } from '@/ui/primitives';

// ------------------------------------------------------------------- storage

interface Storage {
  images_bytes: number;
  images: number;
  image_cap_bytes: number;
  database_bytes: number;
  cached_responses: number;
  backups_bytes: number;
  imdb_bytes: number;
  imdb_updated_at: string | null;
  data_dir: string;
}

export function StorageSection() {
  const toast = useApp((s) => s.toast);
  const sys = useApp((s) => s.sys);
  const qc = useQueryClient();
  const { data, refetch } = useQuery({ queryKey: ['storage'], queryFn: () => api<Storage>('storage'), staleTime: 0 });
  const [cap, setCap] = useSetting<number>('cache.image_cap_mb', 1024);
  const [wipe, setWipe] = useState(false);
  const [typed, setTyped] = useState('');
  const [moving, setMoving] = useState<string | null>(null);
  const [imdbBusy, setImdbBusy] = useState(false);

  const exportLibrary = async (format: 'csv' | 'json') => {
    const path = await pickSave(`marquee-library.${format}`, [{ name: format.toUpperCase(), extensions: [format] }]);
    if (!path) return;
    try {
      await api('export.library', { format, path });
      toast(`Library exported as ${format.toUpperCase()}`, 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  const used = data ? Math.min(1, data.images_bytes / Math.max(1, data.image_cap_bytes)) : 0;

  return (
    <>
      <div className="hairline rounded-card bg-surface p-4">
        {!data ? (
          <Skeleton className="h-20 w-full" />
        ) : (
          <>
            <div className="flex items-baseline justify-between gap-3">
              <span className="font-medium">Artwork cache</span>
              <span className="tnum text-[13px] text-subtext">
                {bytes(data.images_bytes)} of {bytes(data.image_cap_bytes)} · {data.images.toLocaleString()} images
              </span>
            </div>
            <div className="mt-2 h-2 overflow-hidden rounded-full bg-strong" role="meter" aria-label="Artwork cache used" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(used * 100)}>
              <div className="h-full rounded-full bg-accent transition-[width] duration-500" style={{ width: `${used * 100}%` }} />
            </div>
            <dl className="tnum mt-3 grid grid-cols-3 gap-3 text-[13px]">
              <div>
                <dt className="text-faint">Your data</dt>
                <dd className="font-medium">{bytes(data.database_bytes)}</dd>
              </div>
              <div>
                <dt className="text-faint">Saved catalog answers</dt>
                <dd className="font-medium">{data.cached_responses.toLocaleString()}</dd>
              </div>
              <div>
                <dt className="text-faint">Backups</dt>
                <dd className="font-medium">{bytes(data.backups_bytes)}</dd>
              </div>
            </dl>
            <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-white/8 pt-3 text-[13px]">
              <span className="min-w-0 flex-1 text-subtext">
                IMDb episode ratings: <span className="tnum font-medium text-text">{data.imdb_bytes > 0 ? bytes(data.imdb_bytes) : 'not downloaded'}</span>
                {data.imdb_updated_at ? ` · updated ${ago(data.imdb_updated_at)}` : ''}
              </span>
              <Button
                size="sm"
                icon="sync"
                busy={imdbBusy}
                onClick={() => {
                  setImdbBusy(true);
                  void api<{ episodes: number }>('imdb.refresh')
                    .then((r) => toast(`IMDb ratings updated: ${r.episodes.toLocaleString()} episodes`, 'good'))
                    .catch((e) => toast(errorMessage(e), 'bad'))
                    .finally(() => (setImdbBusy(false), void refetch()));
                }}
              >
                {data.imdb_bytes > 0 ? 'Update now' : 'Download now'}
              </Button>
              {data.imdb_bytes > 0 && (
                <Button size="sm" variant="ghost" icon="trash" onClick={() => void api('imdb.remove').then(() => (toast('IMDb ratings removed and turned off', 'info'), refetch(), useApp.getState().refreshBoot()))}>
                  Remove
                </Button>
              )}
            </div>
          </>
        )}
      </div>
      <div className="mt-3 divide-y divide-white/8">
        <Field label="Artwork cache limit" description="Oldest artwork is removed first once the limit is reached.">
          <Select
            label="Artwork cache limit"
            className="w-40"
            value={String(cap)}
            onChange={(v) => {
              setCap(Number(v));
              setTimeout(() => void refetch(), 600);
            }}
            options={[256, 512, 1024, 2048, 4096, 8192].map((mb) => ({ value: String(mb), label: mb >= 1024 ? `${mb / 1024} GB` : `${mb} MB` }))}
          />
        </Field>
        <Field label="Clear cache" description="Removes saved artwork and catalog answers. Nothing of yours is deleted; it all downloads again as you browse.">
          <Button
            icon="trash"
            onClick={() =>
              void api('cache.clear')
                .then(() => (toast('Cache cleared', 'good'), refetch()))
                .catch((e) => toast(errorMessage(e), 'bad'))
            }
          >
            Clear cache
          </Button>
        </Field>
        <Field label="Data location" description={<span className="selectable break-all">{data?.data_dir ?? sys?.data_dir}</span>} row={false}>
          <div className="flex flex-wrap gap-2">
            <Button icon="external" onClick={() => void shell('open_folder', { which: 'data' })}>
              Open folder
            </Button>
            <Button
              disabled={sys?.portable}
              title={sys?.portable ? 'In portable mode the data stays next to Marquee.exe' : undefined}
              onClick={async () => {
                const dir = await pickFolder();
                if (dir) setMoving(dir);
              }}
            >
              Move…
            </Button>
            {sys && !sys.portable && sys.data_dir !== sys.default_data_dir && <Button onClick={() => setMoving(sys.default_data_dir)}>Move back to the default</Button>}
          </div>
          {sys?.portable && <p className="mt-2 text-[12px] text-faint">Portable mode: everything is kept in the "data" folder next to Marquee.exe.</p>}
        </Field>
        <Field label="Export your library" description="Every title you've rated, watched, played, listened to or saved, with your ratings and dates.">
          <div className="flex gap-2">
            <Button icon="download" onClick={() => void exportLibrary('csv')}>
              CSV
            </Button>
            <Button icon="download" onClick={() => void exportLibrary('json')}>
              JSON
            </Button>
          </div>
        </Field>
        <Field label="Delete all my data" description="Wipes the database, the cache, backups, custom themes, and every stored sign-in and API key. Connected services are not touched.">
          <Button variant="danger" icon="trash" onClick={() => (setTyped(''), setWipe(true))}>
            Delete everything…
          </Button>
        </Field>
      </div>

      <Modal
        open={wipe}
        onClose={() => setWipe(false)}
        title="Delete all your Marquee data?"
        width={480}
        footer={
          <>
            <Button onClick={() => setWipe(false)}>Cancel</Button>
            <Button
              variant="danger"
              icon="trash"
              disabled={typed !== 'DELETE'}
              onClick={() =>
                void api('data.delete_all', { confirm: 'DELETE' })
                  .then(async () => {
                    setWipe(false);
                    qc.clear();
                    location.hash = '#/home';
                    await useApp.getState().refreshBoot();
                  })
                  .catch((e) => toast(errorMessage(e), 'bad'))
              }
            >
              Delete everything
            </Button>
          </>
        }
      >
        <p className="text-subtext">This removes your taste profile, ratings, lists, follows, settings, cache, backups and custom themes from this PC, and deletes Marquee's sign-ins and API keys from Windows Credential Manager. It can't be undone. Make a backup first if you might want any of it.</p>
        <label className="mt-4 block">
          <span className="mb-1.5 block text-[13px]">
            Type <span className="font-bold">DELETE</span> to confirm
          </span>
          <Input data-autofocus value={typed} onChange={(e) => setTyped(e.target.value)} aria-label="Type DELETE to confirm" autoComplete="off" spellCheck={false} />
        </label>
      </Modal>
      <Modal
        open={!!moving}
        onClose={() => setMoving(null)}
        title="Move your data?"
        width={480}
        footer={
          <>
            <Button onClick={() => setMoving(null)}>Cancel</Button>
            <Button
              variant="primary"
              onClick={() =>
                void shell('set_data_location', { path: moving })
                  .catch((e) => toast(errorMessage(e), 'bad'))
                  .finally(() => setMoving(null))
              }
            >
              Move and restart
            </Button>
          </>
        }
      >
        <p className="text-subtext">Marquee copies everything to the new folder, restarts, and uses it from then on. The old folder is left in place until you delete it.</p>
        <p className="selectable mt-3 break-all rounded-control bg-surface p-3 text-[13px]">{moving}</p>
      </Modal>
    </>
  );
}

// ---------------------------------------------------------- backup & restore

interface BackupFile {
  path: string;
  name: string;
  bytes: number;
  created_at: string;
  automatic: boolean;
}
interface BackupPreview {
  manifest: { version: string; created_at: string; automatic: boolean; counts: Record<string, number> };
  themes: string[];
  current: Record<string, number>;
  path: string;
}
const COUNT_LABEL: [string, string][] = [
  ['items', 'Titles'],
  ['ratings', 'Ratings'],
  ['events', 'History entries'],
  ['lists', 'Lists'],
  ['list_items', 'Saved titles'],
  ['follows', 'Follows'],
  ['connections', 'Connections'],
];

export function BackupSection() {
  const toast = useApp((s) => s.toast);
  const pendingFile = useApp((s) => s.pendingFile);
  const qc = useQueryClient();
  const { data: files, refetch } = useQuery({ queryKey: ['backups'], queryFn: () => api<BackupFile[]>('backup.list'), staleTime: 0 });
  const [settings, setSettings] = useSetting<{ auto: boolean; every_days: number; keep: number }>('backup', { auto: true, every_days: 7, keep: 5 });
  const [preview, setPreview] = useState<BackupPreview | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const openPreview = async (path: string) => {
    try {
      setPreview(await api<BackupPreview>('backup.preview', { path }));
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  // A .marqueebackup file double-clicked in Explorer lands here.
  useEffect(() => {
    if (pendingFile?.endsWith('.marqueebackup')) {
      useApp.getState().set({ pendingFile: null });
      void openPreview(pendingFile);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingFile]);

  const create = async (choose: boolean) => {
    const path = choose ? await pickSave(`marquee-${new Date().toISOString().slice(0, 10)}.marqueebackup`, [{ name: 'Marquee backup', extensions: ['marqueebackup'] }]) : undefined;
    if (choose && !path) return;
    setBusy(choose ? 'save' : 'now');
    try {
      const out = await api<{ path: string }>('backup.create', path ? { path } : {});
      toast(`Backup saved: ${out.path.split(/[\\/]/).pop()}`, 'good');
      void refetch();
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    setBusy(null);
  };
  const restore = async () => {
    if (!preview) return;
    setBusy('restore');
    try {
      await api('backup.restore', { path: preview.path });
      setPreview(null);
      await qc.invalidateQueries();
      await useApp.getState().refreshBoot();
      toast('Restored. Your previous data was backed up first, in case you want it back. Sign in to your services again to resume syncing.', 'good');
      void refetch();
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    setBusy(null);
  };

  return (
    <>
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" icon="download" busy={busy === 'now'} onClick={() => void create(false)}>
          Back up now
        </Button>
        <Button icon="download" busy={busy === 'save'} onClick={() => void create(true)}>
          Back up to a file…
        </Button>
        <Button
          icon="upload"
          onClick={async () => {
            const path = await pickFile([{ name: 'Marquee backup', extensions: ['marqueebackup'] }]);
            if (path) void openPreview(path);
          }}
        >
          Restore from a file…
        </Button>
        <Button variant="ghost" icon="external" onClick={() => void shell('open_folder', { which: 'backups' })}>
          Open backups folder
        </Button>
      </div>
      <p className="mt-2 text-[13px] text-subtext">A backup is one .marqueebackup file with your taste profile, ratings, lists, follows, settings and custom themes. Sign-ins and API keys are never included; they stay in Windows Credential Manager.</p>

      <div className="mt-4 divide-y divide-white/8">
        <Toggle checked={settings.auto} onChange={(auto) => setSettings({ ...settings, auto })} label="Back up automatically" description="Made after a sync, when one is due." />
        <Field label="How often">
          <Select
            label="Automatic backup frequency"
            className="w-44"
            value={String(settings.every_days)}
            onChange={(v) => setSettings({ ...settings, every_days: Number(v) })}
            options={[
              { value: '1', label: 'Every day' },
              { value: '3', label: 'Every 3 days' },
              { value: '7', label: 'Every week' },
              { value: '14', label: 'Every 2 weeks' },
              { value: '30', label: 'Every month' },
            ]}
          />
        </Field>
        <Field label="Keep" description="Older automatic backups are deleted. Backups you make yourself are never removed.">
          <Select label="Automatic backups to keep" className="w-44" value={String(settings.keep)} onChange={(v) => setSettings({ ...settings, keep: Number(v) })} options={[3, 5, 10, 20].map((n) => ({ value: String(n), label: `The last ${n}` }))} />
        </Field>
      </div>

      <h3 className="mb-2 mt-6 text-[15px] font-semibold">Backups on this PC</h3>
      {!files ? (
        <Skeleton className="h-24 w-full" />
      ) : files.length === 0 ? (
        <p className="hairline rounded-card bg-surface p-5 text-center text-subtext">No backups yet. The first automatic one is made after your next sync.</p>
      ) : (
        <ul className="hairline divide-y divide-white/8 rounded-card bg-surface">
          {files.map((f) => (
            <li key={f.path} className="flex items-center gap-3 px-4 py-2.5">
              <Icon name="database" size={17} className="text-faint" />
              <div className="min-w-0 flex-1">
                <div className="tnum truncate font-medium">{new Date(f.created_at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</div>
                <div className="tnum truncate text-[12px] text-faint">
                  {f.name} · {bytes(f.bytes)}
                </div>
              </div>
              {f.automatic ? <Badge>Automatic</Badge> : f.name.startsWith('before-restore') ? <Badge tone="warn">Before a restore</Badge> : <Badge tone="accent">Manual</Badge>}
              <Button size="sm" onClick={() => void openPreview(f.path)}>
                Restore…
              </Button>
            </li>
          ))}
        </ul>
      )}

      <Modal
        open={!!preview}
        onClose={() => setPreview(null)}
        title="Restore this backup?"
        width={520}
        footer={
          <>
            <Button onClick={() => setPreview(null)}>Cancel</Button>
            <Button variant="primary" icon="undo" busy={busy === 'restore'} onClick={() => void restore()}>
              Restore
            </Button>
          </>
        }
      >
        {preview && (
          <>
            <p className="text-subtext">
              Made {longDate(preview.manifest.created_at.slice(0, 10))} ({ago(preview.manifest.created_at)}) by Marquee {preview.manifest.version}. Restoring replaces what's here now; Marquee backs up the current data first so you can undo it.
            </p>
            <table className="tnum mt-4 w-full text-left text-[13px]">
              <thead className="text-faint">
                <tr>
                  <th scope="col" className="py-1.5 font-medium">
                    What
                  </th>
                  <th scope="col" className="py-1.5 text-right font-medium">
                    Now
                  </th>
                  <th scope="col" className="py-1.5 text-right font-medium">
                    In the backup
                  </th>
                </tr>
              </thead>
              <tbody>
                {COUNT_LABEL.map(([key, label]) => (
                  <tr key={key} className="border-t border-white/8">
                    <th scope="row" className="py-1.5 font-normal">
                      {label}
                    </th>
                    <td className="py-1.5 text-right text-subtext">{(preview.current[key] ?? 0).toLocaleString()}</td>
                    <td className="py-1.5 text-right font-semibold">{(preview.manifest.counts[key] ?? 0).toLocaleString()}</td>
                  </tr>
                ))}
                <tr className="border-t border-white/8">
                  <th scope="row" className="py-1.5 font-normal">
                    Custom themes
                  </th>
                  <td className="py-1.5 text-right text-subtext">–</td>
                  <td className="py-1.5 text-right font-semibold">{preview.themes.length}</td>
                </tr>
              </tbody>
            </table>
            <p className="mt-3 text-[13px] text-faint">Sign-ins aren't part of a backup. After restoring, reconnect each service in Settings → Connections.</p>
          </>
        )}
      </Modal>
    </>
  );
}

// ------------------------------------------------------------------- updates

interface UpdateState {
  available: UpdateInfo | null;
  channel: 'stable' | 'beta';
  last_check: string | null;
  hangar_installed: boolean;
  deferred_to_hangar: boolean;
  install_on_launch: string | null;
  skipped: string[];
  can_roll_back: boolean;
  previous_version: string | null;
}

/** Release notes as plain structure: headings, bullets and paragraphs. No HTML is ever injected. */
export function Notes({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  let list: string[] = [];
  const clean = (s: string) =>
    s
      .replace(/\*\*(.+?)\*\*/g, '$1')
      .replace(/`(.+?)`/g, '$1')
      .replace(/\[(.+?)\]\(.+?\)/g, '$1');
  const flush = () => {
    if (list.length) {
      const items = list;
      blocks.push(
        <ul key={blocks.length} className="my-2 list-disc space-y-1 pl-5 text-subtext">
          {items.map((li, i) => (
            <li key={i}>{clean(li)}</li>
          ))}
        </ul>,
      );
      list = [];
    }
  };
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (/^\s*[-*]\s+/.test(line)) list.push(line.replace(/^\s*[-*]\s+/, ''));
    else if (/^\s{2,}\S/.test(line) && list.length) list[list.length - 1] += ` ${line.trim()}`;
    else {
      flush();
      if (/^#{1,2}\s/.test(line))
        blocks.push(
          <h3 key={blocks.length} className="mb-1 mt-5 text-[16px] font-semibold first:mt-0">
            {clean(line.replace(/^#+\s*/, ''))}
          </h3>,
        );
      else if (/^#{3,}\s/.test(line))
        blocks.push(
          <h4 key={blocks.length} className="mb-1 mt-4 text-[13px] font-semibold uppercase tracking-wider text-faint">
            {clean(line.replace(/^#+\s*/, ''))}
          </h4>,
        );
      else if (line.trim())
        blocks.push(
          <p key={blocks.length} className="my-2 text-subtext">
            {clean(line)}
          </p>,
        );
    }
  }
  flush();
  return <div className="selectable text-[14px] leading-relaxed">{blocks}</div>;
}

function useInstall() {
  const toast = useApp((s) => s.toast);
  const [progress, setProgress] = useState<number | null>(null);
  useEffect(() => on('update:progress', (p: { downloaded: number; total: number | null }) => setProgress(p.total ? p.downloaded / p.total : 0)), []);
  const install = async (when: 'now' | 'next') => {
    try {
      if (when === 'now') setProgress(0);
      await shell('update_install', { when });
      if (when === 'next') {
        toast('Got it. The update installs the next time Marquee starts.', 'good');
        useApp.getState().set({ updateOpen: false });
      }
    } catch (e) {
      setProgress(null);
      toast(errorMessage(e), 'bad');
    }
  };
  return { progress, install };
}

function InstallButtons({ progress, install }: ReturnType<typeof useInstall>) {
  if (progress != null) {
    return (
      <div className="flex min-w-64 items-center gap-3" role="status">
        <div className="h-2 flex-1 overflow-hidden rounded-full bg-strong">
          <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${Math.round(progress * 100)}%` }} />
        </div>
        <span className="tnum text-[13px] text-subtext">{progress >= 1 ? 'Installing' : `Downloading ${Math.round(progress * 100)}%`}</span>
      </div>
    );
  }
  return (
    <>
      <Button onClick={() => void install('next')}>On next launch</Button>
      <Button variant="primary" icon="download" onClick={() => void install('now')}>
        Install now
      </Button>
    </>
  );
}

/** The dialog behind the "Update ready" pill: what's new, and when to install it. */
export function UpdateModal() {
  const open = useApp((s) => s.updateOpen);
  const update = useApp((s) => s.update);
  const installer = useInstall();
  return (
    <Modal open={open && !!update} onClose={() => useApp.getState().set({ updateOpen: false })} title={`Marquee ${update?.version ?? ''} is ready`} width={600} footer={<InstallButtons {...installer} />}>
      {update && (
        <>
          <p className="tnum mb-3 flex flex-wrap items-center gap-2 text-[13px] text-faint">
            You have {update.current}
            {update.date ? ` · released ${longDate(update.date.slice(0, 10))}` : ''}
            {update.prerelease && <Badge tone="warn">Beta</Badge>}
          </p>
          <Notes text={update.notes || 'No release notes were published for this version.'} />
          <p className="mt-4 text-[12px] text-faint">Marquee restarts to finish installing and returns to this screen. If the new version fails to start, the previous one is put back automatically.</p>
        </>
      )}
    </Modal>
  );
}

export function UpdatesSection() {
  const toast = useApp((s) => s.toast);
  const sys = useApp((s) => s.sys);
  const update = useApp((s) => s.update);
  const { data, refetch } = useQuery({ queryKey: ['update_state'], queryFn: () => shell<UpdateState>('update_state'), staleTime: 0 });
  const [channel, setChannel] = useSetting<'stable' | 'beta'>('updates.channel', 'stable');
  const [defer, setDefer] = useSetting<boolean>('updates.defer_to_hangar', true);
  const [checking, setChecking] = useState(false);
  const installer = useInstall();

  const check = async () => {
    setChecking(true);
    try {
      const found = await shell<UpdateInfo | null>('update_check');
      useApp.getState().set({ update: found });
      if (!found) toast("You're up to date", 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    setChecking(false);
    void refetch();
  };
  const deferred = data?.deferred_to_hangar ?? sys?.deferred_to_hangar;

  return (
    <>
      <div className="hairline rounded-card bg-surface p-5">
        <div className="flex flex-wrap items-center gap-4">
          <span className={`grid h-12 w-12 shrink-0 place-items-center rounded-2xl ${update ? 'bg-accent/15 text-accent' : 'bg-good/15 text-good'}`}>
            <Icon name={update ? 'download' : 'check'} size={24} />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[17px] font-semibold">{update ? `Marquee ${update.version} is ready` : "You're up to date"}</div>
            <div className="tnum text-[13px] text-subtext">
              Version {sys?.version ?? useApp.getState().boot?.version}
              {data?.last_check ? ` · checked ${ago(data.last_check)}` : ''}
              {data?.install_on_launch ? ` · ${data.install_on_launch} installs on next launch` : ''}
            </div>
          </div>
          {update ? (
            <div className="flex flex-wrap items-center gap-2">
              <Button variant="ghost" onClick={() => useApp.getState().set({ updateOpen: true })}>
                What's new
              </Button>
              <InstallButtons {...installer} />
            </div>
          ) : (
            <Button icon="sync" busy={checking} onClick={() => void check()}>
              {checking ? 'Checking' : 'Check now'}
            </Button>
          )}
        </div>
        {deferred && (
          <p className="mt-4 flex items-start gap-2 rounded-control bg-accent/10 p-3 text-[13px] text-subtext">
            <Icon name="info" size={15} className="mt-0.5 shrink-0 text-accent" />
            Hangar is managing Marquee's updates, so Marquee doesn't check or prompt by itself. You can still check and install from here.
          </p>
        )}
      </div>

      <div className="mt-3 divide-y divide-white/8">
        <Field label="Channel" description="Stable gets finished releases. Beta gets them earlier, with rougher edges.">
          <Segmented
            label="Update channel"
            value={channel}
            onChange={(c) => {
              setChannel(c);
              setTimeout(() => void refetch(), 300);
            }}
            options={[
              { value: 'stable', label: 'Stable' },
              { value: 'beta', label: 'Beta' },
            ]}
          />
        </Field>
        {(data?.hangar_installed ?? sys?.hangar_installed) && <Toggle checked={defer} onChange={(v) => (setDefer(v), setTimeout(() => void refetch(), 300))} label="Let Hangar manage updates" description="Hangar updates every app in the suite together. Turn this off for Marquee to check and prompt by itself." />}
        <Field
          label="Roll back"
          description={
            data?.can_roll_back
              ? `Reinstalls ${data.previous_version ? `version ${data.previous_version}` : 'the previous version'}, which Marquee kept when it last updated. Your data is not changed.`
              : 'After an update, the previous version is kept here in case you need it. Marquee also rolls back by itself if a new version fails to start.'
          }
        >
          <Button icon="undo" disabled={!data?.can_roll_back} onClick={() => void shell('update_roll_back').catch((e) => toast(errorMessage(e), 'bad'))}>
            Roll back
          </Button>
        </Field>
      </div>
      <p className="mt-3 text-[12px] text-faint">Marquee checks GitHub Releases when it starts and every 6 hours. Updates are signed, and one that doesn't match Marquee's signing key is refused.</p>
      {data && data.skipped.length > 0 && <p className="mt-1 text-[12px] text-faint">Skipped after failing to start: {data.skipped.join(', ')}.</p>}
    </>
  );
}
