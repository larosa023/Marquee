// For You: one endless, mixed-media feed of personal picks, with filters.
import { useInfiniteQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api } from '@/lib/api';
import type { Card, Filters } from '@/lib/types';
import { setting, useApp } from '@/state/app';
import { CardGrid, GridSkeleton } from '@/ui/Card';
import { useCardMenu } from '@/ui/CardMenu';
import { FilterBar, activeFilterCount } from '@/ui/Filters';
import { Button, EmptyState, ErrorState, OfflineBanner, PageHeader } from '@/ui/primitives';

interface Page {
  cards: Card[];
  next: number | null;
  total: number;
}

export default function ForYou() {
  const [filters, setFilters] = useState<Filters>({});
  const navigate = useApp((s) => s.navigate);
  const offline = useApp((s) => s.offline);
  const personal = useApp((s) => s.boot?.has_taste);
  const hasServices = useApp((s) => setting<number[]>(s.boot?.settings, 'my_providers', []).length > 0);
  const menu = useCardMenu();
  const q = useInfiniteQuery({
    queryKey: ['for_you', filters],
    queryFn: ({ pageParam }) => api<Page>('for_you', { filters, offset: pageParam, limit: 42 }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.next,
  });

  // Load the next page a screen before the end is reached.
  const sentinel = useRef<HTMLDivElement>(null);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = q;
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !hasNextPage) return;
    const io = new IntersectionObserver((entries) => entries[0].isIntersecting && !isFetchingNextPage && void fetchNextPage(), { rootMargin: '900px' });
    io.observe(el);
    return () => io.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const cards = q.data?.pages.flatMap((p) => p.cards) ?? [];
  const total = q.data?.pages[0]?.total ?? 0;
  return (
    <>
      <PageHeader title="For You" subtitle={personal ? `${total.toLocaleString()} picks across movies, TV, games and music, best match first` : 'Rate a few things and this becomes yours'}>
        <Button variant="primary" icon="dice" onClick={() => navigate('/for-you/rate')}>
          Quick Rate
        </Button>
      </PageHeader>
      {offline && <OfflineBanner />}
      <FilterBar value={filters} onChange={setFilters} hasServices={hasServices} />
      {q.isPending ? (
        <GridSkeleton />
      ) : q.error ? (
        <ErrorState error={q.error} onRetry={() => void q.refetch()} />
      ) : cards.length === 0 ? (
        <EmptyState
          icon="sparkles"
          title={activeFilterCount(filters) ? 'Nothing matches those filters' : 'No picks yet'}
          action={
            activeFilterCount(filters) ? (
              <Button onClick={() => setFilters({})}>Clear filters</Button>
            ) : (
              <Button variant="primary" icon="sync" onClick={() => void api('sync')}>
                Sync now
              </Button>
            )
          }
        >
          {activeFilterCount(filters) ? 'Loosen a filter or two.' : 'Marquee builds this feed from new and upcoming releases. Sync to fetch them.'}
        </EmptyState>
      ) : (
        <>
          <CardGrid cards={cards} origin="foryou" onContext={menu.onContext} showWhy />
          <div ref={sentinel} className="h-px" />
          {q.isFetchingNextPage && (
            <div className="mt-7">
              <GridSkeleton count={7} />
            </div>
          )}
          {!q.hasNextPage && cards.length > 20 && <p className="mt-10 text-center text-[13px] text-faint">That's everything that matches, for now.</p>}
        </>
      )}
      {menu.element}
    </>
  );
}
