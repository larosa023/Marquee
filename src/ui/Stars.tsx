// Five stars with halves. Click or drag across; arrow keys change by half a star; 0 clears.
import { motion } from 'framer-motion';
import { useState } from 'react';
import { cx } from './primitives';

const STAR = 'M12 2.8l2.8 5.9 6.4.9-4.6 4.5 1.1 6.4L12 17.5l-5.7 3 1.1-6.4-4.6-4.5 6.4-.9z';

export function Stars({ value, onChange, size = 28, label = 'Your rating', readOnly }: { value: number | null; onChange?: (stars: number) => void; size?: number; label?: string; readOnly?: boolean }) {
  const [hover, setHover] = useState<number | null>(null);
  const shown = hover ?? value ?? 0;
  const set = (v: number) => onChange?.(Math.max(0, Math.min(5, v)));
  return (
    <div
      role="slider"
      tabIndex={readOnly ? -1 : 0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={5}
      aria-valuenow={value ?? 0}
      aria-valuetext={value ? `${value} out of 5 stars` : 'Not rated'}
      aria-readonly={readOnly}
      className={cx('inline-flex items-center rounded-control', !readOnly && 'cursor-pointer')}
      onMouseLeave={() => setHover(null)}
      onKeyDown={(e) => {
        if (readOnly) return;
        if (e.key === 'ArrowRight' || e.key === 'ArrowUp') {
          e.preventDefault();
          set((value ?? 0) + 0.5);
        }
        if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') {
          e.preventDefault();
          set((value ?? 0) - 0.5);
        }
        if (e.key === 'Home' || e.key === 'Backspace' || e.key === 'Delete' || e.key === '0') {
          e.preventDefault();
          set(0);
        }
        if (e.key === 'End') {
          e.preventDefault();
          set(5);
        }
        if (/^[1-5]$/.test(e.key)) {
          e.preventDefault();
          set(Number(e.key));
        }
      }}
    >
      {[1, 2, 3, 4, 5].map((n) => {
        const fill = shown >= n ? 1 : shown >= n - 0.5 ? 0.5 : 0;
        return (
          <motion.span
            key={n}
            className="relative block"
            style={{ width: size, height: size }}
            whileTap={readOnly ? undefined : { scale: 0.85 }}
            onMouseMove={(e) => {
              if (readOnly) return;
              const r = e.currentTarget.getBoundingClientRect();
              setHover(e.clientX - r.left < r.width / 2 ? n - 0.5 : n);
            }}
            onClick={(e) => {
              if (readOnly) return;
              const r = e.currentTarget.getBoundingClientRect();
              const v = e.clientX - r.left < r.width / 2 ? n - 0.5 : n;
              // Clicking the current rating again clears it.
              set(v === value ? 0 : v);
            }}
          >
            <svg viewBox="0 0 24 24" width={size} height={size} className="absolute inset-0 text-strong">
              <path d={STAR} fill="currentColor" />
            </svg>
            <svg viewBox="0 0 24 24" width={size} height={size} className="absolute inset-0 text-accent transition-[clip-path] duration-100" style={{ clipPath: `inset(0 ${(1 - fill) * 100}% 0 0)` }}>
              <path d={STAR} fill="currentColor" />
            </svg>
          </motion.span>
        );
      })}
    </div>
  );
}
