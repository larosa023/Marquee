// Renders a screen the way the app does: inside a query client, with the app state loaded.
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render } from '@testing-library/react';
import { MotionConfig } from 'framer-motion';
import type { ReactElement } from 'react';
import { api } from '../src/lib/api';
import type { Bootstrap } from '../src/lib/types';
import { useApp } from '../src/state/app';

/** Loads the fake engine's state into the store, as the app shell does at startup. */
export async function boot() {
  useApp.getState().setBoot(await api<Bootstrap>('bootstrap'));
}

export function renderApp(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <MotionConfig reducedMotion="always">{ui}</MotionConfig>
    </QueryClientProvider>,
  );
}
