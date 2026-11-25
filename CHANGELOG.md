# Changelog

Every release of Waymark Launcher, newest first. Versions with a hyphen, like 0.5.0-beta.1, are betas that only reach players who turned on beta versions.

## 0.5.0 (24 November 2026)

Everything in 0.5.0-beta.1 below, and these.

### Added
- An instance's settings can show the full command Play runs, without starting the game, under Launching. In a terminal, `waymark command <instance>` prints the same. The access token and your user folder are left out.
- Make a cape from a picture, on the Skins page, and wear it in game in place of a cape you own. Capes you make stay in the cape list, so you can switch between them and the real one whenever you like. Only you see them: the game on your computer shows your picture, and other players still see the real cape. Pick the part of the picture that goes on the cape, the color of the edges, and whether the elytra gets the picture too, at up to 2048 × 1024. A cape can also be saved as a PNG.
- The 3D model on the Skins page can show a cape as an elytra.

### Changed
- The MCSR Ranked preset is gone. MCSR Ranked only starts from the launchers speedrun.com allows, so the instance it made couldn't start.
- The RSG and SSG presets say they're for practice, since speedrun.com doesn't accept runs made in Waymark Launcher.
- The friends server keeps sign-up addresses as a keyed hash that can't be reversed. Before, an IPv4 address could be recovered from its hash by trying every address.

## 0.5.0-beta.1 (12 November 2026)

### Added
- Import from the Minecraft Launcher: worlds, mods and settings are copied, and its sign-in files never are.
- Imported instances keep their time played and when you last played them.
- The library shows instances your other launchers have that Waymark Launcher doesn't, with Import all.
- Presets: pick what you want to play (performance, PvP, shaders, quality of life, building, speedrunning practice or an adventure modpack) and get an instance with the newest build of each mod that fits. A first run offers them too.
- Speedrunning presets install the exact jars on speedrun.com's legal-mods list, checked against its SHA-512 hashes.

### Changed
- Settings and instance settings list their sections down the left, and themes are picked from swatches.
- Your face in the title bar opens your accounts.

### Fixed
- Instances from the Modrinth App show up for import again. The Modrinth App changed how it stores them.

## 0.4.3 (12 November 2026)

### Changed
- Minecraft 1.0 to 1.5.2 launch from Mojang's own files, as speedrun.com's rules require. LegacyFix is a setting, Run with LegacyFix, and instances you already had keep it on.
- Legacy Fabric instances use Mojang's LWJGL instead of Legacy Fabric's, which speedrun.com doesn't allow. Apple silicon Macs keep Legacy Fabric's, because Mojang's doesn't run there.

### Added
- [docs/SPEEDRUNNING.md](docs/SPEEDRUNNING.md), a list of everything Waymark Launcher adds when it starts the game.

## 0.4.2 (8 November 2026)

### Fixed
- Toolscreen's resize and zoom hotkeys work again on Fabric, Quilt, NeoForge and Ornithe instances. Toolscreen 1.4.5 turns them off when it can't tell the Minecraft version, and it read these instances as 0.0.0. Waymark Launcher now passes `-Dtoolscreen.minecraftVersion`, unless you set it yourself.

## 0.4.1 (5 November 2026)

### Added
- The message after installing, removing or updating something has an Undo button.

### Changed
- Stored mods are read-only, so editing a mod in one instance can't change it in others or in their waymarks.
- Imports copy files from other launchers instead of linking to them.
- A Java runtime's file list is checked against the hash Mojang publishes for it.

### Fixed
- Hosting no longer offers friends an address they can't reach when your router is behind another one.

## 0.4.0 (5 November 2026)

### Added
- Beta versions, for players who turn them on in Settings.
- Copy diagnostics in Settings, under Help, gathers what a support email needs, with folder names and sign-in tokens removed.
- After the launcher itself closes unexpectedly, it says so the next time it opens.
- Packs that carry their own program files ask you to look before you play.
- Import all, and an empty library that finds instances from other launchers.

### Changed
- Settings, lockfiles and waymarks are written all at once, so a crash or power cut can't leave half a file.
- Reclaim space keeps every file an instance or waymark still needs, and stops instead of guessing when an instance can't be read.
- Two changes to the same instance wait for each other.
- The check after an update waits for its undo before Play can start, only undoes an update when the game says why it failed, and never undoes your own changes.
- Errors say what went wrong in plain words, stay until closed, and keep the technical part under Copy details.
- Keyboard focus is visible everywhere, dialogs work from the keyboard, and the light theme has more contrast.

### Fixed
- CurseForge downloads, which CurseForge started refusing to apps that don't send their key.
- A damaged settings file no longer stops the game from starting.
- A server whose start script went missing keeps its world.
- Game logs no longer stop early on Windows set to another language.
- The snapshot setting.

## 0.3.7 (5 November 2026)

### Changed
- Waymark is now called Waymark Launcher. Updating keeps the same folder, data, sign-in and settings, and moves the Start menu shortcut to the new name.

### Fixed
- Server pages put the name and buttons below the banner instead of over it.
- A narrow window no longer cuts your name in the title bar down to one letter.

## 0.3.6 (4 November 2026)

### Added
- A server list of the Java servers their owners publish on Modrinth, with Join that sets up the right instance.
- Discord rich presence, off until you turn it on.
- Shared game settings between instances, off until you turn it on.

### Changed
- Instance settings open from the three-dot menu on an instance.

### Fixed
- Instance settings ignored going back to the default memory, clearing a Java path, and undoing a saved change.

## 0.3.5 (4 November 2026)

### Fixed
- Your skin didn't show, because Mojang's skin addresses use http and the app only loads https.
- Opening Skins repeatedly made Mojang answer 429 Too Many Requests. Answers are now reused for a few minutes.

## 0.3.4 (4 November 2026)

### Added
- The account button shows your skin's face.

### Fixed
- Your skin showed as Steve after a restart, because each start signed in again and Mojang sometimes refused. The verified session is now kept until it runs out.

## 0.3.3 (4 November 2026)

### Fixed
- Pre-launch commands that start with a quoted path, which Windows ran without their quotes.
- Saving instance settings overwrote changes another tool had made meanwhile. Only the fields you edited change now.

### Added
- Pre-launch commands get `INST_JAVA`, and Prism-style `$INST_*` names work on Windows.

## 0.3.2 (4 November 2026)

### Added
- Starter mods: a list of mods, resource packs and shaders that each new instance on a Minecraft version gets.

### Fixed
- Changing your skin or cape failed with 401 Unauthorized. A sign-in Mojang refused for coming too often was taken for being offline.

## 0.3.1 (3 November 2026)

### Changed
- Vanilla instances have resource packs and data packs again. Only Mods and Shaders stay hidden.

By this release, Mojang had approved Waymark for Minecraft sign-in, so the game could start.

## 0.3.0 (2 November 2026)

### Added
- 215 builds Mojang doesn't list, back to RubyDung from May 2009, from Omniarchive and BetaCraft, with BetaCraft's LegacyFix.
- On Windows, after updating mods or a pack, the game starts out of sight to check it still loads, and the update is undone if it doesn't.
- Let friends join a server you host.
- LiteLoader, Rift, Cleanroom, and Forge back to Minecraft 1.2.5.
- Technic packs in Discover.

### Changed
- Vanilla instances drop the Mods, Resource packs and Shaders tabs.

### Fixed
- Minecraft before 1.6 wrote to your real `.minecraft` folder.

## 0.2.0 (1 November 2026)

The first public version. Minecraft sign-in was still waiting for Mojang's approval, so the game couldn't start.

- A history of every instance, so a bad update can be undone.
- Discover across Modrinth, CurseForge, Feed The Beast and ATLauncher.
- Mods, resource packs, shaders and data packs.
- Worlds and servers, a skin library and screenshots.
- Pack export, share codes and hosting a server from your own computer.
- Friends, with an optional account that needs no email.
- Light, dark and custom themes, and six other languages.
- Updates that never interrupt a game.
