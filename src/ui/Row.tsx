// A horizontal row of cards: drag or wheel to scroll with momentum, arrow keys to move between cards.
import { animate, motion, useMotionValue } from 'framer-motion';
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { Card as CardData } from '@/lib/types';
import { CardSkeleton, PosterCard } from './Card';
import { IconButton } from './primitives';

interface RowProps {
  id: string;
  title: string;
  subtitle?: string;
  cards: CardData[];
  onContext?: (card: CardData, at: { x: number; y: number }) => void;
  action?: ReactNode;
}

export function Row({ id, title, subtitle, cards, onContext, action }: RowProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const x = useMotionValue(0);
  const [max, setMax] = useState(0);
  const [pos, setPos] = useState(0);
  const dragged = useRef(false);

  useLayoutEffect(() => {
    const measure = () => {
      if (!viewport.current || !track.current) return;
      const m = Math.max(0, track.current.scrollWidth - viewport.current.clientWidth);
      setMax(m);
      if (-x.get() > m) x.set(-m);
    };
    measure();
    const ro = new ResizeObserver(measure);
    if (viewport.current) ro.observe(viewport.current);
    if (track.current) ro.observe(track.current);
    return () => ro.disconnect();
  }, [cards.length, x]);

  useEffect(() => x.on('change', (v) => setPos(-v)), [x]);

  const glide = useCallback(
    (to: number) => {
      animate(x, Math.min(0, Math.max(-max, to)), { type: 'spring', stiffness: 220, damping: 32, mass: 0.9 });
    },
    [max, x],
  );
  const page = (dir: 1 | -1) => glide(x.get() - dir * (viewport.current?.clientWidth ?? 600) * 0.85);

  // Trackpads and shift+wheel scroll the row; a plain vertical wheel is left for the page.
  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const dx = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.shiftKey ? e.deltaY : 0;
      if (!dx || max === 0) return;
      e.preventDefault();
      x.set(Math.min(0, Math.max(-max, x.get() - dx)));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [max, x]);

  // Arrow keys move focus card to card and keep the focused card in view.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft' && e.key !== 'Home' && e.key !== 'End') return;
    const buttons = Array.from(track.current?.querySelectorAll<HTMLButtonElement>('[data-card] > div > button:first-child') ?? []);
    const current = buttons.findIndex((b) => b === document.activeElement || b.parentElement?.contains(document.activeElement));
    if (current < 0) return;
    e.preventDefault();
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : Math.min(buttons.length - 1, Math.max(0, current + (e.key === 'ArrowRight' ? 1 : -1)));
    buttons[next]?.focus({ preventScroll: true });
  };
  const onFocus = (e: React.FocusEvent) => {
    const card = (e.target as HTMLElement).closest<HTMLElement>('[data-card]');
    if (!card || !viewport.current) return;
    const left = card.offsetLeft;
    const right = left + card.offsetWidth;
    const view = viewport.current.clientWidth;
    if (left < -x.get()) glide(-(left - 24));
    else if (right > -x.get() + view) glide(-(right - view + 24));
  };

  return (
    <section aria-label={title} className="mb-9">
      <div className="mb-3 flex items-end justify-between gap-4 px-(--pad)">
        <div className="min-w-0">
          <h2 className="text-[19px] font-semibold">{title}</h2>
          {subtitle && <p className="mt-0.5 truncate text-[13px] text-faint">{subtitle}</p>}
        </div>
        <div className="flex items-center gap-1">
          {action}
          <IconButton icon="chevronLeft" label={`Scroll ${title} back`} disabled={pos <= 2} onClick={() => page(-1)} />
          <IconButton icon="chevronRight" label={`Scroll ${title} forward`} disabled={pos >= max - 2} onClick={() => page(1)} />
        </div>
      </div>
      <div ref={viewport} className="overflow-hidden px-(--pad) py-2" onKeyDown={onKeyDown} onFocus={onFocus}>
        <motion.div
          ref={track}
          className="flex w-max gap-(--gap)"
          style={{ x }}
          drag={max > 0 ? 'x' : false}
          dragConstraints={{ left: -max, right: 0 }}
          dragElastic={0.06}
          dragTransition={{ power: 0.32, timeConstant: 280, bounceStiffness: 260, bounceDamping: 36 }}
          onDragStart={() => (dragged.current = true)}
          onDragEnd={() => setTimeout(() => (dragged.current = false), 60)}
          // A drag that ends over a card shouldn't open it.
          onClickCapture={(e) => {
            if (dragged.current) {
              e.stopPropagation();
              e.preventDefault();
            }
          }}
          initial="hidden"
          whileInView="show"
          viewport={{ once: true, margin: '120px' }}
          variants={{ show: { transition: { staggerChildren: 0.035 } } }}
        >
          {cards.map((c) => (
            <PosterCard key={c.id} card={c} origin={id} onContext={onContext} />
          ))}
        </motion.div>
      </div>
    </section>
  );
}

export function RowSkeleton({ titleWidth = 180 }: { titleWidth?: number }) {
  return (
    <div className="mb-9" aria-hidden="true">
      <div className="skeleton mx-(--pad) mb-4 h-5" style={{ width: titleWidth }} />
      <div className="flex gap-(--gap) overflow-hidden px-(--pad)">
        {Array.from({ length: 9 }, (_, i) => (
          <CardSkeleton key={i} />
        ))}
      </div>
    </div>
  );
}
