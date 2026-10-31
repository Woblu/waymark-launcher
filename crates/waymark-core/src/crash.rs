//! Reads what the game left behind and says what went wrong in plain language.
//! These rules run locally, so diagnosis works offline and shares nothing.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Fix {
    UseJava { major: u32 },
    RaiseMemory,
    UpdateDrivers,
    InstallMod { id: String },
    RemoveDuplicate { id: String },
    RunBisect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub title: String,
    pub detail: String,
    pub suspects: Vec<String>,
    pub fix: Fix,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub findings: Vec<Finding>,
    pub report_file: Option<String>,
}

/// Looks at the newest crash report and the latest log for this game directory.
pub fn inspect(game_dir: &Path, since: std::time::SystemTime) -> CrashReport {
    let report = newest_report(&game_dir.join("crash-reports"), since);
    let mut text = report.as_ref().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
    if let Ok(log) = fs::read_to_string(game_dir.join("logs/latest.log")) {
        text.push('\n');
        text.push_str(&log);
    }
    CrashReport {
        findings: analyze(&text),
        report_file: report.map(|p| p.to_string_lossy().into_owned()),
    }
}

fn newest_report(dir: &Path, since: std::time::SystemTime) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(modified, _)| *modified >= since)
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

pub fn analyze(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    wrong_java(text, &mut findings);
    out_of_memory(text, &mut findings);
    missing_mods(text, &mut findings);
    duplicate_mods(text, &mut findings);
    mixin_failure(text, &mut findings);
    graphics_driver(text, &mut findings);
    suspected_mods(text, &mut findings);
    if findings.is_empty() {
        findings.push(Finding {
            title: "The cause isn't clear from the logs".into(),
            detail: "Bisect can test your mods in groups and find the one responsible.".into(),
            suspects: Vec::new(),
            fix: Fix::RunBisect,
        });
    }
    findings
}

fn wrong_java(text: &str, out: &mut Vec<Finding>) {
    const MARKER: &str = "class file version ";
    if !text.contains("UnsupportedClassVersionError") {
        return;
    }
    let needed = text
        .split(MARKER)
        .skip(1)
        .filter_map(|rest| rest.split('.').next()?.trim().parse::<u32>().ok())
        .max();
    if let Some(class_version) = needed {
        let major = class_version.saturating_sub(44);
        out.push(Finding {
            title: format!("This needs Java {major}"),
            detail: format!("A mod was built for Java {major}, which is newer than the Java this instance uses."),
            suspects: Vec::new(),
            fix: Fix::UseJava { major },
        });
    }
}

fn out_of_memory(text: &str, out: &mut Vec<Finding>) {
    if text.contains("java.lang.OutOfMemoryError") {
        out.push(Finding {
            title: "Minecraft ran out of memory".into(),
            detail: "This instance needs more RAM than it has been given.".into(),
            suspects: Vec::new(),
            fix: Fix::RaiseMemory,
        });
    }
}

fn missing_mods(text: &str, out: &mut Vec<Finding>) {
    let mut missing = Vec::new();
    for line in text.lines() {
        // Fabric names the dependency last, in either of these forms:
        //   "- Mod 'Iris' (iris) 1.8 requires any 0.6.x version of sodium, which is missing!"
        //   "- Mod 'Iris' (iris) 1.8 requires version 2 of 'Sodium' (sodium), which is missing!"
        if let Some(id) = missing_fabric_dependency(line) {
            missing.push(id);
        }
        // NeoForge and Forge: "Mod ID: 'sodium', Requested by: 'iris', Expected range: ..."
        if line.contains("Requested by:")
            && let Some(id) = between(line, "Mod ID: '", "'") {
                missing.push(id.to_string());
            }
    }
    missing.sort();
    missing.dedup();
    for id in missing {
        out.push(Finding {
            title: format!("Missing mod: {id}"),
            detail: format!("Another mod needs {id}, but it isn't installed."),
            suspects: vec![id.clone()],
            fix: Fix::InstallMod { id },
        });
    }
}

fn missing_fabric_dependency(line: &str) -> Option<String> {
    let end = line.find(", which is missing")?;
    let needed = line[..end].rsplit(" of ").next()?.trim();
    let id = match (needed.rfind('('), needed.rfind(')')) {
        (Some(open), Some(close)) if open < close => &needed[open + 1..close],
        _ => needed.trim_matches('\''),
    };
    (!id.is_empty()).then(|| id.to_string())
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = text[from..].find(end)? + from;
    Some(&text[from..to])
}

fn duplicate_mods(text: &str, out: &mut Vec<Finding>) {
    if !(text.contains("Found duplicate mods") || text.contains("Duplicate mods found") || text.contains("Multiple versions of")) {
        return;
    }
    let id = text.lines().find_map(|l| between(l, "Mod ID: '", "'").map(String::from)).unwrap_or_default();
    out.push(Finding {
        title: "The same mod is installed twice".into(),
        detail: "Two files provide the same mod, so the game refuses to start.".into(),
        suspects: if id.is_empty() { Vec::new() } else { vec![id.clone()] },
        fix: Fix::RemoveDuplicate { id },
    });
}

fn mixin_failure(text: &str, out: &mut Vec<Finding>) {
    let mut suspects: Vec<String> = text
        .lines()
        .filter_map(|l| between(l, "Mixin apply for mod ", " failed").map(String::from))
        .collect();
    suspects.sort();
    suspects.dedup();
    if suspects.is_empty() && !text.contains("MixinApplyError") && !text.contains("InvalidInjectionException") {
        return;
    }
    let who = if suspects.is_empty() { "A mod".to_string() } else { suspects.join(", ") };
    out.push(Finding {
        title: format!("{who} failed to load"),
        detail: "It changes game code that another mod or this game version doesn't match, which usually means it needs an update.".into(),
        suspects,
        fix: Fix::RunBisect,
    });
}

fn graphics_driver(text: &str, out: &mut Vec<Finding>) {
    const DRIVERS: [(&str, &str); 6] = [
        ("ig9icd64", "Intel"),
        ("ig75icd64", "Intel"),
        ("ig7icd64", "Intel"),
        ("nvoglv64", "NVIDIA"),
        ("atio6axx", "AMD"),
        ("amdvlk64", "AMD"),
    ];
    let crashed_natively = text.contains("EXCEPTION_ACCESS_VIOLATION") || text.contains("SIGSEGV");
    let vendor = DRIVERS.iter().find(|(file, _)| crashed_natively && text.contains(file)).map(|(_, v)| *v);
    if let Some(vendor) = vendor {
        out.push(Finding {
            title: format!("The {vendor} graphics driver crashed"),
            detail: "The crash happened inside the driver, so updating it is the most likely fix.".into(),
            suspects: Vec::new(),
            fix: Fix::UpdateDrivers,
        });
    } else if text.contains("Pixel format not accelerated") || text.contains("GLFW error 65542") {
        out.push(Finding {
            title: "Your graphics driver doesn't support OpenGL here".into(),
            detail: "Minecraft couldn't create a window, which usually means a driver is missing or out of date.".into(),
            suspects: Vec::new(),
            fix: Fix::UpdateDrivers,
        });
    }
}

/// Fabric and Forge both name likely mods in the crash report.
fn suspected_mods(text: &str, out: &mut Vec<Finding>) {
    let mut suspects: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("Suspected Mod:").or_else(|| line.strip_prefix("Suspected Mods:"))?.trim();
            let name = rest.split([',', '(']).next()?.trim();
            (!name.is_empty() && name != "NONE").then(|| name.to_string())
        })
        .collect();
    suspects.sort();
    suspects.dedup();
    if suspects.is_empty() || !out.is_empty() {
        return;
    }
    out.push(Finding {
        title: format!("{} looks responsible", suspects.join(", ")),
        detail: "The crash report points at this code. Bisect can confirm it.".into(),
        suspects,
        fix: Fix::RunBisect,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_a_java_version_mismatch() {
        let log = "java.lang.UnsupportedClassVersionError: x has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 61.0";
        let found = analyze(log);
        assert_eq!(found[0].fix, Fix::UseJava { major: 21 });
        assert_eq!(found[0].title, "This needs Java 21");
    }

    #[test]
    fn explains_running_out_of_memory() {
        assert_eq!(analyze("java.lang.OutOfMemoryError: Java heap space")[0].fix, Fix::RaiseMemory);
    }

    #[test]
    fn names_a_missing_fabric_dependency() {
        let log = " - Mod 'Iris' (iris) 1.8.0 requires any version of 'Sodium' (sodium), which is missing!";
        assert_eq!(analyze(log)[0].fix, Fix::InstallMod { id: "sodium".into() });
    }

    #[test]
    fn names_the_dependency_not_the_mod_that_needs_it() {
        // Copied from a real Fabric Loader 0.20.1 crash.
        let log = "[main/ERROR] \t - Mod 'Iris' (iris) 1.8.8+mc1.21.4 requires any 0.6.x version of sodium, which is missing!";
        let found = analyze(log);
        assert_eq!(found[0].fix, Fix::InstallMod { id: "sodium".into() });
        assert_eq!(found[0].title, "Missing mod: sodium");
    }

    #[test]
    fn names_a_missing_neoforge_dependency() {
        let log = "Mod ID: 'geckolib', Requested by: 'mowziesmobs', Expected range: '[4.0,)', Actual version: '[MISSING]'";
        assert_eq!(analyze(log)[0].fix, Fix::InstallMod { id: "geckolib".into() });
    }

    #[test]
    fn identifies_the_mod_behind_a_mixin_failure() {
        let log = "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError\nMixin apply for mod create failed create.mixins.json:FooMixin";
        let found = analyze(log);
        assert_eq!(found[0].suspects, ["create"]);
        assert_eq!(found[0].fix, Fix::RunBisect);
    }

    #[test]
    fn blames_the_graphics_driver_for_a_native_crash_inside_it() {
        let log = "#  EXCEPTION_ACCESS_VIOLATION (0xc0000005)\n# C  [ig9icd64.dll+0x1234]";
        let found = analyze(log);
        assert_eq!(found[0].fix, Fix::UpdateDrivers);
        assert!(found[0].title.contains("Intel"));
    }

    #[test]
    fn a_driver_name_alone_is_not_a_driver_crash() {
        let found = analyze("Loaded driver ig9icd64.dll\nEverything is fine");
        assert_eq!(found[0].fix, Fix::RunBisect);
    }

    #[test]
    fn uses_the_suspected_mod_named_in_a_crash_report() {
        let log = "Suspected Mod: \n\tSuspected Mods: Create (create), Version: 6.0.1";
        assert_eq!(analyze(log)[0].suspects, ["Create"]);
    }

    #[test]
    fn offers_bisect_when_nothing_matches() {
        let found = analyze("something unrecognised happened");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].fix, Fix::RunBisect);
    }
}
