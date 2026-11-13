// Bring in anything: pick a CSV or JSON file, check (or fix) which column is what, import.
// Known exports (IMDb, RateYourMusic, Trakt, Serializd, Backloggd-style sheets) are mapped automatically.
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { api, errorMessage, pickFile } from '@/lib/api';
import { noun } from '@/lib/format';
import { useApp } from '@/state/app';
import { refreshFeeds } from '@/ui/actions';
import { Icon } from '@/ui/icons';
import { Modal } from '@/ui/overlays';
import { Badge, Button, Select, Skeleton, Toggle } from '@/ui/primitives';

interface Mapping {
  media: string;
  title: string | null;
  year: string | null;
  date: string | null;
  rating: string | null;
  rating_scale: string;
  artist: string | null;
  artist_extra: string | null;
  imdb_id: string | null;
  tmdb_id: string | null;
  steam_id: string | null;
  status: string | null;
  liked: string | null;
  as_wishlist: boolean;
}
interface Preview {
  columns: string[];
  sample: string[][];
  rows: number;
  mapping: Mapping;
  recognised: string | null;
}

const FIELDS: { key: keyof Mapping; label: string; hint: string }[] = [
  { key: 'title', label: 'Title', hint: 'Required unless an ID column is set' },
  { key: 'year', label: 'Year', hint: 'Helps pick the right match' },
  { key: 'rating', label: 'Your rating', hint: '' },
  { key: 'date', label: 'Date watched, played or rated', hint: '' },
  { key: 'artist', label: 'Artist', hint: 'For music' },
  { key: 'artist_extra', label: 'Artist (second column)', hint: 'Some exports split the name in two' },
  { key: 'status', label: 'Status', hint: 'Watched, playing, wishlist…' },
  { key: 'liked', label: 'Liked / favorite', hint: '' },
  { key: 'imdb_id', label: 'IMDb ID', hint: 'tt1234567' },
  { key: 'tmdb_id', label: 'TMDB ID', hint: '' },
  { key: 'steam_id', label: 'Steam app ID', hint: '' },
];

/** Opens the file picker, then the mapper. `source` tags where the history came from ("import" or "serializd"). */
export function useImporter(source: 'import' | 'serializd' = 'import') {
  const [path, setPath] = useState<string | null>(null);
  const start = async (file?: string) => {
    const picked = file ?? (await pickFile([{ name: 'CSV or JSON', extensions: ['csv', 'tsv', 'json', 'txt'] }]));
    if (picked) setPath(picked);
  };
  const element = path ? <ImportModal path={path} source={source} onClose={() => setPath(null)} /> : null;
  return { start, element };
}

function ImportModal({ path, source, onClose }: { path: string; source: string; onClose: () => void }) {
  const toast = useApp((s) => s.toast);
  const qc = useQueryClient();
  const [preview, setPreview] = useState<Preview | null>(null);
  const [mapping, setMapping] = useState<Mapping | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ events: number; wishlist: number; skipped: number } | null>(null);
  const name = path.split(/[\\/]/).pop() ?? path;

  useEffect(() => {
    let live = true;
    api<Preview>('import.preview', { path })
      .then((p) => {
        if (!live) return;
        setPreview(p);
        setMapping(p.mapping);
      })
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [path]);

  const run = async () => {
    if (!mapping) return;
    setBusy(true);
    try {
      const out = await api<{ events: number; wishlist: number; skipped: number }>('import.run', { path, mapping, source });
      setResult(out);
      refreshFeeds(qc);
      void useApp.getState().refreshBoot();
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
    setBusy(false);
  };

  const columns = [{ value: '', label: 'Not in this file' }, ...(preview?.columns ?? []).map((c) => ({ value: c, label: c }))];
  const mediaColumn = mapping?.media.startsWith('column:') ? mapping.media.slice(7) : '';
  const set = (patch: Partial<Mapping>) => mapping && setMapping({ ...mapping, ...patch });
  const ready = !!mapping && (!!mapping.title || !!mapping.imdb_id || !!mapping.tmdb_id || !!mapping.steam_id);

  return (
    <Modal
      open
      onClose={onClose}
      title={result ? 'Imported' : `Import ${name}`}
      width={760}
      footer={
        result ? (
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        ) : (
          <>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" icon="upload" busy={busy} disabled={!ready} onClick={() => void run()}>
              {busy ? 'Importing' : `Import ${preview ? preview.rows.toLocaleString() : ''} ${noun(preview?.rows ?? 2, 'row')}`}
            </Button>
          </>
        )
      }
    >
      {error ? (
        <div className="flex items-start gap-3 rounded-control bg-bad/10 p-4 text-bad">
          <Icon name="alert" size={18} className="mt-0.5 shrink-0" />
          <div>
            <p className="font-medium">That file couldn't be read</p>
            <p className="mt-1 text-[13px]">{error}</p>
          </div>
        </div>
      ) : result ? (
        <div className="py-2">
          <div className="mb-4 grid h-14 w-14 place-items-center rounded-2xl bg-good/15 text-good">
            <Icon name="check" size={28} />
          </div>
          <p className="text-[15px]">
            <span className="tnum font-semibold">{result.events.toLocaleString()}</span> {noun(result.events, 'rating or history entry', 'ratings and history entries')} added
            {result.wishlist > 0 && (
              <>
                , <span className="tnum font-semibold">{result.wishlist.toLocaleString()}</span> saved to your lists
              </>
            )}
            .
          </p>
          {result.skipped > 0 && (
            <p className="mt-1 text-[13px] text-subtext">
              {result.skipped.toLocaleString()} {noun(result.skipped, 'row')} had no title or ID and were skipped.
            </p>
          )}
          <p className="mt-3 text-[13px] text-subtext">Marquee is now matching these titles against the catalogs to learn their genres and themes. Your picks update as it goes; you can keep using the app.</p>
        </div>
      ) : !preview || !mapping ? (
        <div className="space-y-3" aria-busy="true">
          <Skeleton className="h-5 w-1/2" />
          <Skeleton className="h-32 w-full" />
          <Skeleton className="h-48 w-full" />
        </div>
      ) : (
        <>
          <p className="flex flex-wrap items-center gap-2 text-subtext">
            <span className="tnum">
              {preview.rows.toLocaleString()} {noun(preview.rows, 'row')}, {preview.columns.length} columns.
            </span>
            {preview.recognised ? <Badge tone="good">Looks like {preview.recognised}</Badge> : <span>Check that each column is matched to the right thing.</span>}
          </p>

          <div className="mt-4 grid gap-x-5 gap-y-3 sm:grid-cols-2">
            <label className="block">
              <span className="mb-1 block text-[13px] font-medium">What's in this file?</span>
              <Select
                label="Type of media"
                value={mediaColumn ? 'column' : mapping.media}
                onChange={(v) => set({ media: v === 'column' ? `column:${preview.columns[0]}` : v })}
                options={[
                  { value: 'movie', label: 'Movies' },
                  { value: 'tv', label: 'TV shows' },
                  { value: 'game', label: 'Games' },
                  { value: 'album', label: 'Albums' },
                  { value: 'column', label: 'Mixed: a column says which' },
                ]}
              />
            </label>
            {mediaColumn ? (
              <label className="block">
                <span className="mb-1 block text-[13px] font-medium">Type column</span>
                <Select label="Column holding the media type" value={mediaColumn} onChange={(v) => set({ media: `column:${v}` })} options={preview.columns.map((c) => ({ value: c, label: c }))} />
              </label>
            ) : (
              <span />
            )}
            {FIELDS.map((f) => (
              <label key={f.key} className="block">
                <span className="mb-1 flex items-baseline justify-between gap-2 text-[13px]">
                  <span className="font-medium">{f.label}</span>
                  {f.hint && <span className="truncate text-[11px] text-faint">{f.hint}</span>}
                </span>
                <Select label={f.label} value={(mapping[f.key] as string | null) ?? ''} onChange={(v) => set({ [f.key]: v || null } as Partial<Mapping>)} options={columns} />
              </label>
            ))}
            {mapping.rating && (
              <label className="block">
                <span className="mb-1 block text-[13px] font-medium">Rating scale</span>
                <Select
                  label="Rating scale"
                  value={mapping.rating_scale || 'auto'}
                  onChange={(v) => set({ rating_scale: v })}
                  options={[
                    { value: 'auto', label: 'Work it out from the values' },
                    { value: '5', label: 'Out of 5' },
                    { value: '10', label: 'Out of 10' },
                    { value: '100', label: 'Out of 100' },
                  ]}
                />
              </label>
            )}
          </div>
          <div className="mt-2">
            <Toggle checked={mapping.as_wishlist} onChange={(v) => set({ as_wishlist: v })} label="This is a wishlist" description="Save every row to Watchlist, Play Later or Listen Later instead of recording it as watched or played." />
          </div>

          <h3 className="mb-2 mt-4 text-[13px] font-semibold">First rows</h3>
          <div className="hairline max-h-52 overflow-auto rounded-control">
            <table className="w-full text-left text-[12px]">
              <thead className="sticky top-0 bg-strong text-subtext">
                <tr>
                  {preview.columns.map((c) => (
                    <th key={c} scope="col" className="whitespace-nowrap px-2.5 py-1.5 font-semibold">
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {preview.sample.map((row, i) => (
                  <tr key={i} className="border-t border-white/6">
                    {row.map((cell, j) => (
                      <td key={j} className="max-w-56 truncate whitespace-nowrap px-2.5 py-1.5 text-subtext">
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {!ready && <p className="mt-3 text-[13px] text-warn">Choose the Title column (or an ID column) to continue.</p>}
        </>
      )}
    </Modal>
  );
}
