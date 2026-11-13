import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { contrast } from '@/lib/img';
import { Heatmap, heatColor } from './Heatmap';

const ep = (season: number, number: number, rating: number | null) => ({ season, number, name: `Episode ${number}`, air_date: '2025-01-05', rating, votes: 140 });

describe('episode heatmap', () => {
  it('labels every episode with its season, number and rating', () => {
    render(<Heatmap seasons={[[ep(1, 1, 8.1), ep(1, 2, 6.4)], [ep(2, 1, 9.3)], []]} />);
    expect(screen.getByRole('button', { name: 'Season 1 episode 2, Episode 2: 6.4 out of 10' })).toHaveTextContent('6.4');
    expect(screen.getByRole('button', { name: 'Season 2 episode 1, Episode 1: 9.3 out of 10' })).toBeInTheDocument();
    // Empty seasons get no column.
    expect(screen.getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['Episode', 'S1', 'S2']);
  });

  it('shows an unrated episode as a dash and says so', () => {
    render(<Heatmap seasons={[[ep(1, 1, null)]]} />);
    expect(screen.getByRole('button', { name: /not rated yet/ })).toHaveTextContent('–');
  });

  it('gives details for the focused episode', async () => {
    render(<Heatmap seasons={[[ep(1, 1, 8.1)]]} />);
    await userEvent.tab();
    expect(screen.getByText(/8\.1\/10 from 140 votes/)).toBeInTheDocument();
  });

  it('keeps the number readable on every color in the scale', () => {
    const probe = document.createElement('span');
    document.body.append(probe);
    for (let r = 1; r <= 10; r += 0.5) {
      const { bg, fg } = heatColor(r);
      const [, h, s, l] = /hsl\((\d+) (\d+)% (\d+)%\)/.exec(bg)!.map(Number);
      // hsl -> hex, to measure contrast against the text color.
      const a = (s / 100) * Math.min(l / 100, 1 - l / 100);
      const f = (n: number) => {
        const k = (n + h / 30) % 12;
        return Math.round(255 * (l / 100 - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))))
          .toString(16)
          .padStart(2, '0');
      };
      expect(contrast(fg, `#${f(0)}${f(8)}${f(4)}`)).toBeGreaterThanOrEqual(4.5);
    }
    probe.remove();
  });
});
