// Appearance: pick a theme, tune accent / motion / density / scale, and edit themes live.
import { useEffect, useMemo, useState } from 'react';
import { api, errorMessage, pickFile, pickSave } from '@/lib/api';
import { contrast } from '@/lib/img';
import { appearanceFrom, applyTheme, resolveTheme, themeVars, type Appearance as AppearanceSettings, type MotionMode } from '@/lib/theme';
import type { Theme, ThemeColors } from '@/lib/types';
import { useApp } from '@/state/app';
import { Icon } from '@/ui/icons';
import { MenuButton, Modal } from '@/ui/overlays';
import { Badge, Button, Field, IconButton, Input, Segmented, Select, Toggle, cx } from '@/ui/primitives';

const COLOR_FIELDS: { key: keyof ThemeColors; label: string; hint: string }[] = [
  { key: 'background', label: 'Background', hint: 'The page behind everything' },
  { key: 'sidebar', label: 'Sidebar', hint: 'Sidebar and title bar' },
  { key: 'surface', label: 'Surface', hint: 'Cards, panels, inputs' },
  { key: 'surfaceHover', label: 'Surface (hover)', hint: 'A surface under the pointer' },
  { key: 'surfaceStrong', label: 'Surface (strong)', hint: 'Selected and raised controls' },
  { key: 'text', label: 'Text', hint: 'Titles and body text' },
  { key: 'subtext', label: 'Secondary text', hint: 'Descriptions and details' },
  { key: 'faint', label: 'Faint text', hint: 'Hints and timestamps' },
  { key: 'accent', label: 'Accent', hint: 'Buttons, rings and highlights' },
];
const FONTS = ['Inter Variable', 'Segoe UI Variable Text', 'Segoe UI Variable Display', 'Bahnschrift', 'Cascadia Code', 'Georgia'];
const isHex = (v: string) => /^#[0-9a-f]{6}$/i.test(v);

function Swatch({ theme, systemAccent }: { theme: Theme; systemAccent: string | null }) {
  const vars = themeVars(theme, null, systemAccent);
  return (
    <span className="relative block aspect-video overflow-hidden rounded-[10px]" style={{ background: theme.colors.background }} aria-hidden="true">
      <span className="absolute inset-y-0 left-0 w-1/5" style={{ background: theme.colors.sidebar }} />
      <span className="absolute left-[27%] top-[18%] h-1.5 w-2/5 rounded-full" style={{ background: theme.colors.text }} />
      <span className="absolute left-[27%] top-[32%] h-1 w-1/4 rounded-full" style={{ background: theme.colors.subtext }} />
      {[0, 1, 2].map((i) => (
        <span key={i} className="absolute bottom-[14%] h-[34%] w-[17%] rounded-[4px]" style={{ left: `${27 + i * 21}%`, background: i === 0 ? vars['--accent'] : theme.colors.surfaceStrong }} />
      ))}
    </span>
  );
}

function ColorField({ label, hint, value, onChange, allowWindows }: { label: string; hint: string; value: string; onChange: (v: string) => void; allowWindows?: boolean }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  const windows = value === 'windows';
  return (
    <div className="flex items-center gap-3 py-1.5">
      <input type="color" aria-label={`${label} color`} disabled={windows} value={isHex(value) ? value : '#888888'} onChange={(e) => onChange(e.target.value.toUpperCase())} className="h-9 w-11 shrink-0 cursor-pointer rounded-md border-0 bg-transparent p-0 disabled:opacity-40" />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium">{label}</div>
        <div className="truncate text-[12px] text-faint">{hint}</div>
      </div>
      {allowWindows && (
        <button type="button" aria-pressed={windows} onClick={() => onChange(windows ? '#F2C14E' : 'windows')} className={cx('rounded-md px-2 py-1 text-[12px]', windows ? 'bg-accent text-on-accent' : 'hairline text-subtext hover:bg-hover')}>
          Windows accent
        </button>
      )}
      <input
        aria-label={`${label} hex value`}
        value={windows ? 'windows' : text}
        disabled={windows}
        spellCheck={false}
        onChange={(e) => {
          const v = e.target.value.trim();
          setText(v);
          if (isHex(v)) onChange(v.toUpperCase());
        }}
        onBlur={() => setText(value)}
        className="hairline tnum h-8 w-24 rounded-control bg-surface px-2 text-[13px] uppercase disabled:opacity-50"
      />
    </div>
  );
}

function Slider({ label, value, min, max, step, onChange, format }: { label: string; value: number; min: number; max: number; step: number; onChange: (v: number) => void; format: (v: number) => string }) {
  return (
    <label className="flex items-center gap-3 py-1.5">
      <span className="w-40 shrink-0 text-[13px] font-medium">{label}</span>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} aria-valuetext={format(value)} className="h-2 flex-1" />
      <span className="tnum w-14 text-right text-[13px] text-subtext">{format(value)}</span>
    </label>
  );
}

/** Text contrast against the theme's own surfaces, so a custom theme can't quietly become unreadable. */
function ContrastReport({ theme }: { theme: Theme }) {
  const c = theme.colors;
  if (![c.background, c.surface, c.text, c.subtext, c.faint].every(isHex)) return null;
  const rows = [
    { name: 'Text', fg: c.text },
    { name: 'Secondary text', fg: c.subtext },
    { name: 'Faint text', fg: c.faint },
  ].map((r) => ({ ...r, ratio: Math.min(contrast(r.fg, c.background), contrast(r.fg, c.surface)) }));
  const ok = rows.every((r) => r.ratio >= 4.5);
  return (
    <div className={cx('mt-3 rounded-control p-3 text-[13px]', ok ? 'bg-good/10' : 'bg-warn/12')} role="status">
      <div className={cx('flex items-center gap-2 font-medium', ok ? 'text-good' : 'text-warn')}>
        <Icon name={ok ? 'check' : 'alert'} size={15} />
        {ok ? 'Readable: all text meets WCAG AA (4.5:1)' : 'Some text is hard to read on this background'}
      </div>
      <ul className="tnum mt-1.5 space-y-0.5 text-subtext">
        {rows.map((r) => (
          <li key={r.name} className="flex justify-between">
            <span>{r.name}</span>
            <span className={r.ratio >= 4.5 ? undefined : 'font-semibold text-warn'}>{r.ratio.toFixed(1)}:1</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function ThemeEditor({ initial, onClose }: { initial: Theme; onClose: (saved: Theme | null) => void }) {
  const toast = useApp((s) => s.toast);
  const sys = useApp((s) => s.sys);
  const settings = useApp((s) => s.boot?.settings);
  const [draft, setDraft] = useState<Theme>(initial);
  const appearance = useMemo(() => appearanceFrom(settings ?? {}), [settings]);

  // Live preview: the whole app wears the draft while the editor is open.
  useEffect(() => {
    applyTheme({ ...draft, id: `${draft.id}-preview` }, { ...appearance, accent: null }, sys?.accent ?? null);
  }, [draft, appearance, sys]);

  const setColor = (key: keyof ThemeColors, v: string) => setDraft({ ...draft, colors: { ...draft.colors, [key]: v } });
  const setM = (patch: Partial<Theme['marquee']>) => setDraft({ ...draft, marquee: { ...draft.marquee, ...patch } });
  const save = async () => {
    try {
      const saved = await api<Theme>('theme.save', { theme: draft });
      toast(`Saved ${saved.name}`, 'good');
      onClose(saved);
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };

  return (
    <Modal
      open
      onClose={() => onClose(null)}
      title="Theme editor"
      width={880}
      footer={
        <>
          <span className="mr-auto self-center text-[12px] text-faint">Changes preview across the whole app as you make them.</span>
          <Button onClick={() => onClose(null)}>Cancel</Button>
          <Button variant="primary" icon="check" disabled={!draft.name.trim()} onClick={() => void save()}>
            Save theme
          </Button>
        </>
      }
    >
      <div className="grid gap-x-8 gap-y-2 md:grid-cols-2">
        <div>
          <label className="block">
            <span className="mb-1 block text-[13px] font-medium">Name</span>
            <Input value={draft.name} maxLength={40} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </label>
          <label className="mt-3 block">
            <span className="mb-1 block text-[13px] font-medium">Description</span>
            <Input value={draft.description} maxLength={120} onChange={(e) => setDraft({ ...draft, description: e.target.value })} />
          </label>
          <div className="mt-3 flex flex-wrap gap-4">
            <div>
              <span className="mb-1 block text-[13px] font-medium">Mode</span>
              <Segmented
                label="Light or dark"
                size="sm"
                value={draft.mode}
                onChange={(mode) => setDraft({ ...draft, mode })}
                options={[
                  { value: 'dark', label: 'Dark' },
                  { value: 'light', label: 'Light' },
                ]}
              />
            </div>
            <div>
              <span className="mb-1 block text-[13px] font-medium">Window backdrop</span>
              <Segmented
                label="Window backdrop"
                size="sm"
                value={draft.backdrop}
                onChange={(backdrop) => setDraft({ ...draft, backdrop })}
                options={[
                  { value: 'mica', label: 'Mica' },
                  { value: 'acrylic', label: 'Acrylic' },
                  { value: 'none', label: 'Solid' },
                ]}
              />
            </div>
          </div>
          <h3 className="mb-1 mt-5 text-[13px] font-semibold uppercase tracking-wider text-faint">Colors</h3>
          {COLOR_FIELDS.map((f) => (
            <ColorField key={f.key} label={f.label} hint={f.hint} value={draft.colors[f.key]} onChange={(v) => setColor(f.key, v)} allowWindows={f.key === 'accent'} />
          ))}
          <ContrastReport theme={draft} />
        </div>
        <div>
          <h3 className="mb-1 text-[13px] font-semibold uppercase tracking-wider text-faint">Shape and depth</h3>
          <Slider label="Card corners" value={draft.marquee.radius} min={0} max={28} step={1} onChange={(v) => setM({ radius: v })} format={(v) => `${v}px`} />
          <Slider label="Control corners" value={draft.marquee.controlRadius} min={0} max={20} step={1} onChange={(v) => setM({ controlRadius: v })} format={(v) => `${v}px`} />
          <Slider label="Panel blur" value={draft.marquee.blur} min={0} max={48} step={2} onChange={(v) => setM({ blur: v })} format={(v) => `${v}px`} />
          <Slider label="Shadows" value={draft.marquee.shadow} min={0} max={1} step={0.05} onChange={(v) => setM({ shadow: v })} format={(v) => `${Math.round(v * 100)}%`} />
          <Slider label="Hover glow" value={draft.marquee.glow} min={0} max={1} step={0.05} onChange={(v) => setM({ glow: v })} format={(v) => `${Math.round(v * 100)}%`} />
          <Slider label="Sidebar see-through" value={draft.transparency} min={0} max={0.6} step={0.02} onChange={(v) => setDraft({ ...draft, transparency: v })} format={(v) => `${Math.round(v * 100)}%`} />
          <Slider label="Surface opacity" value={draft.surfaceOpacity} min={0.5} max={1} step={0.02} onChange={(v) => setDraft({ ...draft, surfaceOpacity: v })} format={(v) => `${Math.round(v * 100)}%`} />
          <Slider label="Motion intensity" value={draft.marquee.motion} min={0} max={1.5} step={0.1} onChange={(v) => setM({ motion: v })} format={(v) => `${Math.round(v * 100)}%`} />
          <h3 className="mb-1 mt-5 text-[13px] font-semibold uppercase tracking-wider text-faint">Type</h3>
          <Field label="Text font">
            <Select label="Text font" className="w-56" value={draft.marquee.font} onChange={(font) => setM({ font })} options={[...new Set([draft.marquee.font, ...FONTS])].map((f) => ({ value: f, label: f }))} />
          </Field>
          <Field label="Heading font">
            <Select label="Heading font" className="w-56" value={draft.marquee.displayFont} onChange={(displayFont) => setM({ displayFont })} options={[...new Set([draft.marquee.displayFont, ...FONTS])].map((f) => ({ value: f, label: f }))} />
          </Field>
          <div className="hairline mt-4 rounded-card bg-surface p-4">
            <div className="display text-xl font-bold">The quick brown fox</div>
            <p className="mt-1 text-subtext">Secondary text looks like this, with a date in tabular figures:</p>
            <p className="tnum mt-1 text-[13px] text-faint">Nov 13, 2026 · 2 h 14 m · 87% match</p>
            <div className="mt-3 flex gap-2">
              <Button variant="primary" size="sm">
                Primary
              </Button>
              <Button size="sm">Secondary</Button>
              <Badge tone="accent">Badge</Badge>
            </div>
          </div>
        </div>
      </div>
    </Modal>
  );
}

export function AppearanceSection() {
  const boot = useApp((s) => s.boot)!;
  const sys = useApp((s) => s.sys);
  const toast = useApp((s) => s.toast);
  const setSetting = useApp((s) => s.setSetting);
  const pendingFile = useApp((s) => s.pendingFile);
  const a = useMemo(() => appearanceFrom(boot.settings), [boot.settings]);
  const [editing, setEditing] = useState<Theme | null>(null);
  const [deleting, setDeleting] = useState<Theme | null>(null);
  const suite: Theme | null = boot.settings['hangar.theme'] ?? null;
  const followingSuite = a.useSuiteTheme && !!sys?.hangar_installed && !!suite;
  const set = (patch: Partial<AppearanceSettings>) => void setSetting('appearance', { ...a, ...patch });

  const importTheme = async (path: string) => {
    try {
      const t = await api<Theme>('theme.import', { path });
      await useApp.getState().refreshBoot();
      set({ themeId: t.id, useSuiteTheme: false });
      toast(`Imported ${t.name}`, 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  // A .marqueetheme file double-clicked in Explorer lands here.
  useEffect(() => {
    if (pendingFile?.endsWith('.marqueetheme')) {
      useApp.getState().set({ pendingFile: null });
      void importTheme(pendingFile);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingFile]);

  const restore = () => {
    const prefersLight = window.matchMedia?.('(prefers-color-scheme: light)').matches ?? false;
    const fresh = useApp.getState().boot!;
    const ap = appearanceFrom(fresh.settings);
    applyTheme(resolveTheme(ap, fresh.themes, fresh.system_light, fresh.settings['hangar.theme'] ?? null, !!sys?.hangar_installed, prefersLight), ap, sys?.accent ?? null);
  };

  const duplicate = async (t: Theme, edit: boolean) => {
    try {
      const copy = await api<Theme>('theme.duplicate', { id: t.id });
      await useApp.getState().refreshBoot();
      if (edit) setEditing(copy);
      else toast(`Made ${copy.name}`, 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };
  const exportTheme = async (t: Theme) => {
    const path = await pickSave(`${t.name}.marqueetheme`, [{ name: 'Marquee theme', extensions: ['marqueetheme'] }]);
    if (!path) return;
    try {
      await api('theme.export', { id: t.id, path });
      toast(`Exported ${t.name}`, 'good');
    } catch (e) {
      toast(errorMessage(e), 'bad');
    }
  };

  return (
    <>
      {sys?.hangar_installed && (
        <div className="hairline mb-5 rounded-card bg-surface px-4">
          <Toggle checked={a.useSuiteTheme} onChange={(v) => set({ useSuiteTheme: v })} label="Use suite theme" description={suite ? `Follow whatever theme Hangar sets for all your apps. Right now: ${suite.name}.` : "Follow Hangar's theme. Hangar hasn't sent one yet; Marquee uses the theme below until it does."} />
        </div>
      )}

      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-[15px] font-semibold">Theme</h3>
        <Button
          size="sm"
          icon="upload"
          onClick={async () => {
            const path = await pickFile([{ name: 'Marquee theme', extensions: ['marqueetheme', 'json'] }]);
            if (path) void importTheme(path);
          }}
        >
          Import a theme…
        </Button>
      </div>
      <ul className={cx('grid gap-4', followingSuite && 'opacity-60')} style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(190px, 1fr))' }} role="radiogroup" aria-label="Theme">
        {boot.themes.map((t) => {
          const selected = !followingSuite && a.themeId === t.id;
          return (
            <li key={t.id} className={cx('hairline group relative rounded-card bg-surface p-2 transition-colors', selected && 'border-accent! ring-1 ring-accent')}>
              <button type="button" role="radio" aria-checked={selected} data-theme-option={t.id} className="block w-full rounded-[10px] text-left" onClick={() => set({ themeId: t.id, useSuiteTheme: false })}>
                <Swatch theme={t} systemAccent={sys?.accent ?? null} />
                <span className="mt-2 flex items-center gap-1.5 px-1 font-semibold">
                  {selected && <Icon name="check" size={14} className="text-accent" />}
                  <span className="truncate">{t.name}</span>
                </span>
                <span className="clamp-2 mt-0.5 block min-h-9 px-1 text-[12px] text-subtext">{t.description}</span>
              </button>
              <div className="absolute right-3 top-3">
                <MenuButton
                  label={`Actions for ${t.name}`}
                  items={[
                    ...(t.builtin ? [] : [{ id: 'edit', label: 'Edit', icon: 'palette' as const, run: () => setEditing(t) }]),
                    { id: 'dup', label: t.builtin ? 'Duplicate and edit' : 'Duplicate', icon: 'copy' as const, run: () => void duplicate(t, t.builtin) },
                    { id: 'export', label: 'Export…', icon: 'download' as const, run: () => void exportTheme(t) },
                    ...(t.builtin
                      ? []
                      : [
                          { id: 'sep', label: '', separator: true },
                          { id: 'del', label: 'Delete', icon: 'trash' as const, danger: true, run: () => setDeleting(t) },
                        ]),
                  ]}
                >
                  {(open, isOpen) => <IconButton icon="more" label={`Actions for ${t.name}`} size={16} className="h-7! w-7! bg-black/45! text-white! opacity-0 focus-visible:opacity-100 group-hover:opacity-100 aria-expanded:opacity-100" onClick={open} aria-haspopup="menu" aria-expanded={isOpen} />}
                </MenuButton>
              </div>
            </li>
          );
        })}
      </ul>
      <p className="mt-2 text-[12px] text-faint">Built-in themes can't be changed; duplicate one to make it yours. Custom themes are .marqueetheme files in your data folder.</p>

      <div className="mt-6 divide-y divide-white/8">
        <Field label="Accent color" description="Override the theme's accent. Detail pages still take their accent from the artwork.">
          <div className="flex items-center gap-2">
            <input type="color" aria-label="Accent color" value={a.accent ?? '#F2C14E'} onChange={(e) => set({ accent: e.target.value.toUpperCase() })} className="h-9 w-11 cursor-pointer rounded-md border-0 bg-transparent p-0" />
            <Button size="sm" disabled={!a.accent} onClick={() => set({ accent: null })}>
              Use the theme's
            </Button>
          </div>
        </Field>
        <Field label="Motion" description="Full: every transition. Reduced: fades only, no movement. Off: no animation. Windows' own 'Animation effects' setting is always respected.">
          <Segmented
            label="Motion"
            value={a.motion}
            onChange={(motion: MotionMode) => set({ motion })}
            options={[
              { value: 'full', label: 'Full' },
              { value: 'reduced', label: 'Reduced' },
              { value: 'off', label: 'Off' },
            ]}
          />
        </Field>
        <Field label="Density" description="How large cards are and how much space sits between them.">
          <Segmented
            label="Density"
            value={a.density}
            onChange={(density) => set({ density })}
            options={[
              { value: 'compact', label: 'Compact' },
              { value: 'comfortable', label: 'Comfortable' },
              { value: 'spacious', label: 'Spacious' },
            ]}
          />
        </Field>
        <Field label="UI scale" description="Makes everything larger or smaller, from 90% to 150%.">
          <div className="flex items-center gap-3">
            <input type="range" min={0.9} max={1.5} step={0.05} value={a.scale} aria-label="UI scale" aria-valuetext={`${Math.round(a.scale * 100)}%`} onChange={(e) => set({ scale: Number(e.target.value) })} className="h-2 w-48" />
            <span className="tnum w-12 text-right text-subtext">{Math.round(a.scale * 100)}%</span>
            <Button size="sm" disabled={a.scale === 1} onClick={() => set({ scale: 1 })}>
              Reset
            </Button>
          </div>
        </Field>
      </div>

      {editing && (
        <ThemeEditor
          initial={editing}
          onClose={(saved) => {
            setEditing(null);
            if (saved) {
              void useApp
                .getState()
                .refreshBoot()
                .then(() => set({ themeId: saved.id, useSuiteTheme: false }));
            } else restore();
          }}
        />
      )}
      <Modal
        open={!!deleting}
        onClose={() => setDeleting(null)}
        title={`Delete ${deleting?.name}?`}
        width={420}
        footer={
          <>
            <Button onClick={() => setDeleting(null)}>Keep it</Button>
            <Button
              variant="danger"
              icon="trash"
              onClick={() =>
                void api('theme.delete', { id: deleting!.id })
                  .then(async () => {
                    if (a.themeId === deleting!.id) set({ themeId: 'marquee' });
                    setDeleting(null);
                    await useApp.getState().refreshBoot();
                  })
                  .catch((e) => toast(errorMessage(e), 'bad'))
              }
            >
              Delete theme
            </Button>
          </>
        }
      >
        <p className="text-subtext">The theme file is removed from your data folder. Export it first if you might want it back.</p>
      </Modal>
    </>
  );
}
