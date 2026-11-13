# Known limitations

Where a third-party service doesn't allow something, Marquee uses the best real alternative. This file lists each case, what Marquee does instead, and anything that has not been verified on a real machine yet.

## Services

### Steam

- **Adding to the wishlist is manual.** Steam has no supported way for an app to add to a wishlist. "Add to Steam wishlist" opens the game's store page in Steam (`steam://store/<appid>`, or the web store if Steam isn't installed) and marks the entry **Pending**. The next sync reads your wishlist and marks it **Synced** once the game is there.
- **Private profiles share nothing.** If "Game details" is private, Steam returns an empty library. Marquee shows how to make it public (Steam → profile → Edit Profile → Privacy Settings) instead of an error.
- **Hours per month aren't available for games.** Steam reports lifetime playtime and the last two weeks only, so Your Taste shows those two figures rather than a monthly chart for games.
- **Lowest price** is the lowest Marquee itself has seen unless you add an IsThereAnyDeal key, which supplies the all-time low.

### Letterboxd

- **No public API.** Marquee reads your public RSS feed (`letterboxd.com/<user>/rss/`) for recent diary entries, and imports the data-export zip (Settings → Connections → Letterboxd walks through it) for full history, ratings, likes and watchlist. The RSS feed only holds your most recent entries, so re-import the export now and then if you rate a lot on Letterboxd itself.
- **Adding to the watchlist is manual.** Marquee opens the film's Letterboxd page so you can click Watchlist, and saves the film to Marquee's Watchlist either way.
- A private Letterboxd account has no RSS feed; use the export.

### Trakt

- Trakt is the main way Marquee writes for movies and TV (watchlist, watched, ratings).
- **Creating a Trakt API app may need Trakt VIP.** Trakt's developer pages said in 2026 that new API applications are limited to VIP members. If you can't create one, Trakt can't be connected; Letterboxd, imports and Marquee's own lists still cover movies and TV.
- Trakt rates out of 10 and Marquee out of 5 with halves, so ratings convert exactly both ways.

### Spotify

Spotify's February 2026 rules for personal ("development mode") apps shape what is possible:

- The app owner needs **Spotify Premium**, and at most **five accounts** can use the app. Add each account under "User Management" in the Spotify dashboard.
- **Recommendations and audio features are not available** to new apps. Marquee doesn't use them: music picks come from Last.fm similarity, MusicBrainz tags and your own taste profile.
- **There is no "new releases" feed.** New music comes from MusicBrainz (release groups by the artists you follow and artists like them), with artwork from the Cover Art Archive.
- **30-second previews** come from Spotify's own embedded player on the album page, since preview URLs are no longer provided. It appears when Marquee has matched the album to Spotify.
- Spotify may reject a write (saving an album, adding to a playlist) for a development-mode app with a 403. Marquee then keeps the item in its own list, marks it **Not synced**, and says so.
- Spotify sign-ins for development-mode apps expire after about six months; Marquee asks you to reconnect when that happens.

### Last.fm

- Reading needs only your username. Loving tracks needs a one-time approval on Last.fm (Settings → Connections → Last.fm).
- Listening hours are estimated from scrobble counts at 3.5 minutes a track.

### Serializd

- **No public API and no official export.** Serializd isn't a live connection. If you have a CSV or JSON of your diary (from a support request or a community tool), "Import a Serializd file" reads it with the column mapper.

### SeriesGraph

- **No API, so Marquee uses SeriesGraph's source instead.** SeriesGraph draws its heatmaps from IMDb's episode ratings. IMDb publishes those as downloadable files for personal, non-commercial use, so Marquee downloads `title.ratings` and `title.episode` (about 60 MB) once a week and keeps every rated episode on your PC (about 25 MB, `imdb.db` in the data folder). Show heatmaps then use the same numbers SeriesGraph shows. Turn it off in Settings → General; heatmaps then fall back to TMDB's ratings, which have far fewer votes.
- The first download happens during a sync, so heatmaps use TMDB until it finishes. Settings → Storage has "Download now".
- IMDb's files don't include episode titles; Marquee takes those from TMDB. A show TMDB doesn't link to an IMDb id gets TMDB's ratings.
- Show pages also have a "View episode ratings graph on SeriesGraph" button. Its links use `seriesgraph.com/show/<TMDB id>-<name>`, the format SeriesGraph's own pages use.

### MyAnimeList

- **A username reads a public list; signing in is needed to change it.** MAL's API serves public lists with just the app's client ID. Adding to Plan to Watch, marking completed and scoring need you to sign in on MyAnimeList once (Settings → Connections → MyAnimeList). Signing in also reads a private list.
- **Seasons are folded into one show.** MAL lists each season (and part, and cour) separately; TMDB, Trakt and Letterboxd treat a show as one title. Marquee folds a franchise's TV entries into one show so it matches everywhere else: its score is the average of the seasons you scored, and it counts as watched if you've watched any season. Movies stay separate. Writes from Marquee (add, complete, score) go to the show's first entry on MAL.
- Title matching between MAL and TMDB is by English title and year. A few shows with very different English and Japanese names may not match; they still appear, with MAL's artwork and details.
- New seasons of anime (for example "Season 2") appear as their own releases in the feeds and calendar, with MAL's details, rather than being merged into the show.
- "Dropped" with no score counts as a mild "not for me". Shows marked Plan to Watch go to your Watchlist.

### Xbox, PlayStation, GOG, Epic

- **Not included.** None has an official library API for third-party desktop apps; the community routes rely on scraping or on signing in with your account password, which Marquee won't do. Bring those libraries in through the file importer instead (a Backloggd or spreadsheet export works).

### TMDB, IGDB, MusicBrainz

- Where-to-watch data is JustWatch's, through TMDB, and varies in completeness by country.
- MusicBrainz allows one request a second, so the first music sync for a large library takes a few minutes. Marquee paces itself and caches what it gets.

## Windows and the app

- **Snap layouts.** The custom title bar supports snapping by dragging, Win+arrow keys, and Win+Z. The snap-layout flyout that appears when hovering a standard maximize button does not appear on a custom-drawn one (a WebView2 limitation).
- **Clicking a Windows toast** brings up the notification center entry, but does not open the specific title in Marquee; use the bell inside Marquee for that.
- **Trailers** play through YouTube's privacy-enhanced embed. Videos whose owners block embedding show YouTube's "watch on YouTube" message; the button beside the player opens them in your browser.
- **Unsigned installer.** Updates are signed and verified with Marquee's own key. The installer itself is only Authenticode-signed if you add a code-signing certificate to the release workflow; without one, Windows SmartScreen shows a warning the first time it runs.
- **Built-in API keys can be extracted** from the installer by anyone determined to. See README → API keys.
- **Episode heatmap, calendar and feeds are as fresh as the last sync.** Offline, Marquee shows what it has saved and queues list adds and ratings until you reconnect.

## Not yet verified

The first release (0.1.1) was written on Linux build machines without a Windows desktop, real accounts or API keys. That means:

- **The app has not been run on Windows by its author.** The Windows-only code (Credential Manager storage, registry reads for the accent color and Hangar detection, the Hangar named pipe, the tray panel, toasts, Mica, autostart, the installer) compiles and passes its tests on GitHub's `windows-latest` runner, but nobody has yet clicked through it on a real desktop. Expect to find and fix a few things on first launch.
- **No live API calls were made.** Every API client is tested against hand-written fixtures that follow each service's documented response shapes, not against recorded live responses. A field that differs in practice will show up as a missing detail or a sync warning, and is a small fix in that service's module.
- **Performance targets (under 150 MB idle, cold start under 1.5 s) are unmeasured.** The design aims at them (lazy-loaded screens, a small first chunk, the window released after five minutes in the tray), but they need measuring on Windows.
- **The updater's upgrade from a test 0.0.9 build and automatic rollback have not been exercised end to end**; README → "Testing the updater" has the steps.
- **Hangar integration** follows the protocol as read from the Hangar source, and was not tested against a running Hangar.
