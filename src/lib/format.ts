import type { Card, Media } from './types';

const DAY = 86_400_000;

function parse(date: string): Date {
  // Dates from the engine are plain local days ("2026-11-20"); don't let them shift by time zone.
  const [y, m, d] = date.slice(0, 10).split('-').map(Number);
  return new Date(y, (m || 1) - 1, d || 1);
}

export function daysUntil(date: string, now = new Date()): number {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return Math.round((parse(date).getTime() - today.getTime()) / DAY);
}

/** "Today", "Tomorrow", "Fri", "Nov 20", "Nov 20, 2027". */
export function releaseLabel(date: string | null | undefined, now = new Date()): string {
  if (!date) return 'TBA';
  const d = parse(date);
  const days = daysUntil(date, now);
  if (days === 0) return 'Today';
  if (days === 1) return 'Tomorrow';
  if (days === -1) return 'Yesterday';
  if (days > 1 && days < 7) return d.toLocaleDateString(undefined, { weekday: 'long' });
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString(undefined, sameYear ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
}

export function countdown(date: string | null | undefined, now = new Date()): string | null {
  if (!date) return null;
  const days = daysUntil(date, now);
  if (days < 0) return null;
  if (days === 0) return 'Today';
  if (days === 1) return 'Tomorrow';
  if (days < 14) return `In ${days} days`;
  if (days < 60) return `In ${Math.round(days / 7)} weeks`;
  return `In ${Math.round(days / 30)} months`;
}

export function longDate(date: string): string {
  return parse(date).toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' });
}

export function isoDay(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

export function runtimeLabel(card: Pick<Card, 'media' | 'runtime'>): string | null {
  const m = card.runtime;
  if (!m) return null;
  if (card.media === 'game') return `~${Math.round(m / 60)} h`;
  if (card.media === 'tv') return `${m} min episodes`;
  return m >= 60 ? `${Math.floor(m / 60)}h ${m % 60}m` : `${m} min`;
}

export function price(cents: number | null | undefined, currency = 'USD'): string | null {
  if (cents == null) return null;
  if (cents === 0) return 'Free';
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(cents / 100);
  } catch {
    return `$${(cents / 100).toFixed(2)}`;
  }
}

export function ago(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return 'never';
  const mins = Math.round((now - new Date(iso).getTime()) / 60_000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? 'yesterday' : `${days} days ago`;
}

export function bytes(n: number): string {
  if (n < 1024 * 1024) return `${Math.max(0, Math.round(n / 1024))} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(n < 10 * 1024 ** 2 ? 1 : 0)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export const mediaLabel: Record<Media | 'artist', string> = { movie: 'Movie', tv: 'TV', game: 'Game', album: 'Album', artist: 'Artist' };

export function typeBadge(card: Pick<Card, 'media' | 'meta'>): string {
  if (card.media === 'album') {
    const t = card.meta?.album_type;
    return t === 'ep' ? 'EP' : t === 'single' ? 'Single' : 'Album';
  }
  if (card.meta?.anime === true) return 'Anime';
  return mediaLabel[card.media];
}

export function doneVerb(media: Media): string {
  return media === 'game' ? 'Played' : media === 'album' ? 'Listened' : 'Watched';
}

/** The one-line credit under a title: director, network, developer or artist. */
export function credit(card: Pick<Card, 'meta'>): string | null {
  const m = card.meta ?? {};
  return m.artist ?? m.director ?? m.developer ?? m.network ?? null;
}

/** Turns "*Arrival*" markers in a reason into parts so titles can be set in italics. */
export function reasonParts(text: string): { text: string; em: boolean }[] {
  return text
    .split(/(\*[^*]+\*)/g)
    .filter(Boolean)
    .map((part) => (part.startsWith('*') && part.endsWith('*') ? { text: part.slice(1, -1), em: true } : { text: part, em: false }));
}

export function stars(value: number): string {
  return '★'.repeat(Math.floor(value)) + (value % 1 ? '½' : '');
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/** Just the word, for when the number is shown separately: `${n} ${noun(n, 'title')}`. */
export function noun(n: number, one: string, many = `${one}s`): string {
  return n === 1 ? one : many;
}
