// Everything a user can do to a title, as hooks. Each one tells the engine, shows a toast that
// says what happened (including "queued while offline" and "opened the store page"), and
// refreshes whatever is on screen.
import { useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { api, errorMessage } from '@/lib/api';
import { doneVerb } from '@/lib/format';
import type { Card, ListInfo } from '@/lib/types';
import { useApp } from '@/state/app';

/** Refetches every feed (they're local queries, so this is cheap). */
export function refreshFeeds(qc: QueryClient, only?: string[]) {
  for (const key of only ?? ['home', 'for_you', 'hub', 'calendar', 'lists', 'list', 'item', 'tracked', 'quick_rate', 'taste', 'today', 'search']) {
    void qc.invalidateQueries({ queryKey: [key] });
  }
}

interface Pushed {
  state: 'synced' | 'opened' | 'queued' | 'failed';
  hint?: string;
  message?: string;
}

/** Turns the engine's answer about a service write into words. */
export function describePush(where: string, title: string, pushed: Pushed | null | undefined): { text: string; kind: 'good' | 'info' | 'bad' } {
  if (!pushed) return { text: `Added ${title} to ${where}`, kind: 'good' };
  switch (pushed.state) {
    case 'synced':
      return { text: `Added ${title} to ${where}`, kind: 'good' };
    case 'opened':
      return { text: pushed.hint ?? `Opened ${title} so you can finish adding it.`, kind: 'info' };
    case 'queued':
      return { text: `You're offline. ${title} will be added to ${where} when you reconnect.`, kind: 'info' };
    default:
      return { text: `${title} is saved in Marquee, but ${where} said no: ${pushed.message ?? 'unknown error'}`, kind: 'bad' };
  }
}

const DEFAULT_LIST: Record<string, string> = { movie: 'watchlist', tv: 'watchlist', game: 'play-later', album: 'listen-later' };

export function useAddTo() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (card: Pick<Card, 'id' | 'title'>, target: string, where: string) => {
      try {
        const out = await api('add', { id: card.id, target });
        const { text, kind } = describePush(where, card.title, out.pushed);
        toast(text, kind);
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}

/** The + button on a card: saves to the matching Marquee list (Watchlist, Play Later, Listen Later), or removes it again. */
export function useQuickAdd() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (card: Card) => {
      const lists = useApp.getState().boot?.lists ?? [];
      const list = lists.find((l: ListInfo) => l.slug === DEFAULT_LIST[card.media]);
      if (!list) return;
      try {
        if (card.saved) {
          const all = await api<ListInfo[]>('lists');
          // Remove from wherever it is saved.
          for (const l of all) await api('remove', { id: card.id, list_id: l.id });
          toast(`Removed ${card.title} from your lists`, 'info', { label: 'Undo', run: () => void api('add', { id: card.id, target: `list:${list.id}` }).then(() => refreshFeeds(qc)) });
        } else {
          const out = await api('add', { id: card.id, target: `list:${list.id}` });
          const { text, kind } = describePush(list.name, card.title, out.pushed);
          toast(text, kind, { label: 'Undo', run: () => void api('remove', { id: card.id, list_id: list.id }).then(() => refreshFeeds(qc)) });
        }
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}

export function useRate() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (card: Pick<Card, 'id' | 'title'>, stars: number, quiet = false) => {
      try {
        const out = await api('rate', { id: card.id, stars });
        const failed = (out.pushed ?? []).find((p: any) => p.result?.state === 'failed');
        const queued = (out.pushed ?? []).some((p: any) => p.result?.state === 'queued');
        if (failed) toast(`Rated in Marquee, but ${failed.source} said: ${failed.result.message}`, 'bad');
        else if (!quiet) toast(stars > 0 ? `Rated ${card.title} ${stars}★${queued ? ' (will sync when you reconnect)' : ''}` : `Cleared your rating for ${card.title}`, 'good');
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}

export function useDone() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (card: Pick<Card, 'id' | 'title' | 'media'>, done: boolean) => {
      try {
        await api('done', { id: card.id, done });
        toast(done ? `Marked ${card.title} as ${doneVerb(card.media).toLowerCase()}` : `Removed ${card.title} from your history`, 'good');
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}

export function useDismiss() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (card: Pick<Card, 'id' | 'title'>, reason: 'seen' | 'genre' | 'creator' | 'other') => {
      try {
        await api('dismiss', { id: card.id, reason });
        const text = reason === 'seen' ? `Got it, you've seen ${card.title}` : `You won't see ${card.title} again`;
        toast(text, 'info', reason === 'seen' ? undefined : { label: 'Undo', run: () => void api('undismiss', { id: card.id }).then(() => refreshFeeds(qc)) });
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}

export function useMoreLike() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (card: Pick<Card, 'id' | 'title'>) => {
      try {
        await api('more_like', { id: card.id });
        toast(`More like ${card.title}, coming up`, 'good');
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}

export function useFollow() {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  return useCallback(
    async (kind: 'item' | 'artist' | 'franchise', reference: string, name: string, following: boolean, itemId?: number, onSpotify = false) => {
      try {
        if (following) {
          await api('unfollow', { kind, reference });
          toast(`Unfollowed ${name}`, 'info');
        } else {
          await api('follow', { kind, reference, name, item_id: itemId ?? null, on_spotify: onSpotify });
          toast(kind === 'item' ? `You'll be reminded when ${name} is out` : `Following ${name}. New releases will be pinned to your calendar.`, 'good');
        }
      } catch (e) {
        toast(errorMessage(e), 'bad');
      }
      refreshFeeds(qc);
    },
    [qc, toast],
  );
}
