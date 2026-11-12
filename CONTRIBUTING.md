# Contributing

Thanks for helping. Waymark Launcher has one maintainer, so bug reports with steps to reproduce, and small pull requests that do one thing, are the easiest to take in. For anything bigger, open an issue first so we can agree on the approach.

## Before a pull request

Run what CI runs:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p waymark-core

cd apps/desktop
npm run check         # svelte-check
npm run check:i18n    # every UI string has a translation row
```

If you changed how something behaves in the app, also run the end-to-end test described in the [README](README.md#end-to-end-test). Unit tests have missed real bugs here, like a crash search that found the mod and then hid the answer.

New text in the UI needs a row in `apps/desktop/src/lib/catalog.ts`, or it stays in English. Text a player or a store wrote is left alone, so mark it `selectable` or keep it inside `.rich`.

Commit messages say what changed for a player, in a plain sentence. Keep each commit to one change.

## Rules the code follows

These hold everywhere. A change that breaks one won't be merged, even if it works.

- **No gameplay without verified ownership.** A launch needs a `Session`, and release builds only get one through a signed-in account whose ownership Mojang confirmed. `Session::development` exists in debug builds only. Mojang won't approve an app that gets around this.
- **Game files are never redistributed.** Sync and sharing move the lockfile, and the game comes from Mojang's servers. Builds Mojang doesn't list come from Omniarchive and BetaCraft, pinned by hash in `crates/waymark-core/data/old-versions.json`, and need the same signed-in account as any other launch. Default skins load from Mojang's texture server. Server exports stay on the player's machine.
- **Launches stay documented.** [docs/SPEEDRUNNING.md](docs/SPEEDRUNNING.md) lists everything a launch adds. A change to `launch.rs`, or to the libraries a loader gets, updates that page in the same pull request.
- **The EULA is the player's to accept.** A hosted server won't start until they tick the box themselves. Nothing in the app or its tests may tick it for them.
- **CurseForge authors who opt out are respected.** When a file has no download URL, the author has turned off third-party downloads, and the player is sent to CurseForge.
- **Pack files and share codes are untrusted.** Paths are checked with `safe_relative`, download hosts against an allow list, and share codes only carry store ids. Launch commands come from the player's own settings and can never arrive in a pack or a code.
- **Store descriptions are untrusted.** Anyone can publish a project, so descriptions go through the allow list in `apps/desktop/src/lib/rich.ts`, and the end-to-end test attacks it. Don't widen that list or the security policy without adding an attack to the test.
- **Nothing is deleted straight away.** Instances and worlds go to the trash for 30 days, and anything that collects garbage has to keep what's in there.
- **Every network connection is in the privacy policy.** `apps/site/privacy.html` lists every service the app talks to. A change that adds one updates the policy in the same pull request. Log uploads need the player's click, with folder names and tokens removed first. Nothing goes to the friends server unless the player signed in to a Waymark Launcher account. Discord rich presence is off until the player turns it on, and only names a server that came from the server list.
- **Friends can't touch the database.** Tables in `supabase/migrations` have no policies, so the app can only call functions that act as the signed-in player. Don't add a policy or a public function without a reason, and keep the Minecraft sign-in out of it.
- **Tests stay out of sight.** Anything that opens the game or the app runs on a hidden desktop at low priority, because this is someone's gaming PC.
- **One name.** Every product name lives in `crates/waymark-core/src/brand.rs`.

## License

By contributing, you agree that your contribution is licensed under the [GPL 3.0](LICENSE), like the rest of the project.
