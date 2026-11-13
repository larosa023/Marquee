import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { backend } from '../../../tests/setup';
import { boot, renderApp } from '../../../tests/render';
import { api } from '@/lib/api';
import Lists from './Lists';

describe('Lists', () => {
  it('shows the four built-in lists and creates a custom one', async () => {
    await boot();
    renderApp(<Lists listId={null} />);
    for (const name of ['Watchlist', 'Play Later', 'Listen Later', 'Favorites']) expect(await screen.findByRole('button', { name: new RegExp(name) })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'New list' }));
    await userEvent.type(within(screen.getByRole('dialog', { name: 'New list' })).getByRole('textbox'), 'Rainy Sunday{Enter}');
    await waitFor(() => expect(backend.calls.some((c) => c.method === 'list.create' && c.params.name === 'Rainy Sunday')).toBe(true));
    await waitFor(() => expect(location.hash).toBe('#/lists/5'));
  });

  it('shows saved titles with their sync state, and filters them', async () => {
    await api('add', { id: 1, target: 'list:1' });
    await api('add', { id: 2, target: 'list:1' });
    await api('add', { id: 5, target: 'letterboxd:watchlist' });
    await boot();
    renderApp(<Lists listId={1} />);
    expect(await screen.findByRole('button', { name: 'Open Signal From Europa' })).toBeInTheDocument();
    // The Letterboxd add opened the film page, so it is waiting for confirmation.
    const pending = screen.getByRole('button', { name: 'Open Paper Lighthouse' }).closest('li')!;
    expect(within(pending).getByText('Pending')).toBeInTheDocument();
    expect(screen.getByText(/1 waiting to sync/)).toBeInTheDocument();

    await userEvent.type(screen.getByRole('textbox', { name: 'Filter this list' }), 'orbit');
    expect(screen.queryByRole('button', { name: 'Open Signal From Europa' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open The Long Orbit' })).toBeInTheDocument();
  });

  it('reorders from the keyboard and saves the new order', async () => {
    await api('add', { id: 1, target: 'list:1' });
    await api('add', { id: 2, target: 'list:1' }); // newest first: The Long Orbit, Signal From Europa
    await boot();
    renderApp(<Lists listId={1} />);
    const handle = await screen.findByRole('button', { name: /Reorder The Long Orbit\. Position 1 of 2/ });
    handle.focus();
    await userEvent.keyboard('{ArrowDown}');
    await waitFor(() => expect(backend.calls.find((c) => c.method === 'list.reorder')?.params.item_ids).toEqual([1, 2]));
    expect(await screen.findByRole('button', { name: /Reorder The Long Orbit\. Position 2 of 2/ })).toBeInTheDocument();
  });

  it('removes several titles at once', async () => {
    await api('add', { id: 1, target: 'list:1' });
    await api('add', { id: 2, target: 'list:1' });
    await boot();
    renderApp(<Lists listId={1} />);
    await userEvent.click(await screen.findByRole('button', { name: 'List actions' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Select titles' }));
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select Signal From Europa' }));
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select The Long Orbit' }));
    expect(screen.getByText('2 selected')).toBeInTheDocument();
    await userEvent.click(within(screen.getByRole('toolbar', { name: 'Bulk actions' })).getByRole('button', { name: 'Remove' }));
    expect(await screen.findByRole('heading', { name: 'Nothing here yet' })).toBeInTheDocument();
  });

  it('only offers mirrors for services that are connected', async () => {
    await boot();
    renderApp(<Lists listId={1} />);
    await userEvent.click(await screen.findByRole('button', { name: 'Mirror…' }));
    expect(screen.getByRole('menuitem', { name: /Trakt watchlist/ })).toBeDisabled();
    expect(screen.getByRole('menuitem', { name: /Marquee only/ })).toBeEnabled();
  });
});
