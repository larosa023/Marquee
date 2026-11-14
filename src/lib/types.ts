// The shapes the engine returns (see crates/marquee-core). Kept loose where the UI only passes data along.

export type Media = 'movie' | 'tv' | 'game' | 'album';

export interface Card {
  id: number;
  media: Media;
  title: string;
  year: number | null;
  release_date: string | null;
  poster: string | null;
  backdrop: string | null;
  overview: string | null;
  runtime: number | null;
  rating: number | null;
  popularity: number;
  match_pct: number | null;
  why: string[];
  genres: string[];
  meta: Record<string, any>;
  user_rating: number | null;
  done: boolean;
  saved: boolean;
}

export interface ListInfo {
  id: number;
  slug: string;
  name: string;
  kind: 'system' | 'custom';
  mirror: string | null;
  count: number;
  covers: string[];
}

export interface ListEntry extends Card {
  sync_state: 'local' | 'synced' | 'pending' | 'failed';
  added_at: string;
  note: string | null;
}

export interface SourceStatus {
  id: string;
  name: string;
  status: 'disconnected' | 'connected' | 'syncing' | 'error';
  account: string | null;
  last_sync: string | null;
  error: string | null;
  config: Record<string, any>;
  configured: boolean;
  reads: string[];
  writes: string[];
  note: string;
}

export interface ThemeColors {
  background: string;
  sidebar: string;
  surface: string;
  surfaceHover: string;
  surfaceStrong: string;
  text: string;
  subtext: string;
  faint: string;
  accent: string;
}

export interface Theme {
  schema: number;
  id: string;
  name: string;
  description: string;
  mode: 'dark' | 'light';
  backdrop: 'mica' | 'acrylic' | 'none';
  transparency: number;
  surfaceOpacity: number;
  colors: ThemeColors;
  marquee: { radius: number; controlRadius: number; blur: number; shadow: number; font: string; displayFont: string; motion: number; glow: number };
  builtin: boolean;
}

export interface KeyStatus {
  name: string;
  what: string;
  url: string;
  set: boolean;
  custom: boolean;
  built_in: boolean;
}

export interface Bootstrap {
  version: string;
  settings: Record<string, any>;
  sources: SourceStatus[];
  keys: KeyStatus[];
  lists: ListInfo[];
  themes: Theme[];
  system_light: Theme;
  unread: number;
  pending_writes: number;
  has_taste: boolean;
  onboarded: boolean;
  data_dir: string;
}

export interface Notification {
  id: number;
  kind: string;
  title: string;
  body: string;
  item_id: number | null;
  created_at: string;
  read: boolean;
  /** Somewhere to go, such as the title's "where to watch" page. */
  url?: string | null;
}

export interface Filters {
  media?: string[];
  genres?: string[];
  status?: 'all' | 'released' | 'upcoming';
  min_runtime?: number | null;
  max_runtime?: number | null;
  my_services?: boolean;
  on_sale?: boolean;
  /** Games at or near their lowest price on record. */
  near_low?: boolean;
  max_price?: number | null;
  platform?: string | null;
  wishlisted?: boolean;
  hide_done?: boolean;
  album_types?: string[];
  tracking?: boolean;
  my_artists?: boolean;
  sort?: string;
}

export interface CalendarEntry {
  date: string;
  kind: 'release' | 'episode' | 'premiere' | 'finale';
  label: string;
  card: Card;
  pinned: boolean;
}

export interface ApiError {
  kind: 'offline' | 'reauth' | 'not_configured' | 'api' | 'error';
  message: string;
}

export const MEDIA: { id: Media; label: string; plural: string }[] = [
  { id: 'movie', label: 'Movie', plural: 'Movies' },
  { id: 'tv', label: 'TV', plural: 'TV' },
  { id: 'game', label: 'Game', plural: 'Games' },
  { id: 'album', label: 'Music', plural: 'Music' },
];
