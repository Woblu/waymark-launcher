# Waymark

A desktop launcher for Minecraft: Java Edition with an undo button. Every change to an instance leaves a waymark you can return to, so a bad mod update can never cost you your setup.

Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

## What's here

| Path | What it is |
|---|---|
| `crates/waymark-core` | The launcher itself, with no UI. Everything worth testing lives here. |
| `crates/waymark-cli` | A command line over the core, for scripting and for proving each part works. |
| `apps/desktop` | The desktop app: Tauri 2, Svelte 5 and a thin bridge to the core. |
| `apps/site` | The public website, plain HTML and CSS, deployed to Vercel. |

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

Run this before calling a feature done. Unit tests have missed real bugs here, like a crash search that found the mod and then hid the answer.

`apps/desktop/e2e/playing.mjs` covers the window closing during play and coming back afterwards, including after a crash.

## Building a release

See [docs/RELEASING.md](docs/RELEASING.md). CurseForge access and update signing both come from environment variables, so no key ever enters the repository. The Azure client ID in `crates/waymark-core/src/brand.rs` is public by design.

## Rules this project keeps

- **No gameplay without verified ownership.** A launch needs a `Session`. Release builds only get one through a signed-in account whose ownership was confirmed. `Session::development` exists in debug builds only. Mojang won't approve an app that bypasses this.
- **Game files are never redistributed.** Sync and sharing move the lockfile, and the game comes from Mojang's servers. Server exports stay on the player's machine, and the player accepts the EULA themselves.
- **Respect CurseForge authors.** When a file has no download URL, the author has opted out of third-party apps, so the player is sent to CurseForge.
- **Pack files are untrusted.** Paths are checked with `safe_relative` and download hosts against the `.mrpack` allow list.
- **One name.** Every product name lives in `crates/waymark-core/src/brand.rs`.

## Forking

Forks need their own identity:

- **Azure client ID.** The one in `brand.rs` is registered to this project, and Mojang reviews each app's ID separately. Register your own and apply for your own approval.
- **CurseForge key.** Request your own and set `WAYMARK_CURSEFORGE_API_KEY` when you build.
- **Update key and endpoint.** Generate your own, or your builds will look for updates from this project.
- **Name.** Change the product name in `brand.rs`, so people can tell your build from this one.

## License

Waymark is licensed under the [GNU General Public License v3.0](LICENSE). The Libre Franklin font is licensed under the [SIL Open Font License 1.1](apps/site/assets/fonts/OFL.txt).
