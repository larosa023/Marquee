import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { backend } from '../../../tests/setup';
import { boot, renderApp } from '../../../tests/render';
import { useApp } from '@/state/app';
import Settings from './Settings';

describe('Settings', () => {
  it('lists every section and marks the open one', async () => {
    await boot();
    renderApp(<Settings section="notifications" />);
    const nav = screen.getByRole('navigation', { name: 'Settings sections' });
    for (const name of ['General', 'Connections', 'API keys', 'Taste', 'Streaming services', 'Notifications', 'Appearance', 'AI', 'Storage', 'Backup & Restore', 'Updates', 'Keyboard shortcuts', 'About']) {
      expect(within(nav).getByRole('button', { name })).toBeInTheDocument();
    }
    expect(within(nav).getByRole('button', { name: 'Notifications' })).toHaveAttribute('aria-current', 'page');
  });

  it('saves a notification choice straight away and greys out what it switches off', async () => {
    await boot();
    renderApp(<Settings section="notifications" />);
    await userEvent.click(screen.getByRole('switch', { name: 'Steam wishlist sales' }));
    await waitFor(() => expect(backend.calls.find((c) => c.method === 'settings.set' && c.params.key === 'notifications')?.params.value.sales).toBe(false));
    expect(screen.getByRole('slider', { name: 'Minimum discount' })).toBeDisabled();
    // The other defaults are kept rather than dropped.
    expect(useApp.getState().boot?.settings['notifications']).toMatchObject({ sales: false, releases: true, quiet_from: '22:00' });
  });

  it('rebinds a shortcut and refuses one that is already taken', async () => {
    await boot();
    renderApp(<Settings section="shortcuts" />);
    await userEvent.click(screen.getByRole('button', { name: 'Change the shortcut for Search' }));
    expect(screen.getByRole('status')).toHaveTextContent('Press the new shortcut');
    await userEvent.keyboard('{Control>}k{/Control}'); // Ctrl+K is the command palette
    expect(screen.getByRole('alert')).toHaveTextContent('already "Command palette"');
    await userEvent.keyboard('{Control>}{Shift>}f{/Shift}{/Control}');
    await waitFor(() => expect(useApp.getState().boot?.settings['shortcuts']).toEqual({ search: 'Ctrl+Shift+F' }));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Reset the shortcut for Search' }));
    await waitFor(() => expect(useApp.getState().boot?.settings['shortcuts']).toEqual({}));
  });

  it('will not delete everything until DELETE is typed', async () => {
    await boot();
    renderApp(<Settings section="storage" />);
    await userEvent.click(await screen.findByRole('button', { name: 'Delete everything…' }));
    const dialog = screen.getByRole('dialog', { name: 'Delete all your Marquee data?' });
    const confirm = within(dialog).getByRole('button', { name: 'Delete everything' });
    expect(confirm).toBeDisabled();
    await userEvent.type(within(dialog).getByLabelText('Type DELETE to confirm'), 'delete');
    expect(confirm).toBeDisabled();
    await userEvent.clear(within(dialog).getByLabelText('Type DELETE to confirm'));
    await userEvent.type(within(dialog).getByLabelText('Type DELETE to confirm'), 'DELETE');
    await userEvent.click(confirm);
    await waitFor(() => expect(backend.calls.some((c) => c.method === 'data.delete_all' && c.params.confirm === 'DELETE')).toBe(true));
    await waitFor(() => expect(useApp.getState().boot?.onboarded).toBe(false));
  });

  it('never shows a stored API key, only whether one is set', async () => {
    await boot();
    renderApp(<Settings section="keys" />);
    const row = screen.getByText('TMDB API key').closest('li')!;
    expect(within(row).getByText('Built in')).toBeInTheDocument();
    await userEvent.click(within(row).getByRole('button', { name: 'Enter key' }));
    const field = within(row).getByLabelText('TMDB API key');
    expect(field).toHaveAttribute('type', 'password');
    expect(field).toHaveValue('');
    await userEvent.type(field, 'secret-value{Enter}');
    await waitFor(() => expect(backend.calls.some((c) => c.method === 'keys.set' && c.params.name === 'tmdb' && c.params.value === 'secret-value')).toBe(true));
    await waitFor(() => expect(within(row).queryByLabelText('TMDB API key')).not.toBeInTheDocument());
    expect(document.body.textContent).not.toContain('secret-value');
  });
});
