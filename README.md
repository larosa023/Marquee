# Marquee

Marquee is a Windows 11 desktop app that tracks new releases across **movies, TV, games and music**, learns your taste from the services you already use, recommends things you'll actually like, and lets you add anything to the right wishlist, watchlist or playlist.

It answers three questions:

1. **What's new?** Every new or upcoming movie, show, season, game and album worth knowing about.
2. **What would I like?** Personal picks, each with a plain reason ("Because you rated *Arrival* 5★ and play a lot of *Outer Wilds*").
3. **How do I save it?** One click to your Letterboxd watchlist, Steam wishlist, Spotify playlist, Trakt watchlist, or Marquee's own lists.

Taste crosses media: a film you love can suggest a game or an album with the same mood.

Marquee is part of the [Hangar](https://github.com/larosa023/Hangar) suite and also works fully by itself. Everything stays on your PC: there is no Marquee server and no telemetry.

## Screenshots

_Screenshots go here once the first release is installed. Suggested set: Home, a detail page, Your Taste, the Release Calendar, Settings → Appearance._

## Features

- **Home, "Now Showing"**: hero carousel and rows for Out This Week, Coming Soon, New Seasons of Shows You Watch, New From Artists You Love, Games You Wishlisted Are Out or On Sale, Trending, and Hidden Gems For You.
- **For You**: an endless mixed feed with filters, and **Quick Rate** for tuning your profile fast.
- **Release Calendar**: month, week and agenda views; follow titles, artists and franchises; export to `.ics`.
- **Movies, TV, Games, Music** hubs: next-episode countdowns, Steam prices and lowest price, Steam Deck status, new music from artists you follow.
- **Detail pages**: trailer, cast and crew, where to watch or buy, why you'd like it, similar titles across every medium, and an episode-rating heatmap for shows.
- **Lists**: Watchlist, Play Later, Listen Later, Favorites and custom lists; mirror a Trakt watchlist, Steam wishlist or Spotify playlist; share as an image or Markdown.
- **Your Taste**: what Marquee has learned, shown plainly, with pins, blocks and per-source weights.
- **Search** across everything, and **vibe search** ("a short cozy game for a rainy Sunday").
- **Anime from MyAnimeList**: your list, scores and Plan to Watch feed your taste, each season's new anime shows up in the feeds, and you can add, complete and score anime from Marquee.
- **IMDb episode ratings**, the same numbers SeriesGraph charts, in every show's episode heatmap.
- **Optional local AI** through [Ollama](https://ollama.com): natural-language vibe search, written blurbs, local embeddings.
- Windows notifications, a tray panel, a Ctrl+K command palette, eight themes plus a live theme editor, backup and restore, and a signed updater with rollback.

The full list is in [CHANGELOG.md](CHANGELOG.md). What a third-party API won't allow, and what Marquee does instead, is in [KNOWN_LIMITATIONS.md](KNOWN_LIMITATIONS.md).

## Install

Download the latest `Marquee_<version>_x64-setup.exe` from [Releases](https://github.com/larosa023/Marquee/releases) and run it. It installs for your user only, with no admin prompt. A portable zip is there too: unzip it anywhere and run `Marquee.exe`; it keeps its data in the `data` folder beside it.

Requires Windows 11 (or Windows 10 with the WebView2 runtime, which the installer fetches if it is missing).

## API keys

Marquee talks to each service directly from your PC, and each service wants a key that identifies the app. Release builds can have keys built in; any key can also be entered in **Settings → API keys**, where it is stored in Windows Credential Manager.

| Key | Used for | Where to get it | Notes |
|---|---|---|---|
| `TMDB_API_KEY` | Movies and TV | [themoviedb.org/settings/api](https://www.themoviedb.org/settings/api) | Free. Use the "API Key (v3 auth)" value. |
| `IGDB_CLIENT_ID`, `IGDB_CLIENT_SECRET` | Games | [dev.twitch.tv/console/apps](https://dev.twitch.tv/console/apps) | Register an app (category "Application Integration", any redirect URL such as `http://localhost`), then create a client secret. Your Twitch account needs two-factor sign-in turned on. |
| `LASTFM_API_KEY`, `LASTFM_API_SECRET` | Music recommendations, scrobbles | [last.fm/api/account/create](https://www.last.fm/api/account/create) | Free. No callback URL needed. |
| `SPOTIFY_CLIENT_ID` | Spotify sign-in | [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard) | Redirect URI: `http://127.0.0.1:43117/spotify`. Tick "Web API". No client secret is used. See the Spotify note in KNOWN_LIMITATIONS.md. |
| `TRAKT_CLIENT_ID`, `TRAKT_CLIENT_SECRET` | Trakt sign-in | [trakt.tv/oauth/applications](https://trakt.tv/oauth/applications) | Redirect URI: `urn:ietf:wg:oauth:2.0:oob`. |
| `STEAM_API_KEY` | Your Steam library and wishlist | [steamcommunity.com/dev/apikey](https://steamcommunity.com/dev/apikey) | Any domain name works. |
| `MAL_CLIENT_ID` | Your MyAnimeList list and each season's new anime | [myanimelist.net/apiconfig](https://myanimelist.net/apiconfig) | Click "Create ID". App Type: **other**. App Redirect URL: `http://127.0.0.1:43117/mal`. Homepage URL: `https://github.com/larosa023/Marquee`. Fill in the rest however you like. An "other" app has no secret; `MAL_CLIENT_SECRET` is only for a "web" app. |
| `ITAD_API_KEY` | All-time low game prices (optional) | [isthereanydeal.com/apps/my](https://isthereanydeal.com/apps/my/) | Without it Marquee shows the lowest price it has seen itself. |

Movies and TV need TMDB, games need IGDB, and music needs Last.fm. A missing key only switches off what depends on it; the screen says which key to add.

Keys built into a release are inside the installer and can be extracted by anyone determined to. That is normal for desktop apps, and all of these are low-privilege app identifiers, but if you'd rather not ship yours, leave the GitHub secrets empty and enter the keys in Settings on your own PC instead.

## Building from source

You need [Node 22](https://nodejs.org), [Rust](https://rustup.rs) (stable) and the [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/) (the MSVC build tools and WebView2).

```powershell
git clone https://github.com/larosa023/Marquee
cd Marquee
npm ci
copy .env.example .env      # then fill in the keys you have
npm run tauri dev           # run it
npm run tauri build         # make the installers in target\release\bundle
```

Useful commands:

| Command | What it does |
|---|---|
| `npm run check` | Type-check, lint, format check and component tests |
| `cargo test --workspace` | Engine tests (no network: API clients run against saved fixtures) |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lint |
| `npm run test:e2e` | Browser smoke tests against a fake engine |
| `npm run dev:mock` | The UI in a browser with fake data, for working on screens |
| `npm run icons` | Regenerate every icon size from the drawing in `scripts/make-icons.mjs` |

## How it is put together

```
crates/marquee-core/   The engine. No window code; everything here runs in tests.
  integrations/        One module per service (Steam, Trakt, Spotify, Last.fm, Letterboxd, file import)
                       behind a shared `Source` trait
  catalog/             TMDB, IGDB, Steam store, MusicBrainz, Last.fm, Cover Art Archive
  taste/               Profile, tag vocabulary, scoring, reasons, vibe search
  sync/                Applying history, pushing writes, the offline queue
  notifications/ updater.rs hangar.rs backup.rs themes.rs db/ ...
  api.rs               The one function the UI calls: dispatch(method, params)
src-tauri/             The Windows app shell: window, tray, toasts, Credential Manager, updater, Hangar pipe
src/                   The UI (React + TypeScript)
  features/<screen>/   One folder per screen
  ui/                  Shared components
tests/                 Fake engine for UI tests, component test setup, Playwright smoke tests
```

Your data lives in `%LOCALAPPDATA%\com.larosa023.marquee\data` (or wherever you move it in Settings → Storage): a SQLite database, cached artwork, backups, custom themes and logs. Sign-ins and API keys are in Windows Credential Manager under the service name `Marquee`.

## Releasing

One-time setup:

1. **Update signing key.** From the repo root run `powershell -ExecutionPolicy Bypass -File scripts\setup-signing.ps1`. It creates the key pair, writes the public key into `src-tauri/tauri.conf.json` (commit that), and stores `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as repository secrets. Back up `%USERPROFILE%\.tauri\marquee.key` and its password; without them, installed copies can't be updated in place.
2. **API keys as secrets** (optional, see above): `TMDB_API_KEY`, `IGDB_CLIENT_ID`, `IGDB_CLIENT_SECRET`, `LASTFM_API_KEY`, `LASTFM_API_SECRET`, `SPOTIFY_CLIENT_ID`, `TRAKT_CLIENT_ID`, `TRAKT_CLIENT_SECRET`, `STEAM_API_KEY`, `ITAD_API_KEY`, `MAL_CLIENT_ID`, `MAL_CLIENT_SECRET`.
3. **Code-signing certificate** (optional): `WINDOWS_CERTIFICATE` (a base64-encoded `.pfx`) and `WINDOWS_CERTIFICATE_PASSWORD`. Without one the installer works but Windows SmartScreen warns the first time it runs. Updates are signed and verified either way.

Each release:

```powershell
npm run set-version -- 0.2.0     # package.json, Cargo.toml, tauri.conf.json, hangar.json
# add a "## 0.2.0" section to CHANGELOG.md
git commit -am "Marquee 0.2.0"
git push origin main
```

That's all: no tag, nothing to click. When the version on `main` hasn't been released yet, the Release workflow builds the installer, MSI and portable zip, signs the update, and publishes the GitHub Release with that changelog section as its notes. Installed copies see it within six hours (or at their next launch) and offer "Install now". A version with a suffix (`0.2.0-beta.1`) is published as a pre-release and only reaches people on the Beta channel.

### Testing the updater

1. `npm run set-version -- 0.0.9`, then `npm run tauri build` with `TAURI_SIGNING_PRIVATE_KEY` (and its password) set in your shell. Install the resulting `Marquee_0.0.9_x64-setup.exe`. Don't commit the version change.
2. Publish the new version as above.
3. Open the installed 0.0.9: within a few seconds an "Update ready" pill appears in the top bar. Choose "Install now"; Marquee restarts as the new version on the screen you were on. Settings → Updates then shows "Roll back" to 0.0.9.

## Hangar

Marquee speaks Hangar's app protocol (version 1): it writes `%LOCALAPPDATA%\Hangar\run\marquee.json` while running, answers on the named pipe listed there, follows theme pushes, and lets Hangar trigger updates, backups and restores. When Hangar is installed and managing updates, Marquee hides its own update prompts (Settings → Updates has the switch).

Hangar finds apps through its own `manifest.json`. To list Marquee there, add the `hangarManifestEntry` object from [`hangar.json`](hangar.json) to the `apps` array in the Hangar repo. `hangar.json` also documents Marquee's command-line flags (`--quit`, `--minimized`, `--backup <file>`, `--restore <file>`).

## Credits

Movie and TV data from [TMDB](https://www.themoviedb.org) (this product uses the TMDB API but is not endorsed or certified by TMDB), with availability from JustWatch. Game data from [IGDB](https://www.igdb.com). Music data from [MusicBrainz](https://musicbrainz.org), the [Cover Art Archive](https://coverartarchive.org) and [Last.fm](https://www.last.fm). Prices and reviews from Steam. Episode and show ratings: information courtesy of IMDb (https://www.imdb.com), used with permission. Anime data from [MyAnimeList](https://myanimelist.net). Marquee is not affiliated with any of these services, or with Letterboxd, Trakt, Spotify, MyAnimeList, Serializd or SeriesGraph.

## License

[MIT](LICENSE)
