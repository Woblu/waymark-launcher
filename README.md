# Waymark Launcher

A launcher for Minecraft: Java Edition that keeps a history of every instance. Each change to an instance's mods and config files leaves a waymark you can go back to, so a bad mod update can be undone. Worlds aren't part of waymarks.

Download it from [waymark-launcher.vercel.app](https://waymark-launcher.vercel.app) or the [latest release](https://github.com/Woblu/waymark-launcher/releases/latest). [CHANGELOG.md](CHANGELOG.md) lists what changed in each version.

Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

## Status

Waymark Launcher was started in October 2026 and has one maintainer, Woblu. It's tested on Windows and Linux. The macOS builds haven't been tested on a real Mac. The six translations are machine-made. Expect rough edges, and please [open an issue](https://github.com/Woblu/waymark-launcher/issues) when you hit one.

## How it's built

Waymark Launcher is written with a lot of help from Claude, an AI model made by Anthropic. Woblu decides what goes in and tests each release on Windows and Linux before publishing it. What backs that up:

- The core has over 360 unit tests (`cargo test -p waymark-core`), and an end-to-end test drives the real app like a player would. Both are described below.
- Releases are built by GitHub Actions from this repository, and each one lists every file's hash in `SHA256SUMS.txt`.
- [docs/SPEEDRUNNING.md](docs/SPEEDRUNNING.md) lists everything a launch adds to the game's command line, with links to the code.

## What it promises players

- **You need to own the game.** Playing needs a Microsoft account whose ownership of Minecraft was confirmed with Mojang. Without a connection, an account confirmed in the last 30 days can still play (`may_play_offline` in [`auth.rs`](crates/waymark-core/src/auth.rs)). There's no username-only mode.
- **Game files come from Mojang.** Waymark Launcher doesn't host or redistribute them. Builds Mojang doesn't list come from Omniarchive and BetaCraft, pinned by hash in [`old-versions.json`](crates/waymark-core/data/old-versions.json).
- **Deleting isn't instant.** Deleted instances and worlds go to a trash folder for 30 days.
- **You can see what it runs.** An instance's settings, under Launching, show the full command Play runs. From a terminal, `waymark command <instance>` prints the same. The [privacy policy](https://waymark-launcher.vercel.app/privacy) lists every service it connects to.

## What's here

| Path | What it is |
|---|---|
| `crates/waymark-core` | The launcher itself, with no UI. Everything worth testing lives here. |
| `crates/waymark-cli` | A command line over the core, for scripting and for trying each part. |
| `apps/desktop` | The desktop app: Tauri 2, Svelte 5 and a thin bridge to the core. |
| `apps/site` | The website, plain HTML and CSS, deployed to Vercel. |
| `supabase` | The optional friends server: database migrations and one function. |

## How it works

An instance is a lockfile (`waymark.lock`) plus files in a content-addressed store, where every file is kept once under its BLAKE3 hash. A waymark is a small snapshot pointing into that store, which is why they're cheap to take and quick to return to. Worlds aren't tracked, because they change constantly and need their own backups.

## Developing

You need Rust 1.93 or newer, Node 22, and on Windows the Visual Studio C++ tools and WebView2.

```powershell
cargo test -p waymark-core            # the core's tests
cargo run -p waymark-cli -- versions  # try the core from a terminal

cd apps/desktop
npm install
npm run tauri dev                     # run the desktop app
npm run dev                           # the UI alone in a browser, with sample data
```

In a browser the UI runs on sample data, and `http://localhost:1420/#mods` style links open a specific screen. See `applyPreview` in `apps/desktop/src/lib/mock.ts`.

Set `WAYMARK_HOME` to use a throwaway data folder instead of your real one.

The launcher writes its own log to `logs/launcher.log` in the data folder (two files of up to 5 MB each), and a crash of the launcher itself leaves `logs/last-crash.txt`, which the app shows once on the next start. Set `WAYMARK_LOG=debug` for more detail. **Copy diagnostics** in Settings gathers both for a support email, with folder names and tokens removed.

## End-to-end test

`apps/desktop/e2e/run.mjs` drives the real app like a player: it creates an instance, adds mods, rolls back, launches the game, runs a crash search, imports, installs a pack and deletes, then checks each result on disk. It fails on any console error or security policy violation. It opens a Minecraft window during the launch step.

```powershell
cd apps/desktop
npm run dev                    # in one terminal

$env:WAYMARK_HOME = '<an empty folder>'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
..\..\target\debug\waymark-desktop.exe
node e2e/run.mjs               # in another terminal
```

`apps/desktop/e2e/playing.mjs` covers the window closing during play and coming back afterwards, including after a crash.

## Translations

The strings live in `apps/desktop/src/lib/catalog.ts`, one row each, and `i18n.ts` swaps them in as text is drawn. The translations were made by machine, so corrections from native speakers are very welcome. Messages from the Rust core are English only.

## Contributing and security

[CONTRIBUTING.md](CONTRIBUTING.md) has the checks to run and the rules the code follows. Report security problems privately, as [SECURITY.md](SECURITY.md) describes.

## Building a release

See [docs/RELEASING.md](docs/RELEASING.md). CurseForge access and update signing both come from environment variables, so no key ever enters the repository. The Azure client ID in `crates/waymark-core/src/brand.rs` is public by design.

## Forking

Forks need their own identity:

- **Azure client ID.** The one in `brand.rs` is registered to this project, and Mojang reviews each app's ID separately. Register your own and apply for your own approval.
- **CurseForge key.** Request your own and set `WAYMARK_CURSEFORGE_API_KEY` when you build.
- **Update key and endpoint.** Generate your own, or your builds will look for updates from this project.
- **Name.** Change the product name in `brand.rs`, so people can tell your build from this one.

## Thanks

Waymark Launcher stands on other people's work, and none of it is shipped inside the app. It's downloaded when needed.

- [Omniarchive](https://omniarchive.net/) recovered most of the builds from before 1.0, [BetaCraft](https://betacraft.uk/) lists them, and BetaCraft's [LegacyFix](https://github.com/betacraftuk/legacyfix) (GPL 3.0) brings back what they lost. [Bouncy Castle](https://www.bouncycastle.org/) lets LegacyFix make secure connections on old Java.
- [Prism Launcher](https://prismlauncher.org/) hosts the libraries old Forge downloads at startup, after their original host went away. Its `INST_*` variables are what tools like Toolscreen expect, so Waymark Launcher uses the same ones.
- [mcsr-meta](https://github.com/tildejustin/mcsr-meta) by tildejustin, and the [legal-mods](https://github.com/Minecraft-Java-Edition-Speedrunning/legal-mods) list it follows, decide what the speedrunning presets install.
- The mod loaders: [Fabric](https://fabricmc.net/), [Quilt](https://quiltmc.org/), [Legacy Fabric](https://legacyfabric.net/), [Babric](https://github.com/babric), [Ornithe](https://ornithemc.net/), [Forge](https://minecraftforge.net/), [NeoForge](https://neoforged.net/), [LiteLoader](https://www.liteloader.com/), [Rift](https://github.com/DimensionalDevelopment/Rift) and [Cleanroom](https://github.com/CleanroomMC/Cleanroom).
- [Modrinth](https://modrinth.com/), [CurseForge](https://www.curseforge.com/), [Feed The Beast](https://www.feed-the-beast.com/), [ATLauncher](https://atlauncher.com/) and [Technic](https://www.technicpack.net/) for their public APIs, and [mclo.gs](https://mclo.gs/) for log sharing.

## License

Waymark Launcher is licensed under the [GNU General Public License v3.0](LICENSE). The Libre Franklin font is licensed under the [SIL Open Font License 1.1](apps/site/assets/fonts/OFL.txt), and so is the Monocraft font used for skin name tags ([license](apps/desktop/public/licenses/monocraft-OFL.txt)). The cape maker's layout comes from James Harrison's [Minecraft Cape Creator](https://github.com/minecraftcapes/minecraft-cape-creator) for MinecraftCapes, under the [MIT License](apps/desktop/public/licenses/minecraft-cape-creator-MIT.txt).
