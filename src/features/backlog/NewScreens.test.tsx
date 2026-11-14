import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { backend } from '../../../tests/setup';
import { boot, renderApp } from '../../../tests/render';
import Backlog from './Backlog';
import Wrapped from '../wrapped/Wrapped';
import Reel from '../reel/Reel';

describe('Backlog', () => {
  it('shows a start-next pick, lengths and re-sorts', async () => {
    await boot();
    renderApp(<Backlog />);
    expect(await screen.findByRole('region', { name: 'Start next' })).toBeInTheDocument();
    expect(screen.getAllByText('9.5 h').length).toBeGreaterThan(0);
    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Sort' }), 'shortest');
    await waitFor(() => expect(backend.calls.filter((c) => c.method === 'backlog').at(-1)?.params.sort).toBe('shortest'));
  });
});

describe('Wrapped', () => {
  it('shows the year in numbers and favourites per medium', async () => {
    await boot();
    renderApp(<Wrapped />);
    expect(await screen.findByRole('heading', { name: /in Marquee/ })).toBeInTheDocument();
    expect(screen.getByText('612')).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Movies' })).toBeInTheDocument();
    expect(screen.getByText('Boards of Canada')).toBeInTheDocument();
  });
});

describe('Trailer reel', () => {
  it('plays a trailer, moves on with the keyboard and asks for more', async () => {
    await boot();
    renderApp(<Reel />);
    const first = await screen.findByTitle(/trailer$/);
    expect(first.getAttribute('src')).toContain('youtube-nocookie.com/embed/fixture');
    const title = screen.getByRole('complementary', { name: 'About this trailer' }).querySelector('h2')!.textContent;
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    });
    await waitFor(() => expect(screen.getByRole('complementary', { name: 'About this trailer' }).querySelector('h2')!.textContent).not.toBe(title));
  });
});
