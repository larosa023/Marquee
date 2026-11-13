// The bridge to the engine. Every screen calls `api(method, params)`; nothing else talks to Rust.

import type { ApiError } from './types';

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<any>;
type Listen = (event: string, handler: (payload: any) => void) => Promise<() => void>;

interface Backend {
  invoke: Invoke;
  listen: Listen;
}

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let backendPromise: Promise<Backend> | null = null;

function backend(): Promise<Backend> {
  if (backendPromise) return backendPromise;
  if (inTauri) {
    backendPromise = Promise.all([import('@tauri-apps/api/core'), import('@tauri-apps/api/event')]).then(([core, event]) => ({
      invoke: core.invoke as Invoke,
      listen: (name, handler) => event.listen(name, (e) => handler(e.payload)),
    }));
  } else if (import.meta.env.VITE_MOCK === '1' || import.meta.env.MODE === 'test') {
    // A fake engine for UI tests and for working on screens in a browser. It is only compiled
    // into builds made with VITE_MOCK=1; the release build doesn't contain it.
    backendPromise = import('../../tests/mock/backend').then((m) => m.createBackend());
  } else {
    backendPromise = Promise.reject({ kind: 'error', message: 'Marquee has to run as the desktop app.' } satisfies ApiError);
  }
  return backendPromise;
}

/** Tests swap in a fresh fake engine between cases. */
export function setBackendForTests(b: Backend | null) {
  backendPromise = b ? Promise.resolve(b) : null;
}

export function isApiError(e: unknown): e is ApiError {
  return typeof e === 'object' && e !== null && 'message' in e && 'kind' in e;
}

export function errorMessage(e: unknown): string {
  if (isApiError(e)) return e.message;
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return 'Something went wrong.';
}

/** Calls the engine. Rejects with an `ApiError` ({ kind, message }). */
export async function api<T = any>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  const b = await backend();
  try {
    return (await b.invoke('call', { method, params })) as T;
  } catch (e) {
    throw isApiError(e) ? e : ({ kind: 'error', message: errorMessage(e) } satisfies ApiError);
  }
}

/** Calls an app-shell command (window, tray, updater). */
export async function shell<T = any>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const b = await backend();
  return b.invoke(cmd, args) as Promise<T>;
}

/** Subscribes to an engine event. Returns the unsubscribe function. */
export function on(event: string, handler: (payload: any) => void): () => void {
  let off: (() => void) | null = null;
  let cancelled = false;
  backend()
    .then((b) => b.listen(event, handler))
    .then((un) => {
      if (cancelled) un();
      else off = un;
    })
    .catch(() => {});
  return () => {
    cancelled = true;
    off?.();
  };
}

export async function openUrl(url: string) {
  try {
    await shell('open_url', { url });
  } catch {
    if (!inTauri) window.open(url, '_blank', 'noopener');
  }
}

/** File pickers. In the browser (tests) they resolve to null. */
export async function pickFile(filters: { name: string; extensions: string[] }[]): Promise<string | null> {
  if (!inTauri) return (window as any).__MOCK_PICK__ ?? null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ multiple: false, directory: false, filters });
  return typeof picked === 'string' ? picked : null;
}

export async function pickSave(defaultPath: string, filters: { name: string; extensions: string[] }[]): Promise<string | null> {
  if (!inTauri) return (window as any).__MOCK_PICK__ ?? null;
  const { save } = await import('@tauri-apps/plugin-dialog');
  return (await save({ defaultPath, filters })) ?? null;
}

export async function pickFolder(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ multiple: false, directory: true });
  return typeof picked === 'string' ? picked : null;
}
