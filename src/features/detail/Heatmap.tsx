// Episode ratings as a grid: one column per season, one cell per episode, coloured from weak to great.
// Colour is never the only signal: every cell carries its number and a full label.
import { useState } from 'react';
import { longDate } from '@/lib/format';

interface Episode {
  season: number;
  number: number;
  name: string;
  air_date: string | null;
  rating: number | null;
  votes: number;
}

/** 5.0 and below reads as weak, 9.0 and above as great. Returns a background and a text colour that contrast. */
export function heatColor(rating: number | null): { bg: string; fg: string } {
  if (rating == null || rating <= 0) return { bg: 'var(--surface-strong)', fg: 'var(--subtext)' };
  const t = Math.min(1, Math.max(0, (rating - 5) / 4));
  // Red (8°) through amber to green (140°); lightness chosen so black text always clears 4.5:1.
  const hue = 8 + t * 132;
  return { bg: `hsl(${hue.toFixed(0)} 72% ${(58 + t * 4).toFixed(0)}%)`, fg: '#0a0a0c' };
}

export function Heatmap({ seasons }: { seasons: Episode[][] }) {
  const [active, setActive] = useState<Episode | null>(null);
  const shown = seasons.filter((s) => s.length > 0);
  const longest = Math.max(...shown.map((s) => s.length));
  return (
    <div className="max-w-full">
      <div className="no-scrollbar overflow-x-auto pb-1">
        <table className="tnum border-separate border-spacing-1" aria-label="Episode ratings by season">
          <thead>
            <tr>
              <th scope="col" className="sr-only">
                Episode
              </th>
              {shown.map((s) => (
                <th key={s[0].season} scope="col" className="min-w-9 pb-1 text-center text-[11px] font-semibold text-faint">
                  S{s[0].season}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {Array.from({ length: longest }, (_, row) => (
              <tr key={row}>
                <th scope="row" className="pr-2 text-right text-[11px] font-medium text-faint">
                  E{row + 1}
                </th>
                {shown.map((s) => {
                  const ep = s[row];
                  if (!ep) return <td key={s[0].season} />;
                  const { bg, fg } = heatColor(ep.rating);
                  return (
                    <td key={s[0].season} className="p-0">
                      <button
                        type="button"
                        className="grid h-7 w-9 place-items-center rounded-md text-[11px] font-bold transition-transform hover:scale-110 focus-visible:scale-110"
                        style={{ background: bg, color: fg, animation: 'heat-in 0.35s both', animationDelay: `${Math.min(600, (row + shown.indexOf(s)) * 14)}ms` }}
                        aria-label={`Season ${ep.season} episode ${ep.number}, ${ep.name}: ${ep.rating ? `${ep.rating.toFixed(1)} out of 10` : 'not rated yet'}`}
                        onMouseEnter={() => setActive(ep)}
                        onFocus={() => setActive(ep)}
                      >
                        {ep.rating ? ep.rating.toFixed(1) : '–'}
                      </button>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="mt-3 flex min-h-10 flex-wrap items-center justify-between gap-x-6 gap-y-2 text-[13px]" aria-live="polite">
        {active ? (
          <p className="text-subtext">
            <span className="font-semibold text-text">
              S{active.season} E{active.number} · {active.name}
            </span>
            {active.rating ? ` · ${active.rating.toFixed(1)}/10 from ${active.votes.toLocaleString()} votes` : ' · not rated yet'}
            {active.air_date ? ` · ${longDate(active.air_date)}` : ''}
          </p>
        ) : (
          <p className="text-faint">Hover or focus an episode for its title and votes.</p>
        )}
        <div className="flex items-center gap-1.5 text-[11px] text-faint" aria-hidden="true">
          <span>Weak</span>
          {[5, 6, 7, 8, 9].map((r) => (
            <span key={r} className="h-3 w-5 rounded-sm" style={{ background: heatColor(r).bg }} />
          ))}
          <span>Great</span>
        </div>
      </div>
    </div>
  );
}
