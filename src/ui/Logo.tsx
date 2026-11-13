// The Marquee mark: a theatre-marquee frame of bulbs around an M. While a sync runs the bulbs chase.
import { cx } from './primitives';

const BULBS: [number, number][] = (() => {
  const pts: [number, number][] = [];
  const n = 5;
  for (let i = 0; i < n; i++) pts.push([14 + (36 / (n - 1)) * i, 9]);
  for (let i = 1; i < n - 1; i++) pts.push([55, 9 + (46 / (n - 1)) * i]);
  for (let i = n - 1; i >= 0; i--) pts.push([14 + (36 / (n - 1)) * i, 55]);
  for (let i = n - 2; i > 0; i--) pts.push([9, 9 + (46 / (n - 1)) * i]);
  return pts;
})();

export function Logo({ size = 28, active = false, className }: { size?: number; active?: boolean; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="Marquee" className={cx(active && 'bulbs-on', className)}>
      <rect x="3" y="3" width="58" height="58" rx="15" fill="#0A0A0C" />
      <rect x="3.75" y="3.750" width="56.500" height="56.500" rx="14.250" fill="none" stroke="var(--accent)" strokeOpacity="0.35" strokeWidth="1.500" />
      {BULBS.map(([cx, cy], i) => (
        <circle key={i} className="bulb" cx={cx} cy={cy} r="2.300" fill="var(--accent)" style={{ ['--i' as string]: i } as React.CSSProperties} />
      ))}
      <path d="M19.500 45V20.500h4.700L32 34l7.800-13.500h4.700V45h-4.800V30.200L33.600 41h-3.200l-6.100-10.800V45z" fill="var(--accent)" />
    </svg>
  );
}
