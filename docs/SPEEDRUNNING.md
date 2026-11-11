# Waymark Launcher and speedrunning

This page is for the people who review launchers for speedrun.com's Minecraft: Java Edition leaderboards and for MCSR Ranked, and for runners who want to check for themselves. It lists everything Waymark Launcher does when it starts the game, from 0.4.3 on, with the rules each part touches.

Waymark Launcher is open source under GPL-3.0, so all of this can be read in the code: [github.com/Woblu/waymark-launcher](https://github.com/Woblu/waymark-launcher). The launch itself is built in [`crates/waymark-core/src/launch.rs`](../crates/waymark-core/src/launch.rs).

## Sign-in

Every launch needs a Microsoft account that owns Minecraft. Waymark Launcher signs in through Microsoft, confirms ownership with Mojang, and gives the game the account's real name, UUID and access token, as the official launcher does. Release builds have no offline mode.

## Where every file comes from

| Files | Source |
|---|---|
| Version files, game jars, libraries, natives and assets | Mojang's servers, each file checked against the hash Mojang publishes |
| Java | Mojang's own Java runtimes, the ones the official launcher uses, unless the player sets a Java path |
| Fabric, Quilt, Legacy Fabric and Ornithe | The profile each project publishes on its own meta server, and the libraries that profile names |
| Forge and NeoForge | Their own installers |
| Mods, resource packs and shaders | Only what the player adds: from Modrinth, from CurseForge, or a file they drop on the window |

Mods are kept once in a read-only store and placed into each instance from there, so a jar stays byte for byte the file that was downloaded or dropped in.

## What goes on the command line

The version's own file decides the main class, the classpath and the game's arguments: Mojang's file, or a loader's profile laid over it. Waymark Launcher adds only these:

- `-Xms` and `-Xmx`, from the player's memory setting.
- The G1 garbage collector flags the official launcher uses by default.
- `-Dtoolscreen.minecraftVersion=<version>`, so Toolscreen can tell which version is running. The game never reads it, and it isn't one of the game's debug properties (A.8.18).
- `--width` and `--height`, or `--fullscreen`, if the player set a window size.
- `--quickPlaySingleplayer` or `--quickPlayMultiplayer` (1.20 and newer), or `--server` and `--port` (older versions), only when the player opens a world or server straight from Waymark Launcher.
- Before 1.6 only: `-Duser.home`, and `APPDATA` on Windows, point at the instance's folder, because those versions open a log in the real `.minecraft` before anything can move it.
- Whatever the player typed into the instance's Java arguments, environment variables, wrapper command, and commands to run before launching and after the game closes.

Mojang's version files from 1.13 on fill `-Dminecraft.launcher.brand` with `${launcher_name}`, which Waymark Launcher sets to `Waymark Launcher`, so the game and mods can always tell which launcher started them.

Waymark Launcher never adds a Java agent (A.8.19), never attaches a debugger, and never patches, replaces or adds a library that the version file doesn't name (A.8.17). It doesn't touch LWJGL 3 or GLFW (A.8.4). The exceptions follow, with what changed in 0.4.3.

## Exceptions

### Minecraft 1.0 to 1.5.2

Mojang's own releases from 1.0 to 1.5.2 launch from Mojang's own version files.

Waymark Launcher can also run them through [LegacyFix](https://github.com/betacraftuk/legacyfix), BetaCraft's patches, which bring back skins and fix bugs these versions have on today's computers. LegacyFix is a Java agent and uses Legacy Fabric's LWJGL, so it's off unless the player turns on **Run with LegacyFix** in the instance's settings. The setting says speedrun.com won't accept runs made with it. Instances made before 0.4.3 keep LegacyFix until the player turns it off.

Builds Mojang doesn't list, like Classic, Indev, Infdev, the alphas, the betas and the pre-releases, always run through LegacyFix. Runs on those aren't allowed anyway (A.1.2).

### Legacy Fabric

Legacy Fabric's profiles replace Mojang's LWJGL 2 with Legacy Fabric's build. From 0.4.3, Waymark Launcher keeps Mojang's (A.8.3.a). The one exception is Apple silicon Macs, where Mojang never built LWJGL 2 and the game can't start without Legacy Fabric's.

### Forge 1.6.4 and older

Old Forge instances get Forge's own `legacyjavafixer` mod, because Forge from before Java 8 crashes on it, and the libraries old FML would otherwise try to download at startup, a download that stops the game. This only happens for Forge instances.

## What runs beside the game

- Waymark Launcher reads the game's output for its log viewer and crash help, and watches the process to know when the game closes. It doesn't inject anything into the game or draw anything over it.
- Each instance has its own game folder, so two instances never share one (A.9.1). Resource packs, shader packs and screenshots are only shared between instances when the player chooses.
- Shared game settings are off by default. When they're on, Waymark Launcher copies option values between instances as each starts and closes. It only copies values the game itself wrote, so nothing goes past what the game's own settings allow (A.1.4).
- Discord rich presence runs in the launcher, not the game, and is off unless the player turns it on.

## For runners

- Get speedrun mods from [mc.sr/mods](https://mc.sr/mods). The rules only allow the exact jars in legal-mods (A.8.5), and a mod from Modrinth or CurseForge can be a different build. Drop the jars onto Waymark Launcher's window with the instance open, and they go in unchanged.
- On 1.0 to 1.5.2, leave **Run with LegacyFix** off.
- Check the current list of allowed launchers in rule A.1.1 before you submit a run.
