// A small fake engine for UI tests and for working on screens in a browser.
// It is only compiled into builds made with VITE_MOCK=1 and into the test runner; the release
// build never contains it. Everything here is invented test data, clearly not from any service.

import type { Card, ListEntry, ListInfo, Media, Notification, SourceStatus, Theme } from '../../src/lib/types';

type Handler = (payload: any) => void;

function poster(title: string, hue: number): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="450"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue} 60% 42%)"/><stop offset="1" stop-color="hsl(${(hue + 50) % 360} 55% 18%)"/></linearGradient></defs><rect width="300" height="450" fill="url(#g)"/><text x="24" y="400" font-family="sans-serif" font-size="26" font-weight="700" fill="white">${title.replace(/&/g, '&amp;').slice(0, 18)}</text></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function day(offset: number): string {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

function theme(id: string, name: string, mode: 'dark' | 'light', background: string, surface: string, text: string, accent: string): Theme {
  const dark = mode === 'dark';
  return {
    schema: 1,
    id,
    name,
    description: `${name} test theme`,
    mode,
    backdrop: 'mica',
    transparency: 0.18,
    surfaceOpacity: 1,
    colors: { background, sidebar: background, surface, surfaceHover: surface, surfaceStrong: dark ? '#28282E' : '#DADDE3', text, subtext: dark ? '#B9B4AA' : '#3D4350', faint: dark ? '#898680' : '#5A6170', accent },
    marquee: { radius: 14, controlRadius: 10, blur: 24, shadow: 0.6, font: 'Inter Variable', displayFont: 'Inter Variable', motion: 1, glow: 0.5 },
    builtin: true,
  };
}

interface MockOptions {
  onboarded?: boolean;
  hasTaste?: boolean;
  offline?: boolean;
}

export function createBackend(options: MockOptions = (globalThis as any).__MOCK_STATE__ ?? {}) {
  const listeners = new Map<string, Set<Handler>>();
  const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((h) => h(payload));
  const calls: { method: string; params: any }[] = [];

  let nextId = 1;
  const mk = (media: Media, title: string, days: number, extra: Partial<Card> = {}): Card => {
    const id = nextId++;
    return {
      id,
      media,
      title,
      year: new Date(day(days)).getFullYear(),
      release_date: day(days),
      poster: poster(title, (id * 47) % 360),
      backdrop: null,
      overview: `${title} is a made-up ${media} used to test Marquee's screens.`,
      runtime: media === 'movie' ? 116 : media === 'tv' ? 48 : null,
      rating: 7.8,
      popularity: 100 - id,
      match_pct: 96 - id * 3,
      why: id % 2 ? ['Because you rated *Test Arrival* 5★'] : ['Because you play a lot of *Fixture Wilds*'],
      genres: media === 'album' ? ['Electronic'] : ['Science Fiction', 'Drama'],
      meta: media === 'album' ? { artist: 'The Fixtures', album_type: 'album' } : media === 'game' ? { platforms: ['PC'], price: { cents: 2499, currency: 'USD' }, discount: 0 } : {},
      user_rating: null,
      done: false,
      saved: false,
      ...extra,
    };
  };
  const catalog: Card[] = [
    mk('movie', 'Signal From Europa', 2),
    mk('tv', 'The Long Orbit', -1, { meta: { seasons_count: 2, network: 'Test+' } }),
    mk('game', 'Hollow Lanterns', 5),
    mk('album', 'Neon Rain', 0),
    mk('movie', 'Paper Lighthouse', -3),
    mk('game', 'Tidewalker', 12, { meta: { platforms: ['PC'], price: { cents: 1999, currency: 'USD' }, discount: 40 } }),
    mk('tv', 'Quiet Harbor', 9),
    mk('album', 'Static Bloom', -6),
    mk('movie', 'Midnight Cartographers', 20),
    mk('game', 'Ember Atlas', -10),
    mk('album', 'Glass Weather', 15),
    mk('tv', 'Northlight', 30),
  ];
  const byId = (id: number) => catalog.find((c) => c.id === id);

  const settings: Record<string, any> = { onboarded: options.onboarded ?? true, region: 'US' };
  let hasTaste = options.hasTaste ?? true;
  const lists: { info: ListInfo; items: { id: number; sync_state: ListEntry['sync_state']; added_at: string }[] }[] = [
    ['watchlist', 'Watchlist'],
    ['play-later', 'Play Later'],
    ['listen-later', 'Listen Later'],
    ['favorites', 'Favorites'],
  ].map(([slug, name], i) => ({ info: { id: i + 1, slug, name, kind: 'system', mirror: null, count: 0, covers: [] }, items: [] }));
  const themes: Theme[] = [theme('marquee', 'Marquee', 'dark', '#0A0A0C', '#151518', '#F6F2EA', '#F2C14E'), theme('midnight', 'Midnight', 'dark', '#0B1020', '#141B30', '#EEF2FF', '#7AA2FF'), theme('pure-light', 'Pure Light', 'light', '#FFFFFF', '#F3F4F6', '#111318', '#B45309')];
  const sources: SourceStatus[] = [
    ['steam', 'Steam'],
    ['trakt', 'Trakt'],
    ['spotify', 'Spotify'],
    ['lastfm', 'Last.fm'],
    ['letterboxd', 'Letterboxd'],
    ['serializd', 'Serializd'],
    ['import', 'File import'],
  ].map(([id, name]) => ({ id, name, status: 'disconnected', account: null, last_sync: null, error: null, config: {}, configured: true, reads: ['History'], writes: id === 'trakt' ? ['Watchlist'] : [], note: `${name} test connection.` }));
  const notifications: Notification[] = [{ id: 1, kind: 'release', title: 'Neon Rain is out today', body: 'New album from The Fixtures', item_id: 4, created_at: new Date().toISOString(), read: false }];
  const overrides: { feature: string; label: string; mode: 'pin' | 'block' }[] = [];
  const dismissed = new Set<number>();

  const refresh = () => {
    for (const l of lists) {
      l.info.count = l.items.length;
      l.info.covers = l.items.slice(0, 4).map((i) => byId(i.id)?.poster ?? '');
    }
    for (const c of catalog) c.saved = lists.some((l) => l.items.some((i) => i.id === c.id));
  };
  const visible = () => catalog.filter((c) => !dismissed.has(c.id)).map((c) => (hasTaste ? c : { ...c, match_pct: null, why: [] }));
  const bootstrap = () => {
    refresh();
    return {
      version: '0.1.0',
      settings: { ...settings },
      sources: sources.map((s) => ({ ...s })),
      keys: ['tmdb', 'igdb_client_id', 'igdb_client_secret', 'lastfm_key', 'lastfm_secret', 'spotify_client_id', 'trakt_client_id', 'trakt_client_secret', 'steam', 'itad'].map((name) => ({
        name,
        what: `Test key for ${name}`,
        url: 'https://example.test',
        set: name !== 'itad',
        custom: false,
        built_in: name !== 'itad',
      })),
      lists: lists.map((l) => ({ ...l.info })),
      themes: themes.map((t) => ({ ...t })),
      system_light: themes[2],
      unread: notifications.filter((n) => !n.read).length,
      pending_writes: 0,
      has_taste: hasTaste,
      onboarded: !!settings.onboarded,
      data_dir: 'C:\\Users\\test\\AppData\\Roaming\\Marquee',
    };
  };
  const fail = (kind: string, message: string) => Promise.reject({ kind, message });

  async function call(method: string, p: any): Promise<any> {
    calls.push({ method, params: p });
    if (options.offline && ['search', 'providers', 'taste.seeds'].includes(method)) return method === 'search' ? { cards: [], offline: true } : fail('offline', "You're offline.");
    switch (method) {
      case 'bootstrap':
        return bootstrap();
      case 'settings.set':
        settings[p.key] = p.value;
        return null;
      case 'keys.set':
        return bootstrap().keys;
      case 'home': {
        const cards = visible();
        return {
          hero: cards.slice(0, 3),
          rows: [
            { id: 'week', title: 'Out This Week', subtitle: 'Fresh releases that fit your taste', cards: cards.filter((c) => c.release_date! <= day(7) && c.release_date! >= day(-7)) },
            { id: 'soon', title: 'Coming Soon', subtitle: 'On the way', cards: cards.filter((c) => c.release_date! > day(7)) },
          ],
          personal: hasTaste,
          counts: { items: catalog.length, events: hasTaste ? 12 : 0 },
        };
      }
      case 'for_you': {
        const media: string[] | undefined = p.filters?.media;
        const cards = visible().filter((c) => !media?.length || media.includes(c.media));
        return { cards, next: null, total: cards.length };
      }
      case 'hub':
        return { cards: visible().filter((c) => c.media === p.media), next: null, genres: [{ id: 'Science Fiction', count: 3 }], platforms: p.media === 'game' ? ['PC'] : [] };
      case 'tracked_shows':
        return [];
      case 'calendar':
        return visible()
          .filter((c) => c.release_date! >= p.from && c.release_date! <= p.to)
          .map((card) => ({ date: card.release_date, kind: 'release', label: card.title, card, pinned: false }));
      case 'quick_rate':
        return visible()
          .filter((c) => c.user_rating == null)
          .slice(0, p.limit ?? 24);
      case 'today':
        return visible().filter((c) => c.release_date === day(0));
      case 'item': {
        const card = byId(p.id);
        if (!card) return fail('error', "That title isn't in Marquee anymore.");
        refresh();
        const targets = [
          card.media === 'movie' && { id: 'letterboxd:watchlist', label: 'Letterboxd watchlist', hint: 'Opens the film page', ready: true },
          (card.media === 'movie' || card.media === 'tv') && { id: 'trakt:watchlist', label: 'Trakt watchlist', ready: sources[1].status === 'connected' },
          card.media === 'game' && { id: 'steam:wishlist', label: 'Steam wishlist', hint: 'Opens the store page', ready: sources[0].status === 'connected' },
          card.media === 'album' && { id: 'spotify:library', label: 'Spotify library', ready: sources[2].status === 'connected' },
        ].filter(Boolean);
        return {
          item: {
            id: card.id,
            media: card.media,
            title: card.title,
            year: card.year,
            release_date: card.release_date,
            poster: card.poster,
            backdrop: null,
            overview: card.overview,
            runtime: card.runtime,
            rating: card.rating,
            data: { genres: card.genres, artist: card.meta.artist, creators: [{ name: 'Test Director', role: 'director' }], cast: [], seasons: card.media === 'tv' ? [{ number: 1, name: 'Season 1', episodes: 2, air_date: day(-400), poster: null }] : undefined },
            ids: card.media === 'tv' ? { tmdb: '900' } : {},
            detail_level: 2,
          },
          card: { ...card },
          match_pct: hasTaste ? card.match_pct : null,
          why: hasTaste ? card.why : [],
          where:
            card.media === 'game'
              ? { price: card.meta.price, discount: card.meta.discount, store_url: 'https://example.test/store' }
              : card.media === 'album'
                ? { spotify: 'https://example.test/spotify', lastfm: 'https://example.test/lastfm' }
                : { providers: { stream: [{ id: 8, name: 'Test+', logo: null }], rent: [], buy: [], free: [], link: 'https://example.test/watch' } },
          similar: { same: catalog.filter((c) => c.media === card.media && c.id !== card.id), cross: { game: catalog.filter((c) => c.media === 'game' && c.id !== card.id).slice(0, 2) } },
          in_lists: lists.flatMap((l) => l.items.filter((i) => i.id === card.id).map((i) => ({ list_id: l.info.id, sync_state: i.sync_state }))),
          following: { item: false, artist: card.media === 'album' ? false : null, franchise: null },
          events: card.user_rating ? [{ source: 'marquee', kind: 'rating', value: card.user_rating, at: new Date().toISOString() }] : [],
          moods: [{ feature: 't:space', label: 'Space', weight: 1 }],
          targets,
          seriesgraph: card.media === 'tv' ? 'https://seriesgraph.com/show/900' : null,
          letterboxd: card.media === 'movie' ? 'https://letterboxd.com/tmdb/900' : null,
          offline: false,
          note: null,
        };
      }
      case 'item.episodes':
        return {
          seasons: [
            [
              { season: 1, number: 1, name: 'Pilot', air_date: day(-400), rating: 8.1, votes: 120 },
              { season: 1, number: 2, name: 'Drift', air_date: day(-393), rating: 6.4, votes: 98 },
            ],
          ],
          average: 7.3,
          source: 'TMDB',
        };
      case 'item.blurb':
        return { blurb: null };
      case 'search.local': {
        const q = String(p.query).toLowerCase();
        return { cards: catalog.filter((c) => c.saved && c.title.toLowerCase().includes(q)), lists: lists.filter((l) => l.info.name.toLowerCase().includes(q)).map((l) => ({ id: l.info.id, name: l.info.name })) };
      }
      case 'search': {
        const q = String(p.query).toLowerCase();
        return { cards: catalog.filter((c) => c.title.toLowerCase().includes(q) && (!p.media || c.media === p.media)), offline: false };
      }
      case 'vibe':
        return { cards: catalog.slice(0, 4), engine: 'tags', understood: ['cozy'], media: [], like: [] };
      case 'add': {
        const card = byId(p.id)!;
        const target = String(p.target);
        const def = { movie: 0, tv: 0, game: 1, album: 2 }[card.media];
        const list = target.startsWith('list:') ? lists.find((l) => l.info.id === Number(target.slice(5)))! : lists[def];
        if (!list.items.some((i) => i.id === card.id)) list.items.unshift({ id: card.id, sync_state: target.startsWith('list:') ? 'local' : 'pending', added_at: new Date().toISOString() });
        refresh();
        if (target === 'letterboxd:watchlist' || target === 'steam:wishlist') return { sync_state: 'pending', pushed: { state: 'opened', hint: `Opened ${card.title} so you can finish adding it.` } };
        return { sync_state: 'local', pushed: null };
      }
      case 'remove': {
        const list = lists.find((l) => l.info.id === p.list_id);
        if (list) list.items = list.items.filter((i) => i.id !== p.id);
        refresh();
        return { removed: true };
      }
      case 'rate': {
        const card = byId(p.id);
        if (card) card.user_rating = p.stars > 0 ? p.stars : null;
        hasTaste = true;
        return { pushed: [] };
      }
      case 'done': {
        const card = byId(p.id);
        if (card) card.done = !!p.done;
        return { pushed: [] };
      }
      case 'dismiss':
        if (p.reason !== 'seen') dismissed.add(p.id);
        return null;
      case 'undismiss':
        dismissed.delete(p.id);
        return null;
      case 'more_like':
      case 'follow':
      case 'unfollow':
        return null;
      case 'follows':
        return [];
      case 'lists':
        refresh();
        return lists.map((l) => ({ ...l.info }));
      case 'list': {
        refresh();
        const list = lists.find((l) => l.info.id === p.id);
        if (!list) return fail('error', "That list doesn't exist.");
        return { info: { ...list.info }, entries: list.items.map((i) => ({ ...byId(i.id)!, sync_state: i.sync_state, added_at: i.added_at, note: null })) };
      }
      case 'list.create': {
        const id = Math.max(...lists.map((l) => l.info.id)) + 1;
        lists.push({ info: { id, slug: `custom-${id}`, name: p.name, kind: 'custom', mirror: null, count: 0, covers: [] }, items: [] });
        return { id };
      }
      case 'list.rename':
        lists.find((l) => l.info.id === p.id)!.info.name = p.name;
        return null;
      case 'list.delete':
        lists.splice(
          lists.findIndex((l) => l.info.id === p.id),
          1,
        );
        return null;
      case 'list.reorder': {
        const list = lists.find((l) => l.info.id === p.id)!;
        list.items.sort((a, b) => p.item_ids.indexOf(a.id) - p.item_ids.indexOf(b.id));
        return null;
      }
      case 'list.mirror':
        lists.find((l) => l.info.id === p.id)!.info.mirror = p.mirror ?? null;
        return null;
      case 'list.bulk': {
        const list = lists.find((l) => l.info.id === p.id)!;
        if (p.action === 'remove' || p.action === 'move') list.items = list.items.filter((i) => !p.item_ids.includes(i.id));
        if (p.action === 'copy' || p.action === 'move') for (const id of p.item_ids) await call('add', { id, target: `list:${p.to}` });
        if (p.action === 'done') for (const id of p.item_ids) byId(id)!.done = true;
        refresh();
        return { count: p.item_ids.length };
      }
      case 'list.markdown': {
        const list = lists.find((l) => l.info.id === p.id)!;
        return `# ${list.info.name}\n\n${list.items.map((i) => `- **${byId(i.id)!.title}**`).join('\n')}\n`;
      }
      case 'taste':
        return {
          empty: !hasTaste,
          media: {
            movie: { genres: [{ feature: 'g:science-fiction', label: 'Science Fiction', weight: 1 }], themes: [{ feature: 't:space', label: 'Space', weight: 1 }] },
            tv: { genres: [], themes: [] },
            game: { genres: [{ feature: 'g:adventure', label: 'Adventure', weight: 1 }], themes: [{ feature: 't:space', label: 'Space', weight: 0.8 }] },
            album: { genres: [], themes: [] },
          },
          dna: [{ feature: 't:space', label: 'Space', weight: 1, support: 4, media: { movie: 0.5, tv: 0, game: 0.5, album: 0 }, spans: 2 }],
          creators: [{ feature: 'c:test-director', label: 'Test Director', role: 'director', weight: 1 }],
          decades: [{ decade: '2010s', media: { movie: 2, game: 1 }, total: 3 }],
          adventurous: 42,
          dial: settings.taste?.dial ?? 0.35,
          source_weights: settings.taste?.source_weights ?? {},
          sources: [{ source: 'marquee', events: 12 }],
          overrides: [...overrides],
          disliked: [],
          stats: {
            monthly: [{ month: day(0).slice(0, 7), watched: 6.5, listened: 12 }],
            played_hours: 120,
            played_recent_hours: 4.5,
            top_year: { movie: catalog.filter((c) => c.user_rating && c.media === 'movie'), tv: [], game: [], album: [] },
            year: new Date().getFullYear(),
            streak_weeks: 3,
            longest_streak_weeks: 5,
            counts: { rated: 12, watched: 30, games: 8, artists: 20 },
          },
          vocabulary: [
            { feature: 't:space', label: 'Space' },
            { feature: 't:cozy', label: 'Cozy' },
          ],
        };
      case 'taste.override': {
        const i = overrides.findIndex((o) => o.feature === p.feature);
        if (i >= 0) overrides.splice(i, 1);
        if (p.mode) overrides.push({ feature: p.feature, label: p.feature === 't:cozy' ? 'Cozy' : 'Space', mode: p.mode });
        return null;
      }
      case 'taste.reset':
        overrides.length = 0;
        return { removed: 3 };
      case 'taste.seeds':
        return { cards: catalog.map((c) => ({ ...c })), missing: [] };
      case 'taste.seed':
        hasTaste = hasTaste || (p.ids ?? []).length > 0;
        return { scored: catalog.length };
      case 'sources':
        return sources.map((s) => ({ ...s }));
      case 'connect.lastfm':
      case 'connect.letterboxd':
      case 'connect.steam_id':
      case 'connect.steam':
      case 'connect.spotify': {
        const id = method.split('.')[1];
        const s = sources.find((x) => x.id === id)!;
        s.status = 'connected';
        s.account = p.username ?? p.steamid ?? 'tester';
        s.config = { username: p.username };
        emit('sources:changed', { id });
        return { account: s.account };
      }
      case 'connect.trakt.start':
        return { user_code: 'TEST1234', verification_url: 'https://trakt.tv/activate', device_code: 'device', interval: 5, expires_in: 600 };
      case 'connect.trakt.poll':
        return { account: null };
      case 'connect.lastfm.auth_start':
        return { token: 'tok' };
      case 'connect.lastfm.auth_finish':
        return { account: 'tester' };
      case 'disconnect': {
        const s = sources.find((x) => x.id === p.source)!;
        Object.assign(s, { status: 'disconnected', account: null, last_sync: null, config: {} });
        return null;
      }
      case 'sync': {
        emit('sync:progress', { label: 'Reading your history', pct: 0.3 });
        await new Promise((r) => setTimeout(r, 15));
        emit('sync:progress', { label: 'Scoring new releases', pct: 0.8 });
        await new Promise((r) => setTimeout(r, 15));
        settings.last_sync = { at: new Date().toISOString(), offline: !!options.offline };
        for (const s of sources) if (s.status === 'connected') s.last_sync = new Date().toISOString();
        emit('sync:progress', { label: 'Up to date', pct: 1 });
        emit('sync:done', { offline: !!options.offline, events: 0, warnings: [] });
        return { offline: !!options.offline, events: 0, scored: catalog.length, warnings: [] };
      }
      case 'spotify.playlists':
        return [];
      case 'spotify.create_playlist':
        return { id: 'new-playlist' };
      case 'letterboxd.export_steps':
        return ['Open letterboxd.com and sign in.', 'Go to Settings → Data.', 'Click "Export your data".', 'Drop that zip here.'];
      case 'import.preview':
        return {
          columns: ['Title', 'Rating'],
          sample: [['Fixture Wilds', '10']],
          rows: 1,
          mapping: { media: 'game', title: 'Title', year: null, date: null, rating: 'Rating', rating_scale: 'auto', artist: null, artist_extra: null, imdb_id: null, tmdb_id: null, steam_id: null, status: null, liked: null, as_wishlist: false },
          recognised: null,
        };
      case 'import.run':
        hasTaste = true;
        return { events: 1, wishlist: 0, skipped: 0 };
      case 'import.letterboxd':
        return { counts: { ratings: 2 } };
      case 'notifications':
        return notifications.map((n) => ({ ...n }));
      case 'notifications.read':
        for (const n of notifications) if (p.id == null || n.id === p.id) n.read = true;
        return null;
      case 'notifications.clear':
        notifications.length = 0;
        return null;
      case 'providers':
        return [
          { id: 8, name: 'Test+', logo: null },
          { id: 9, name: 'Fixtureflix', logo: null },
        ];
      case 'ai.status':
        return { enabled: true, running: false, endpoint: 'http://localhost:11434', models: [], chat_model: null, embed_model: null, error: null };
      case 'storage':
        return { images_bytes: 52_000_000, images: 310, image_cap_bytes: 1_073_741_824, database_bytes: 4_200_000, cached_responses: 88, backups_bytes: 0, data_dir: 'C:\\Users\\test\\AppData\\Roaming\\Marquee' };
      case 'cache.clear':
        return { images_bytes: 0, images: 0, image_cap_bytes: 1_073_741_824, database_bytes: 4_200_000, cached_responses: 0, backups_bytes: 0, data_dir: 'C:\\Users\\test\\AppData\\Roaming\\Marquee' };
      case 'diagnostics':
        return 'Marquee 0.1.0 (test)\n';
      case 'backup.list':
        return [];
      case 'backup.create':
        return { path: 'C:\\backups\\marquee-test.marqueebackup' };
      case 'backup.preview':
        return {
          manifest: { version: '0.1.0', created_at: new Date().toISOString(), automatic: false, counts: { items: 12, ratings: 3, events: 9, lists: 4, list_items: 2, follows: 0, connections: 0 } },
          themes: [],
          current: { items: 12, ratings: 3, events: 9, lists: 4, list_items: 2, follows: 0, connections: 0 },
          path: p.path,
        };
      case 'backup.restore':
      case 'export.library':
      case 'export.ics':
      case 'theme.export':
      case 'theme.delete':
        return null;
      case 'export.calendar_file':
        return { path: 'C:\\Users\\test\\Marquee\\marquee.ics' };
      case 'data.delete_all':
        if (p.confirm !== 'DELETE') return fail('error', 'Type DELETE to confirm.');
        settings.onboarded = false;
        hasTaste = false;
        for (const l of lists) l.items = [];
        return null;
      case 'themes':
        return themes.map((t) => ({ ...t }));
      case 'theme.duplicate': {
        const src = themes.find((t) => t.id === p.id)!;
        const copy = { ...src, id: `${src.id}-copy`, name: `${src.name} copy`, builtin: false, colors: { ...src.colors }, marquee: { ...src.marquee } };
        themes.push(copy);
        return copy;
      }
      case 'theme.save': {
        const t = p.theme as Theme;
        const i = themes.findIndex((x) => x.id === t.id);
        if (i >= 0) themes[i] = t;
        else themes.push(t);
        return t;
      }
      case 'theme.import':
        return themes[1];
      default:
        return fail('error', `Marquee doesn't have an action called "${method}".`);
    }
  }

  async function invoke(cmd: string, args: Record<string, any> = {}): Promise<any> {
    switch (cmd) {
      case 'call':
        return call(args.method, args.params ?? {});
      case 'system_info':
        return {
          version: '0.1.0',
          accent: '#3B82F6',
          portable: false,
          data_dir: 'C:\\Users\\test\\AppData\\Roaming\\Marquee',
          default_data_dir: 'C:\\Users\\test\\AppData\\Roaming\\Marquee',
          log_dir: 'C:\\Users\\test\\AppData\\Roaming\\Marquee\\logs',
          hangar_installed: false,
          deferred_to_hangar: false,
          crashed: null,
          platform: 'windows',
        };
      case 'update_state':
        return { available: null, channel: 'stable', last_check: null, hangar_installed: false, deferred_to_hangar: false, install_on_launch: null, skipped: [], can_roll_back: false, previous_version: null };
      case 'update_check':
        return null;
      case 'app_ready':
        return {};
      default:
        // Window, tray and updater commands have nothing to do outside the desktop app.
        return null;
    }
  }

  async function listen(event: string, handler: Handler): Promise<() => void> {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(handler);
    return () => listeners.get(event)?.delete(handler);
  }

  return { invoke, listen, emit, calls };
}
