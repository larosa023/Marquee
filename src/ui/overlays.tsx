// Things that float above the page: modals, menus, toasts.
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useApp } from '@/state/app';
import { Icon, type IconName } from './icons';
import { IconButton, cx } from './primitives';

/** Keeps Tab inside an open dialog and gives focus back to where it was when the dialog closes. */
function useFocusTrap(active: boolean, ref: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    if (!active) return;
    const before = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const focusables = () => Array.from(node?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])') ?? []).filter((el) => el.offsetParent !== null);
    const first = node?.querySelector<HTMLElement>('[data-autofocus]') ?? focusables()[0];
    first?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return;
      const list = focusables();
      if (list.length === 0) return;
      const [a, z] = [list[0], list[list.length - 1]];
      if (e.shiftKey && document.activeElement === a) {
        e.preventDefault();
        z.focus();
      } else if (!e.shiftKey && document.activeElement === z) {
        e.preventDefault();
        a.focus();
      }
    };
    node?.addEventListener('keydown', onKey);
    return () => {
      node?.removeEventListener('keydown', onKey);
      before?.focus?.({ preventScroll: true });
    };
  }, [active, ref]);
}

export function Modal({ open, onClose, title, children, width = 520, footer }: { open: boolean; onClose: () => void; title: string; children: ReactNode; width?: number; footer?: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(open, ref);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, onClose]);
  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div className="fixed inset-0 z-50 grid place-items-center p-6" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.16 }}>
          <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={onClose} />
          <motion.div
            ref={ref}
            role="dialog"
            aria-modal="true"
            aria-label={title}
            className="panel relative flex max-h-[86vh] w-full flex-col"
            style={{ maxWidth: width }}
            initial={{ opacity: 0, y: 18, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 10, scale: 0.98 }}
            transition={{ type: 'spring', stiffness: 420, damping: 34 }}
          >
            <div className="flex items-center justify-between gap-4 px-6 pb-2 pt-5">
              <h2 className="text-lg font-semibold">{title}</h2>
              <IconButton icon="x" label="Close" onClick={onClose} />
            </div>
            <div className="selectable min-h-0 flex-1 overflow-y-auto px-6 pb-5">{children}</div>
            {footer && <div className="flex flex-wrap justify-end gap-2 border-t border-white/8 px-6 py-4">{footer}</div>}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

export interface MenuItem {
  id: string;
  label: string;
  icon?: IconName;
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
  /** A heading, not an action. */
  heading?: boolean;
  run?: () => void;
}

/** A menu at a point on screen (context menus) or under an anchor (dropdowns). Arrow keys, Home/End, Enter, Escape. */
export function Menu({ at, items, onClose, label }: { at: { x: number; y: number } | null; items: MenuItem[]; onClose: () => void; label: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useFocusTrap(!!at, ref);
  useLayoutEffect(() => {
    if (!at || !ref.current) return setPos(null);
    const { width, height } = ref.current.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(at.x, window.innerWidth - width - 8)), top: Math.max(8, Math.min(at.y, window.innerHeight - height - 8)) });
  }, [at, items.length]);
  useEffect(() => {
    if (!at) return;
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== 'Escape') return;
      if (e instanceof MouseEvent && ref.current?.contains(e.target as Node)) return;
      onClose();
    };
    window.addEventListener('mousedown', close, true);
    window.addEventListener('keydown', close, true);
    window.addEventListener('blur', close);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('mousedown', close, true);
      window.removeEventListener('keydown', close, true);
      window.removeEventListener('blur', close);
      window.removeEventListener('resize', close);
    };
  }, [at, onClose]);
  if (!at) return null;
  const move = (dir: 1 | -1 | 'first' | 'last') => {
    const buttons = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button:not([disabled])') ?? []);
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = dir === 'first' ? 0 : dir === 'last' ? buttons.length - 1 : (i + dir + buttons.length) % buttons.length;
    buttons[next]?.focus();
  };
  return createPortal(
    <motion.div
      ref={ref}
      role="menu"
      aria-label={label}
      className="panel fixed z-60 min-w-56 max-w-80 p-1.5"
      style={{ left: pos?.left ?? at.x, top: pos?.top ?? at.y, visibility: pos ? 'visible' : 'hidden' }}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.12 }}
      onKeyDown={(e) => {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          move(1);
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          move(-1);
        }
        if (e.key === 'Home') {
          e.preventDefault();
          move('first');
        }
        if (e.key === 'End') {
          e.preventDefault();
          move('last');
        }
      }}
    >
      {items.map((item) =>
        item.separator ? (
          <div key={item.id} role="separator" className="mx-2 my-1.5 h-px bg-white/10" />
        ) : item.heading ? (
          <div key={item.id} className="px-2.5 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
            {item.label}
          </div>
        ) : (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.run?.();
            }}
            className={cx('flex w-full items-center gap-2.5 rounded-[calc(var(--control-radius)-2px)] px-2.5 py-2 text-left transition-colors hover:bg-hover focus-visible:bg-hover disabled:opacity-45', item.danger && 'text-bad')}
          >
            {item.icon && <Icon name={item.icon} size={16} className={item.danger ? undefined : 'text-subtext'} />}
            <span className="min-w-0 flex-1">
              <span className="block truncate">{item.label}</span>
              {item.hint && <span className="block truncate text-[12px] text-faint">{item.hint}</span>}
            </span>
            {item.checked && <Icon name="check" size={15} className="text-accent" />}
          </button>
        ),
      )}
    </motion.div>,
    document.body,
  );
}

/** A button that opens a menu underneath itself. */
export function MenuButton({ items, label, children }: { items: MenuItem[]; label: string; children: (open: (e: React.MouseEvent<HTMLElement>) => void, isOpen: boolean) => ReactNode }) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      {children((e) => {
        const r = e.currentTarget.getBoundingClientRect();
        setAt(at ? null : { x: r.left, y: r.bottom + 6 });
      }, !!at)}
      <AnimatePresence>{at && <Menu at={at} items={items} onClose={() => setAt(null)} label={label} />}</AnimatePresence>
    </>
  );
}

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  return createPortal(
    <div className="pointer-events-none fixed inset-x-0 bottom-6 z-70 flex flex-col items-center gap-2" role="status" aria-live="polite">
      <AnimatePresence>
        {toasts.map((t) => (
          <motion.div key={t.id} layout initial={{ opacity: 0, y: 24, scale: 0.94 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: 12, scale: 0.96 }} transition={{ type: 'spring', stiffness: 460, damping: 34 }} className="panel pointer-events-auto flex max-w-xl items-center gap-3 py-2.5 pl-3.5 pr-2">
            <Icon name={t.kind === 'good' ? 'check' : t.kind === 'bad' ? 'alert' : 'info'} size={17} className={t.kind === 'good' ? 'text-good' : t.kind === 'bad' ? 'text-bad' : 'text-accent'} />
            <span className="min-w-0 flex-1">{t.text}</span>
            {t.action && (
              <button
                type="button"
                className="rounded-control px-2.5 py-1 font-semibold text-accent hover:bg-hover"
                onClick={() => {
                  t.action?.run();
                  dismiss(t.id);
                }}
              >
                {t.action.label}
              </button>
            )}
            <IconButton icon="x" label="Dismiss" size={15} className="h-7! w-7!" onClick={() => dismiss(t.id)} />
          </motion.div>
        ))}
      </AnimatePresence>
    </div>,
    document.body,
  );
}
