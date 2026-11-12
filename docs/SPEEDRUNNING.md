# Waymark Launcher and speedrunning

This page lists everything Waymark Launcher does when it starts the game, from 0.4.3 on, next to the speedrun.com Minecraft: Java Edition rules each part touches. If you find something here that doesn't match the code, please open an issue.

Waymark Launcher isn't on speedrun.com's list of allowed launchers (rule A.1.1). Runs made with it aren't accepted, and MCSR Ranked, which follows the same list, won't start from it.

The launch is built in [`crates/waymark-core/src/launch.rs`](../crates/waymark-core/src/launch.rs).

## Sign-in

Every launch needs a Microsoft account that owns Minecraft. Waymark Launcher signs in through Microsoft, confirms ownership with Mojang, and gives the game the account's name, UUID and access token.

If Microsoft's or Mojang's servers can't be reached, an account whose ownership was confirmed in the last 30 days can still play. The game then gets the account's name and UUID with `0` as the access token ([`auth.rs`](../crates/waymark-core/src/auth.rs), `may_play_offline`). Servers that check sign-ins won't accept that session. A sign-in server that answers with an error doesn't lead to this, only one that can't be reached.

## Where every file comes from

| Files | Source |
|---|---|
| Version files, game jars, libraries, natives and assets | Mojang's servers, each file checked against the hash Mojang publishes |
| Java | Mojang's Java runtimes, the ones the official launcher uses, unless the player sets a Java path |
| Fabric, Quilt, Legacy Fabric and Ornithe | The profile each project publishes on its own meta server, and the libraries that profile names |
| Forge and NeoForge | Their own installers. Forge before 1.6 comes from Forge's maven. |
| Libraries old FML downloads at startup (Forge 1.3.2 to 1.5.2) | Prism Launcher's mirror, `files.prismlauncher.org/fmllibs`, because the address FML used is gone. Each file is checked against a SHA-1 kept in [`loaders.rs`](../crates/waymark-core/src/loaders.rs). |
| Mods, resource packs and shaders | Only what the player adds: from Modrinth, from CurseForge, or a file they drop on the window |
| A speedrunning preset's mods | The jars on speedrun.com's legal-mods list, read from [mcsr-meta](https://github.com/tildejustin/mcsr-meta), fetched only from the hosts that list uses, and checked against its SHA-512 hashes |

Mods are kept once in a read-only store and placed into each instance from there. A jar stays byte for byte the file that was downloaded or dropped in.

## What goes on the command line

To see the exact command for one instance, open its settings, choose Launching, then Show the launch command. From a terminal, `waymark command <instance>` prints the same.

The version's own file decides the main class, the classpath and the game's arguments: Mojang's file, or a loader's profile laid over it. Waymark Launcher adds these:

- `-Xms` and `-Xmx`, from the player's memory setting. `-Xms` is the same as `-Xmx` up to 1 GB.
- The G1 garbage collector flags the official launcher uses by default.
- `-Dtoolscreen.minecraftVersion=<version>`, which Toolscreen reads to tell which version is running. The game doesn't read it, and it isn't one of the debug properties in A.8.18.
- For version files without their own JVM arguments (before 1.13), `-Djava.library.path` and `-cp`, as the official launcher does for them.
- `--width` and `--height`, or `--fullscreen`, if the player set a window size.
- `--quickPlaySingleplayer` or `--quickPlayMultiplayer` (1.20 and newer), or `--server` and `--port` (older versions), when the player opens a world or server from Waymark Launcher.
- Before 1.6: `-Duser.home`, and `APPDATA` on Windows, pointing at the instance's folder. These versions open a log in the real `.minecraft` before anything can move it.
- Forge before 1.6: `-Dminecraft.applet.TargetDirectory`, `-Dfml.ignoreInvalidMinecraftCertificates=true` and `-Dfml.ignorePatchDiscrepancies=true`. See below.
- Whatever the player typed into the instance's Java arguments, environment variables, wrapper command, and commands to run before launching and after the game closes.

The game's process also gets the environment variables `INST_ID`, `INST_NAME`, `INST_DIR`, `INST_MC_DIR`, `INST_MC_VER` and `INST_JAVA`. They're the names Prism Launcher and MultiMC use, and tools like Toolscreen read them. The game doesn't.

Mojang's version files from 1.13 on fill `-Dminecraft.launcher.brand` with `${launcher_name}`. Waymark Launcher sets it to `Waymark Launcher`, so the game and mods can tell which launcher started them.

Waymark Launcher doesn't add a Java agent (A.8.19) or attach a debugger, and doesn't patch, replace or add a library the version file doesn't name (A.8.17). It doesn't touch LWJGL 3 or GLFW (A.8.4). The exceptions follow.

## Exceptions

### Minecraft 1.0 to 1.5.2

Mojang's releases from 1.0 to 1.5.2 launch from Mojang's own version files.

Waymark Launcher can also run them through [LegacyFix](https://github.com/betacraftuk/legacyfix), BetaCraft's patches that bring back skins and fix bugs these versions have on today's computers. LegacyFix is a Java agent and uses Legacy Fabric's LWJGL. It's off unless the player turns on **Run with LegacyFix** in the instance's settings, and that setting says speedrun.com's rules don't allow it. Instances made before 0.4.3 keep LegacyFix until the player turns it off.

Builds Mojang doesn't list (Classic, Indev, Infdev, the alphas, the betas and the pre-releases) always run through LegacyFix. Runs on those aren't allowed anyway (A.1.2).

### Legacy Fabric

Legacy Fabric's profiles replace Mojang's LWJGL 2 with Legacy Fabric's build. From 0.4.3, Waymark Launcher keeps Mojang's (A.8.3.a). Apple silicon Macs are the exception: Mojang never built LWJGL 2 for them, and the game can't start without Legacy Fabric's.

### Ornithe

Ornithe instances use Ornithe's gen 2 profiles, which the Ornithe mods on Modrinth are built for. Each profile names Ornithe's own copy of the vanilla version file, such as `1.2.5-vanilla`, and Waymark Launcher uses Mojang's file in its place, so the game's libraries come from Mojang.

The speedrunning community uses Ornithe gen 1 for 1.0 to 1.2.5. Waymark Launcher doesn't offer gen 1, so speedrun mods built for it won't load in these instances.

### Forge 1.7.10 and older

- Forge for 1.6 and 1.7, except 1.7.10 builds from 10.13.3 on, gets Forge's own `legacyjavafixer` mod, because it crashes on Java 8 without it.
- Forge before 1.6 is a jar mod. Its files go into the instance's copy of the game jar, the way it was made to be installed.
- Forge 1.3.2 to 1.5.2 gets the libraries FML would otherwise try to download at startup, from the mirror in the table above. That download stops the game when it fails.
- Forge before 1.6 also gets the three flags listed above. The first stops it from looking for its folder in the player's real `.minecraft`, and the other two stop it from refusing a game jar that has been changed, which a jar mod always does.

None of this happens outside Forge instances.

## Launches the player didn't start

On Windows, after the player updates an instance's mods or modpack, Waymark Launcher starts the game on a hidden desktop to see whether it still loads, and undoes the update if it doesn't. This uses the same launch as Play, with the signed-in account. It stops when the game reaches the title screen, when it fails, when 7 minutes pass, or when the player presses Play. It can be turned off in Settings, under **Check that the game still starts after an update** ([`start_check.rs`](../crates/waymark-core/src/start_check.rs)).

Waymark Launcher doesn't start the game at any other time without the player pressing Play.

## What runs beside the game

- Waymark Launcher reads the game's output for its log viewer and crash help, and watches the process to know when the game closes. It doesn't inject anything into the game or draw anything over it.
- Each instance has its own game folder, and two instances never share one (A.9.1). Resource packs, shader packs and screenshots are only shared between instances when the player chooses.
- Shared game settings are off by default. When they're on, Waymark Launcher copies option values between instances as each starts and closes. It only copies values the game itself wrote, so nothing goes past what the game's own settings allow (A.1.4).
- Discord rich presence runs in the launcher, not the game, and is off unless the player turns it on.

## Practicing in Waymark Launcher

- Runs you want to submit need one of the launchers in rule A.1.1.
- For practice that matches a real run, use the exact jars from [mc.sr/mods](https://mc.sr/mods) (A.8.5), since a mod from Modrinth or CurseForge can be a different build. Drop them onto Waymark Launcher's window with the instance open and they go in unchanged, or use a Speedrunning preset, which installs them for you.
- On 1.0 to 1.5.2, leave **Run with LegacyFix** off.
