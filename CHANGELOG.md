# Changelog

All notable changes to Marquee are listed here. Versions follow [Semantic Versioning](https://semver.org).

## 0.1.3

### Faster, smoother artwork
- Posters and covers fade in over a shimmer placeholder instead of popping in, and art that's already cached shows straight away.
- Grids load TMDB's 342 px posters (sharp on high-DPI screens, about a third the size); detail pages and Quick Rate still use full-size art.
- Cached images are served straight from disk without waiting on the database, so scrolling a full grid no longer stalls.
- The hero and detail artwork are fetched first.

### Missing posters and "TBA" dates fixed
- Titles from your Letterboxd, Trakt and Steam history are now looked up eight at a time, and more of them are filled in each sync.
- Titles skipped because a catalog key was missing were wrongly marked "not found" for a week. They are now retried on this update and whenever you add a key.
- Saving a TMDB, IGDB or Last.fm key starts a sync at once.
- Movies, TV, Games and Music show a banner with a shortcut to Settings → API keys when the key that fills them is missing.

## 0.1.2

### New

- **MyAnimeList.** Connect with your username to bring your anime list, scores and Plan to Watch into your taste profile, or sign in to also add anime to Plan to Watch, mark them completed and score them from Marquee. Each season's new anime appears in the feeds and calendar, and anime pages link to MyAnimeList with its score.
- **IMDb episode ratings, the data SeriesGraph charts.** Show heatmaps now use IMDb's episode ratings, downloaded from IMDb's public datasets once a week and kept on your PC. The show's IMDb rating appears beside the heatmap. Turn it off in Settings → General.
- Anime genres and themes (slice of life, isekai, shounen, iyashikei and more) now feed the cross-media taste, so a love of anime can surface games and music with the same mood.

### Fixed

- The "View episode ratings graph on SeriesGraph" button now opens the show's page in SeriesGraph's own address format.
- The "What's new" dialog for an update now shows the full release notes instead of one line.

## 0.1.1

The first release. (There is no 0.1.0 build: that tag was taken before the code was ready.)

### Your feed

- Home, "Now Showing": a hero carousel of the week's biggest releases for your taste, and rows for Out This Week, Coming Soon, New Seasons of Shows You Watch, New From Artists You Love, Games You Wishlisted Are Out or On Sale, Trending, and Hidden Gems For You.
- For You: an endless mixed feed with filters for type, genre, released or upcoming, length, your streaming services, and price or on sale.
- Quick Rate: rapid-fire cards to rate or skip, by mouse or keyboard.
- Movies, TV, Games and Music hubs with filters and sorting. TV tracks the shows you watch with next-episode countdowns; Games shows Steam price, discount, lowest price and Steam Deck status; Music shows albums, EPs and singles from artists you follow and artists like them.
- Release Calendar with month, week and agenda views, follows for titles, artists and franchises, and export to an .ics file (one-off, or a file kept up to date for your calendar app to subscribe to).
- Detail pages with trailer, cast and crew, gallery, where to watch, buy or listen, why you would like it, similar titles from every medium, and an episode-rating heatmap for shows with a link to SeriesGraph.
- Global search across every medium and your lists, and vibe search: describe a mood and get matches across media.
- Ctrl+K command palette for search, navigation, adding to lists, switching theme and syncing.

### Your taste

- A taste engine that runs entirely on your PC. It turns ratings, likes, playtime, scrobbles, history and wishlists into one profile, with a shared vocabulary of moods and themes so a film you love can suggest a game or an album.
- Every pick has a match percentage and one or two reasons drawn from your own history.
- Not interested (seen it, not my genre, dislike this creator) and More like this, both of which change your picks straight away.
- A taste dial from Comfort to Adventurous.
- Your Taste: top genres and themes per medium, a cross-media taste DNA chart, favorite creators, decades, how adventurous you are, hours per month, your top of the year, streaks, and controls to pin, block, re-weight or forget.
- Optional local AI through Ollama: natural-language vibe search, written "why you'd like this" blurbs and local embeddings. Nothing leaves your machine.

### Connections

- Steam: sign in through Steam, read your library, playtime and wishlist. Adding to the wishlist opens the store page and is confirmed on the next sync.
- Trakt: device-code sign-in; reads history, ratings, watchlist and collection; writes watchlist, watched and ratings.
- Spotify: sign-in with PKCE; reads top artists and tracks, saved albums, followed artists and playlists; saves albums, follows artists, adds to playlists and keeps a "Marquee Picks" playlist.
- Last.fm: reads scrobbles, top artists and albums, loved tracks and tags from your username; optional sign-in to love tracks.
- Letterboxd: reads your public RSS feed and imports your full data export. Adding to the watchlist opens the film page.
- Import anything from CSV or JSON with a column mapper; IMDb, RateYourMusic, Trakt, Letterboxd and Serializd files are recognised automatically.

### Lists

- Watchlist, Play Later, Listen Later and Favorites, plus any number of custom mixed-media lists.
- A list can mirror your Trakt watchlist, your Steam wishlist or a Spotify playlist, with a sync state on every entry.
- Drag to reorder, bulk actions, sort and filter, and share a list as an image card or Markdown.

### The app

- Windows toast notifications for release days, new seasons, new albums, wishlist sales over a threshold you choose, and a weekly digest, with quiet hours and an in-app notification center.
- A tray icon with a mini panel: today's releases, quick search, sync now and quit. Optional launch at startup, minimized to the tray.
- Eight built-in themes (Marquee, Midnight, Cinema Red, Slate Light, Pure Light, OLED Black, Synthwave, Forest) plus Follow Windows, a live theme editor, and .marqueetheme import and export.
- Motion settings (Full, Reduced, Off), three densities, and UI scale from 90% to 150%. Full keyboard navigation and rebindable shortcuts.
- Works offline from what it has saved, and queues list adds and ratings until you reconnect.
- Backup and restore to a single .marqueebackup file, automatic backups, full library export to CSV or JSON, and a "Delete all my data" button.
- Built-in signed updater with Stable and Beta channels, "Install now" or "On next launch", and automatic rollback if a new version fails to start.
- Hangar integration: follows the suite theme, lets Hangar trigger updates, backups and restores, and defers update prompts to Hangar when it is managing them.
- Sign-ins and API keys are stored in Windows Credential Manager. There is no Marquee server and no telemetry.
