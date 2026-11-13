import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { createBackend } from '../../../tests/mock/backend';
import { boot, renderApp } from '../../../tests/render';
import { setBackendForTests } from '@/lib/api';
import { useApp } from '@/state/app';
import Search from './Search';

describe('Search', () => {
  it('finds titles as you type and opens the highlighted one with Enter', async () => {
    await boot();
    renderApp(<Search initial="" />);
    await userEvent.type(screen.getByRole('combobox'), 'harbor');
    const hit = await screen.findByRole('option', { name: /Quiet Harbor/ });
    expect(hit).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{Enter}');
    expect(location.hash).toMatch(/item=\d+/);
    // The search is remembered.
    await waitFor(() => expect(useApp.getState().boot?.settings['search.recent']).toEqual(['harbor']));
  });

  it('moves through results with the arrow keys', async () => {
    await boot();
    renderApp(<Search initial="" />);
    await userEvent.type(screen.getByRole('combobox'), 'li'); // "Paper Lighthouse", "Northlight"
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(1));
    const options = screen.getAllByRole('option');
    expect(options[0]).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getAllByRole('option')[1]).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{ArrowUp}');
    expect(screen.getAllByRole('option')[0]).toHaveAttribute('aria-selected', 'true');
  });

  it('narrows to one type with the tabs', async () => {
    await boot();
    renderApp(<Search initial="a" />);
    await userEvent.type(screen.getByRole('combobox'), 't'); // "at": Europa? no; matches "Glass Weather", "Static Bloom", "Ember Atlas"...
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(0));
    await userEvent.click(screen.getByRole('tab', { name: /Games/ }));
    await waitFor(() => expect(screen.getAllByRole('option').every((o) => /Game/.test(o.textContent ?? ''))).toBe(true));
  });

  it('says so when nothing matches, and offers a vibe search', async () => {
    await boot();
    renderApp(<Search initial="" />);
    await userEvent.type(screen.getByRole('combobox'), 'zzzz');
    expect(await screen.findByRole('heading', { name: 'Nothing found for "zzzz"' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Try it as a vibe' }));
    expect(await screen.findByText(/tag matching/)).toBeInTheDocument();
  });

  it('runs a vibe search from the route and shows what it understood', async () => {
    await boot();
    renderApp(<Search initial="vibe:something cozy" />);
    expect(await screen.findByText('cozy')).toBeInTheDocument();
    expect(screen.getByRole('combobox')).toHaveValue('something cozy');
    expect(screen.getByText(/Matched by mood and theme tags/)).toBeInTheDocument();
  });

  it('explains that results are local-only while offline', async () => {
    setBackendForTests(createBackend({ onboarded: true, hasTaste: true, offline: true }));
    await boot();
    renderApp(<Search initial="" />);
    await userEvent.type(screen.getByRole('combobox'), 'orbit');
    expect(await screen.findByText(/You're offline, so this is only what's already saved/)).toBeInTheDocument();
  });
});
