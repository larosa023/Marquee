// Test environment for the component tests: a fresh fake engine per test, and the few browser
// features jsdom doesn't have.
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, beforeEach, vi } from 'vitest';
import { setBackendForTests } from '../src/lib/api';
import { useApp } from '../src/state/app';
import { createBackend } from './mock/backend';

class Observer {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
}
vi.stubGlobal('ResizeObserver', Observer);
vi.stubGlobal('IntersectionObserver', Observer);
window.matchMedia ??= ((query: string) => ({ matches: false, media: query, onchange: null, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
Element.prototype.scrollIntoView ??= () => {};
Element.prototype.scrollTo ??= (() => {}) as typeof Element.prototype.scrollTo;
window.scrollTo = (() => {}) as typeof window.scrollTo;

export let backend = createBackend();

beforeEach(() => {
  backend = createBackend({ onboarded: true, hasTaste: true });
  setBackendForTests(backend);
  location.hash = '';
  useApp.setState({ boot: null, sys: null, path: '/home', item: null, itemOrigin: null, toasts: [], paletteOpen: false, notificationsOpen: false, update: null, updateOpen: false, pendingFile: null, sync: { running: false, label: '', pct: 0 } });
});

afterEach(() => {
  cleanup();
  setBackendForTests(null);
});
