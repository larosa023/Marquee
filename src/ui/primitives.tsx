// Shared building blocks: buttons, inputs, toggles, badges, skeletons and the "designed states".
import { motion } from 'framer-motion';
import { forwardRef, useId, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from 'react';
import { Icon, type IconName } from './icons';

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(' ');
}

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'glass';
  size?: 'sm' | 'md' | 'lg';
  icon?: IconName;
  busy?: boolean;
};

const buttonVariants = {
  primary: 'bg-accent text-on-accent font-semibold hover:brightness-110',
  secondary: 'bg-strong text-text hover:bg-hover hairline',
  ghost: 'bg-transparent text-subtext hover:bg-hover hover:text-text',
  danger: 'bg-transparent text-bad hairline hover:bg-bad/10',
  glass: 'bg-black/35 text-white backdrop-blur-md border border-white/20 hover:bg-black/50',
};
const buttonSizes = { sm: 'h-8 px-3 text-[13px] gap-1.5', md: 'h-9.5 px-4 gap-2', lg: 'h-11.5 px-5.5 text-[15px] gap-2.5' };

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button({ variant = 'secondary', size = 'md', icon, busy, className, children, disabled, ...rest }, ref) {
  return (
    <button
      ref={ref}
      type="button"
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      className={cx('inline-flex shrink-0 items-center justify-center whitespace-nowrap rounded-control transition-[background,filter,transform,opacity] duration-150 active:scale-[0.97] disabled:opacity-50 disabled:active:scale-100', buttonVariants[variant], buttonSizes[size], className)}
      {...rest}
    >
      {icon && <Icon name={icon} size={size === 'sm' ? 15 : 17} className={busy ? 'opacity-40' : undefined} />}
      {children}
    </button>
  );
});

type IconButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string; size?: number; active?: boolean; filled?: boolean };

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton({ icon, label, size = 18, active, filled, className, ...rest }, ref) {
  return (
    <button ref={ref} type="button" aria-label={label} title={label} className={cx('grid h-9 w-9 shrink-0 place-items-center rounded-control transition-colors duration-150 disabled:opacity-40', active ? 'bg-strong text-accent' : 'text-subtext hover:bg-hover hover:text-text', className)} {...rest}>
      <Icon name={icon} size={size} filled={filled} />
    </button>
  );
});

export function Toggle({ checked, onChange, label, description, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; description?: ReactNode; disabled?: boolean }) {
  const id = useId();
  return (
    <div className={cx('flex items-center justify-between gap-6 py-2.5', disabled && 'opacity-50')}>
      <label htmlFor={id} className="min-w-0">
        <div id={`${id}-label`} className="font-medium">
          {label}
        </div>
        {description && (
          <div id={`${id}-about`} className="mt-0.5 text-[13px] text-subtext">
            {description}
          </div>
        )}
      </label>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={`${id}-label`}
        aria-describedby={description ? `${id}-about` : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cx('relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200', checked ? 'bg-accent' : 'bg-strong')}
      >
        <motion.span layout transition={{ type: 'spring', stiffness: 600, damping: 34 }} className={cx('absolute top-0.5 h-5 w-5 rounded-full shadow', checked ? 'right-0.5 bg-on-accent' : 'left-0.5 bg-subtext')} />
      </button>
    </div>
  );
}

export function Field({ label, description, children, row = true }: { label: string; description?: ReactNode; children: ReactNode; row?: boolean }) {
  return (
    <div className={cx('py-2.5', row ? 'flex items-center justify-between gap-6' : 'space-y-2')}>
      <div className="min-w-0">
        <div className="font-medium">{label}</div>
        {description && <div className="mt-0.5 text-[13px] text-subtext">{description}</div>}
      </div>
      <div className={row ? 'shrink-0' : undefined}>{children}</div>
    </div>
  );
}

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { icon?: IconName }>(function Input({ icon, className, ...rest }, ref) {
  return (
    <div className={cx('relative', className)}>
      {icon && <Icon name={icon} size={16} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" />}
      <input ref={ref} className={cx('hairline h-9.5 w-full rounded-control bg-surface px-3 placeholder:text-faint focus:border-accent', icon && 'pl-9')} {...rest} />
    </div>
  );
});

export function Select<T extends string>({ value, onChange, options, label, className }: { value: T; onChange: (v: T) => void; options: { value: T; label: string }[]; label: string; className?: string }) {
  return (
    <div className={cx('relative', className)}>
      <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value as T)} className="hairline h-9.5 w-full appearance-none rounded-control bg-surface pl-3 pr-9">
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={15} className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-faint" />
    </div>
  );
}

/** A row of mutually exclusive options (view switchers, motion mode...). Arrow keys move between them. */
export function Segmented<T extends string>({ value, onChange, options, label, size = 'md' }: { value: T; onChange: (v: T) => void; options: { value: T; label: string; icon?: IconName }[]; label: string; size?: 'sm' | 'md' }) {
  const id = useId();
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="hairline inline-flex rounded-control bg-surface p-0.5"
      onKeyDown={(e) => {
        const i = options.findIndex((o) => o.value === value);
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') onChange(options[(i + 1) % options.length].value);
        if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') onChange(options[(i - 1 + options.length) % options.length].value);
      }}
    >
      {options.map((o) => {
        const selected = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(o.value)}
            className={cx('relative inline-flex items-center gap-1.5 rounded-[calc(var(--control-radius)-3px)] px-3 font-medium transition-colors', size === 'sm' ? 'h-7 text-[13px]' : 'h-8.5', selected ? 'text-text' : 'text-subtext hover:text-text')}
          >
            {selected && <motion.span layoutId={`seg-${id}`} className="absolute inset-0 rounded-[inherit] bg-strong" transition={{ type: 'spring', stiffness: 500, damping: 38 }} />}
            <span className="relative inline-flex items-center gap-1.5">
              {o.icon && <Icon name={o.icon} size={15} />}
              {o.label}
            </span>
          </button>
        );
      })}
    </div>
  );
}

export function Chip({ selected, onClick, children, icon }: { selected?: boolean; onClick?: () => void; children: ReactNode; icon?: IconName }) {
  return (
    <button type="button" aria-pressed={selected} onClick={onClick} className={cx('inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full px-3 text-[13px] font-medium transition-colors', selected ? 'bg-accent text-on-accent' : 'hairline bg-surface text-subtext hover:bg-hover hover:text-text')}>
      {icon && <Icon name={icon} size={14} />}
      {children}
    </button>
  );
}

export function Badge({ children, tone = 'neutral' }: { children: ReactNode; tone?: 'neutral' | 'accent' | 'good' | 'warn' | 'bad' }) {
  const tones = { neutral: 'bg-strong text-subtext', accent: 'bg-accent/15 text-accent', good: 'bg-good/15 text-good', warn: 'bg-warn/15 text-warn', bad: 'bg-bad/15 text-bad' };
  return <span className={cx('inline-flex h-5.5 items-center rounded-md px-1.5 text-[11px] font-semibold uppercase tracking-wide', tones[tone])}>{children}</span>;
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="hairline inline-flex h-5.5 min-w-5.5 items-center justify-center rounded-md bg-surface px-1.5 font-sans text-[11px] font-medium text-subtext">{children}</kbd>;
}

export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden="true" className={cx('skeleton', className)} />;
}

/** What a screen shows when there's nothing to show. Never a blank page. */
export function EmptyState({ icon, title, children, action }: { icon: IconName; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} className="mx-auto flex max-w-md flex-col items-center px-6 py-16 text-center">
      <div className="mb-5 grid h-16 w-16 place-items-center rounded-2xl bg-accent/12 text-accent">
        <Icon name={icon} size={30} />
      </div>
      <h2 className="text-xl font-semibold">{title}</h2>
      {children && <div className="mt-2 text-subtext">{children}</div>}
      {action && <div className="mt-6 flex flex-wrap justify-center gap-2">{action}</div>}
    </motion.div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const e = error as { kind?: string; message?: string } | undefined;
  const offline = e?.kind === 'offline';
  return (
    <EmptyState
      icon={offline ? 'wifiOff' : 'alert'}
      title={offline ? "You're offline" : "That didn't load"}
      action={
        onRetry && (
          <Button icon="sync" onClick={onRetry}>
            Try again
          </Button>
        )
      }
    >
      {offline ? 'Marquee shows everything it has saved. New releases and details will fill in when you reconnect.' : (e?.message ?? 'Something went wrong.')}
    </EmptyState>
  );
}

/** The slim banner shown at the top of a page while offline. */
export function OfflineBanner() {
  return (
    <div role="status" className="mx-(--pad) mb-4 flex items-center gap-2.5 rounded-control bg-warn/12 px-3.5 py-2 text-[13px] text-warn">
      <Icon name="wifiOff" size={16} />
      <span>You're offline. Showing what's saved; anything you add or rate will sync when you're back.</span>
    </div>
  );
}

export function SectionTitle({ children, hint }: { children: ReactNode; hint?: ReactNode }) {
  return (
    <div className="mb-3 flex items-baseline justify-between gap-4">
      <h2 className="text-[19px] font-semibold">{children}</h2>
      {hint && <div className="text-[13px] text-faint">{hint}</div>}
    </div>
  );
}

export function PageHeader({ title, subtitle, children }: { title: string; subtitle?: ReactNode; children?: ReactNode }) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-4 px-(--pad) pb-5 pt-2">
      <div className="min-w-0">
        <h1 className="display text-[30px] font-bold leading-tight">{title}</h1>
        {subtitle && <p className="mt-1 text-subtext">{subtitle}</p>}
      </div>
      {children && <div className="flex flex-wrap items-center gap-2">{children}</div>}
    </header>
  );
}
