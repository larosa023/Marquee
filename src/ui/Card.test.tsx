import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { backend } from '../../tests/setup';
import { boot, renderApp } from '../../tests/render';
import { api } from '@/lib/api';
import type { Card } from '@/lib/types';
import { useApp } from '@/state/app';
import { MatchRing, PosterCard } from './Card';
import { Stars } from './Stars';

async function firstCard(): Promise<Card> {
  return (await api<{ cards: Card[] }>('for_you', {})).cards[0];
}

describe('PosterCard', () => {
  it('describes the title, type, match and date to a screen reader', async () => {
    await boot();
    const card = await firstCard();
    renderApp(<PosterCard card={card} origin="test" />);
    const open = screen.getByRole('button', { name: /Signal From Europa, Movie, 93% match/ });
    expect(open).toBeInTheDocument();
    expect(screen.getByRole('img', { name: '93% match' })).toBeInTheDocument();
  });

  it('opens the detail page from where it was clicked', async () => {
    await boot();
    const card = await firstCard();
    renderApp(<PosterCard card={card} origin="row-week" />);
    await userEvent.click(screen.getByRole('button', { name: /Signal From Europa, Movie/ }));
    expect(useApp.getState().itemOrigin).toBe('row-week');
    expect(location.hash).toContain(`item=${card.id}`);
  });

  it('saves to the matching list with one click and says so', async () => {
    await boot();
    const card = await firstCard();
    renderApp(<PosterCard card={card} origin="test" />);
    await userEvent.click(screen.getByRole('button', { name: 'Save Signal From Europa' }));
    await waitFor(() => expect(backend.calls.some((c) => c.method === 'add' && c.params.target === 'list:1')).toBe(true));
    await waitFor(() => expect(useApp.getState().toasts.at(-1)?.text).toBe('Added Signal From Europa to Watchlist'));
  });

  it('hides the match ring when there is no taste profile yet', async () => {
    await boot();
    const card = { ...(await firstCard()), match_pct: null };
    renderApp(<PosterCard card={card} origin="test" />);
    expect(screen.queryByRole('img', { name: /match/ })).not.toBeInTheDocument();
  });

  it('opens its menu from the keyboard', async () => {
    await boot();
    const card = await firstCard();
    const onContext = vi.fn();
    renderApp(<PosterCard card={card} origin="test" onContext={onContext} />);
    screen.getByRole('button', { name: /Signal From Europa, Movie/ }).focus();
    await userEvent.keyboard('{Shift>}{F10}{/Shift}');
    expect(onContext).toHaveBeenCalledWith(card, expect.objectContaining({ x: expect.any(Number) }));
  });
});

describe('MatchRing', () => {
  it('states the percentage as text, not only as a ring', () => {
    renderApp(<MatchRing pct={72} />);
    expect(screen.getByRole('img', { name: '72% match' })).toHaveTextContent('72');
  });
});

describe('Stars', () => {
  it('rates in half stars from the keyboard and clears with 0', async () => {
    const onChange = vi.fn();
    renderApp(<Stars value={3} onChange={onChange} />);
    const slider = screen.getByRole('slider', { name: 'Your rating' });
    expect(slider).toHaveAttribute('aria-valuetext', '3 out of 5 stars');
    slider.focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(onChange).toHaveBeenLastCalledWith(3.5);
    await userEvent.keyboard('5');
    expect(onChange).toHaveBeenLastCalledWith(5);
    await userEvent.keyboard('0');
    expect(onChange).toHaveBeenLastCalledWith(0);
  });

  it('does nothing when read-only', async () => {
    const onChange = vi.fn();
    renderApp(<Stars value={null} onChange={onChange} readOnly />);
    expect(screen.getByRole('slider')).toHaveAttribute('aria-valuetext', 'Not rated');
    await userEvent.keyboard('{ArrowRight}');
    expect(onChange).not.toHaveBeenCalled();
  });
});
