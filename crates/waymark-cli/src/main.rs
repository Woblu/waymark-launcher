//! A small command line over the core, for scripting and for proving each part
//! works without the UI.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use waymark_core::bisect::{Bisect, Step, Verdict};
use waymark_core::content::{ContentKind, ContentSource, SearchQuery};
use waymark_core::event::Event;
use waymark_core::instance::NewInstance;
use waymark_core::lock::LoaderKind;
use waymark_core::paths::Paths;
use waymark_core::waymarks::WaymarkKind;
use waymark_core::{Core, Error, Result};

const USAGE: &str = "waymark <command>

  versions                                  list recent Minecraft releases
  loaders <kind> <minecraft>                list loader versions
  list                                      list instances
  create <name> <minecraft> [loader]        create an instance
  search <text>                             search Modrinth for mods
  cf-search <text>                          search CurseForge for mods
  add <instance> <project>                  add a Modrinth mod and its dependencies
  cf-add <instance> <project-id>            add a CurseForge mod and its dependencies
  pack <project>                            install a Modrinth modpack
  mods <instance>                           list an instance's mods
  updates <instance>                        check for mod updates
  trail <instance>                          show an instance's waymarks
  mark <instance> <label>                   set a waymark
  return <instance> <waymark>               return to a waymark
  import [name]                             list or copy instances from other launchers
  sign-in                                   sign in with Microsoft
  play <instance>                           launch with the active account
  play-dev <instance>                       debug builds only: launch without an account
  pack-update <instance>                    update an instance's modpack, keeping your changes
  export <instance> <folder>                build a server folder
  bisect <instance>                         find which mod causes a crash
  reclaim                                   free space no waymark needs
";

fn loader_kind(text: &str) -> Result<LoaderKind> {
    match text {
        "vanilla" => Ok(LoaderKind::Vanilla),
        "fabric" => Ok(LoaderKind::Fabric),
        "quilt" => Ok(LoaderKind::Quilt),
        "forge" => Ok(LoaderKind::Forge),
        "neoforge" => Ok(LoaderKind::NeoForge),
        other => Err(Error::Other(format!("unknown loader: {other}"))),
    }
}

fn argument<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str> {
    args.get(index).map(String::as_str).ok_or_else(|| Error::Other(format!("missing <{name}>\n\n{USAGE}")))
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// Prints events as they arrive and returns the exit once a game finishes.
fn print_events() -> (waymark_core::event::Emitter, mpsc::UnboundedReceiver<Option<i32>>) {
    let (exited, receiver) = mpsc::unbounded_channel();
    let emit = Arc::new(move |event: Event| match event {
        Event::TaskStarted { title, .. } => println!("> {title}"),
        Event::TaskProgress { done, total, detail, .. } if total > 1 => {
            eprint!("\r  {detail}: {:.0} / {:.0} MB   ", megabytes(done), megabytes(total));
        }
        Event::TaskFailed { message, .. } => eprintln!("\nfailed: {message}"),
        Event::TaskFinished { .. } => eprintln!(),
        Event::GameStarted { pid, .. } => println!("game running as pid {pid}"),
        Event::GameLog { line, .. } => println!("{line}"),
        Event::GameExited { code, played_secs, crash, .. } => {
            println!("game exited with {code:?} after {played_secs}s");
            for finding in crash.into_iter().flat_map(|c| c.findings) {
                println!("  {}: {}", finding.title, finding.detail);
            }
            let _ = exited.send(code);
        }
        _ => {}
    });
    (emit, receiver)
}

async fn wait_for_exit(exits: &mut mpsc::UnboundedReceiver<Option<i32>>) {
    exits.recv().await;
}

async fn run(core: Core, mut exits: mpsc::UnboundedReceiver<Option<i32>>, args: Vec<String>) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("help");
    match command {
        "versions" => {
            let manifest = core.version_manifest().await?;
            println!("latest release {}, snapshot {}", manifest.latest.release, manifest.latest.snapshot);
            for version in manifest.versions.iter().filter(|v| v.kind == "release").take(12) {
                println!("  {}", version.id);
            }
        }
        "loaders" => {
            let kind = loader_kind(argument(&args, 1, "kind")?)?;
            let versions = core.loader_versions(kind, argument(&args, 2, "minecraft")?).await?;
            for version in versions.iter().take(10) {
                println!("  {}{}", version.version, if version.stable { "" } else { " (unstable)" });
            }
        }
        "list" => {
            for instance in core.instances()? {
                let mods = core.lockfile(&instance.id)?.mods.len();
                println!("{:<28} {} {:?}, {mods} mods", instance.id, instance.minecraft, instance.loader.kind);
            }
        }
        "create" => {
            let loader = args.get(3).map_or(Ok(LoaderKind::Vanilla), |l| loader_kind(l))?;
            let instance = core
                .create_instance(NewInstance {
                    name: argument(&args, 1, "name")?.to_string(),
                    minecraft: argument(&args, 2, "minecraft")?.to_string(),
                    loader,
                    loader_version: None,
                })
                .await?;
            println!("created {} (launches {})", instance.id, instance.version_id);
        }
        "search" => {
            let query = SearchQuery {
                text: argument(&args, 1, "text")?.to_string(),
                kind: ContentKind::Mod,
                minecraft: None,
                loader: None,
                offset: 0,
                limit: 8,
            };
            let page = core.search_content(ContentSource::Modrinth, &query).await?;
            println!("{} results", page.total);
            for hit in page.hits {
                println!("  {:<26} {:>12} downloads  {}", hit.slug, hit.downloads, hit.title);
            }
        }
        "cf-search" => {
            let query = SearchQuery {
                text: argument(&args, 1, "text")?.to_string(),
                kind: ContentKind::Mod,
                minecraft: None,
                loader: None,
                offset: 0,
                limit: 5,
            };
            let page = core.search_content(ContentSource::CurseForge, &query).await?;
            println!("{} results", page.total);
            for hit in page.hits {
                println!("  {:<12} {:>12} downloads  {}", hit.id, hit.downloads, hit.title);
            }
        }
        "cf-add" => {
            let mark = core
                .install_content(argument(&args, 1, "instance")?, ContentSource::CurseForge, argument(&args, 2, "project")?, ContentKind::Mod)
                .await?;
            println!("{}", mark.map_or("already installed".to_string(), |m| m.label));
        }
        "add" => {
            let mark = core
                .install_content(argument(&args, 1, "instance")?, ContentSource::Modrinth, argument(&args, 2, "project")?, ContentKind::Mod)
                .await?;
            println!("{}", mark.map_or("already installed".to_string(), |m| m.label));
        }
        "pack" => {
            let result = core.install_pack(ContentSource::Modrinth, argument(&args, 1, "project")?, None).await?;
            let mods = core.lockfile(&result.instance.id)?.mods.len();
            println!("installed {} with {mods} mods", result.instance.id);
        }
        "mods" => {
            for entry in core.lockfile(argument(&args, 1, "instance")?)?.mods {
                let state = if entry.enabled { " " } else { "x" };
                println!("[{state}] {:<36} {:?}", entry.name, entry.origin);
            }
        }
        "updates" => {
            for update in core.check_updates(argument(&args, 1, "instance")?).await? {
                let held = if update.held { "  (held: too new)" } else { "" };
                println!("{}: {:?} -> {}{held}", update.name, update.current, update.latest);
            }
        }
        "trail" => {
            for waymark in core.trail(argument(&args, 1, "instance")?).list()? {
                println!(
                    "{}  {:<40} {} files, {:.1} MB, +{} -{} ~{}",
                    waymark.id,
                    waymark.label,
                    waymark.files,
                    megabytes(waymark.bytes),
                    waymark.added.len(),
                    waymark.removed.len(),
                    waymark.changed.len(),
                );
            }
        }
        "mark" => {
            let mark = core.trail(argument(&args, 1, "instance")?).mark(argument(&args, 2, "label")?, WaymarkKind::Manual)?;
            println!("{}", mark.map_or("nothing has changed".to_string(), |m| m.id));
        }
        "return" => {
            let mark = core.trail(argument(&args, 1, "instance")?).restore(argument(&args, 2, "waymark")?)?;
            println!("{}", mark.label);
        }
        "import" => {
            let found = core.find_external_instances();
            match args.get(1) {
                Some(name) => {
                    let external = found
                        .into_iter()
                        .find(|f| f.name == *name)
                        .ok_or_else(|| Error::NotFound(format!("an instance called {name}")))?;
                    let instance = core.import_external(external).await?;
                    println!("imported {}, launches {}", instance.id, instance.version_id);
                }
                None => {
                    for instance in found {
                        println!("{:?}: {} ({} {:?})", instance.launcher, instance.name, instance.minecraft, instance.loader.kind);
                    }
                }
            }
        }
        "sign-in" => {
            let login = core.begin_sign_in().await?;
            println!("Open {} and enter {}", login.verification_uri, login.user_code);
            let account = core.finish_sign_in(&login).await?;
            println!("signed in as {}", account.name);
        }
        "play" => {
            core.launch(argument(&args, 1, "instance")?).await?;
            wait_for_exit(&mut exits).await;
        }
        #[cfg(debug_assertions)]
        "play-dev" => {
            core.launch_with(argument(&args, 1, "instance")?, waymark_core::auth::Session::development()).await?;
            wait_for_exit(&mut exits).await;
        }
        "pack-update" => {
            let id = argument(&args, 1, "instance")?;
            match core.check_pack_update(id).await? {
                None => println!("already up to date"),
                Some(update) => {
                    println!("{} {} -> {}", update.name, update.current, update.latest);
                    let report = core.update_pack(id).await?;
                    println!("+{} ~{} -{}, kept {} of your changes", report.added, report.updated, report.removed, report.kept.len());
                }
            }
        }
        "export" => {
            let export = core.export_server(argument(&args, 1, "instance")?, argument(&args, 2, "folder")?.as_ref()).await?;
            println!("{} ({} mods, {} left out)", export.folder.display(), export.mods, export.skipped.len());
        }
        "bisect" => bisect(&core, argument(&args, 1, "instance")?)?,
        "reclaim" => println!("freed {:.1} MB", megabytes(core.reclaim_space()?)),
        _ => print!("{USAGE}"),
    }
    Ok(())
}

/// Walks through a bisect by hand: apply each step, test it, report back.
fn bisect(core: &Core, id: &str) -> Result<()> {
    let lock = core.lockfile(id)?;
    let requires: BTreeMap<String, Vec<String>> = lock.mods.iter().map(|m| (m.hash.clone(), m.requires.clone())).collect();
    let names: BTreeMap<&str, &str> = lock.mods.iter().map(|m| (m.hash.as_str(), m.name.as_str())).collect();
    let mut search = Bisect::new(lock.mods.iter().map(|m| m.hash.clone()).collect(), requires);
    let mut step = search.step();
    loop {
        match &step {
            Step::Test { enabled, disabled, round, estimate } => {
                for hash in enabled {
                    core.set_mod_enabled(id, hash, true)?;
                }
                for hash in disabled {
                    core.set_mod_enabled(id, hash, false)?;
                }
                println!("round {round} of about {estimate}: {} mods on. Does it crash? [y/n]", enabled.len());
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                let verdict = if answer.trim().starts_with('y') { Verdict::Crashes } else { Verdict::Works };
                step = search.report(verdict);
            }
            Step::Found { culprits } => {
                let found: Vec<&str> = culprits.iter().filter_map(|c| names.get(c.as_str()).copied()).collect();
                println!("found: {}", found.join(" + "));
                return Ok(());
            }
            Step::NotAMod => {
                println!("it still crashes with every mod off, so a mod isn't the cause");
                return Ok(());
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let (emit, exits) = print_events();
    let root = std::env::var("WAYMARK_HOME").map(Into::into).or_else(|_| Paths::default_root());
    let core = root.and_then(|root| Core::new(root, emit));
    let outcome = match core {
        Ok(core) => run(core, exits, std::env::args().skip(1).collect()).await,
        Err(error) => Err(error),
    };
    if let Err(error) = outcome {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
