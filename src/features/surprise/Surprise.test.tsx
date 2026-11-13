import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { backend } from '../../../tests/setup';
import { boot, renderApp } from '../../../tests/render';
import { useApp } from '@/state/app';
import Surprise from './Surprise';

describe('Surprise me', () => {
  it('rolls a pick of the chosen kind and re-rolls without repeating it', async () => {
    await boot();
    renderApp(<Surprise />);
    act(() => useApp.getState().set({ surprise: 'movie' }));
    const open = await screen.findByRole('button', { name: /^Open / }, { timeout: 3000 });
    expect(open).toBeInTheDocument();
    const first = backend.calls.filter((c) => c.method === 'surprise');
    expect(first.at(-1)?.params).toMatchObject({ media: 'movie', upcoming: false });

    await userEvent.click(screen.getByRole('button', { name: /Roll another movie/ }));
    await waitFor(() => {
      const rolls = backend.calls.filter((c) => c.method === 'surprise');
      expect(rolls.at(-1)?.params.skip.length).toBeGreaterThan(0);
    });

    await userEvent.click(screen.getByRole('radio', { name: 'Game' }));
    await waitFor(() => expect(backend.calls.filter((c) => c.method === 'surprise').at(-1)?.params.media).toBe('game'));
  });
});
