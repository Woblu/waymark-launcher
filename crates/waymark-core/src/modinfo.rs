//! Reads a mod's own name, version and icon from inside its jar, so mods that
//! arrive without store metadata, like hand-placed or imported ones, still show
//! up properly instead of as a filename.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

const MAX_ICON_BYTES: u64 = 2 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    /// Where the mod's icon sits inside the jar.
    pub icon: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub sources: Option<String>,
    pub issues: Option<String>,
}

// Fields that mods fill in loosely are read as plain values, so one odd entry
// doesn't cost us the name and icon.
#[derive(Deserialize)]
struct FabricMod {
    id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    icon: Option<Value>,
    description: Option<String>,
    #[serde(default)]
    authors: Value,
    #[serde(default)]
    contact: Value,
    #[serde(default)]
    license: Value,
}

#[derive(Deserialize)]
struct QuiltMod {
    quilt_loader: QuiltLoader,
}

#[derive(Deserialize)]
struct QuiltLoader {
    id: Option<String>,
    version: Option<String>,
    metadata: Option<QuiltMetadata>,
}

#[derive(Deserialize)]
struct QuiltMetadata {
    name: Option<String>,
    icon: Option<Value>,
    description: Option<String>,
    #[serde(default)]
    contributors: Value,
    #[serde(default)]
    contact: Value,
    #[serde(default)]
    license: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForgeMods {
    #[serde(default)]
    mods: Vec<ForgeMod>,
    logo_file: Option<String>,
    license: Option<String>,
    #[serde(rename = "issueTrackerURL")]
    issue_tracker: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForgeMod {
    mod_id: Option<String>,
    display_name: Option<String>,
    version: Option<String>,
    logo_file: Option<String>,
    description: Option<String>,
    authors: Option<String>,
    #[serde(rename = "displayURL")]
    display_url: Option<String>,
}

fn entry(archive: &mut zip::ZipArchive<File>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(
        String::from_utf8_lossy(&bytes)
            .trim_start_matches('\u{feff}')
            .to_string(),
    )
}

/// Fabric's reader accepts raw line breaks inside strings and real mods ship
/// them, Sodium among them, but they aren't valid JSON. This swaps them for
/// spaces so one long description doesn't cost us the whole file.
fn relax_strings(text: &str) -> String {
    let mut in_string = false;
    let mut escaped = false;
    text.chars()
        .map(|c| {
            let control = in_string && c.is_control();
            if escaped {
                escaped = false;
            } else if in_string && c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = !in_string;
            }
            if control { ' ' } else { c }
        })
        .collect()
}

/// Forge jars can defer their version to the manifest with a placeholder.
fn real_version(version: Option<String>) -> Option<String> {
    version.filter(|v| !v.is_empty() && !v.contains("${"))
}

/// Fabric and Quilt icons are a path, or a map of sizes to paths.
fn icon_path(icon: Option<Value>) -> Option<String> {
    match icon? {
        Value::String(path) => Some(path),
        Value::Object(sizes) => sizes
            .into_iter()
            .filter_map(|(size, path)| {
                Some((size.parse::<u32>().ok()?, path.as_str()?.to_string()))
            })
            .max_by_key(|(size, _)| *size)
            .map(|(_, path)| path),
        _ => None,
    }
}

/// Authors are names, objects with a name, or for Quilt a map of name to role.
fn people(value: Value) -> Vec<String> {
    match value {
        Value::Array(list) => list
            .into_iter()
            .filter_map(|person| match person {
                Value::String(name) => Some(name),
                Value::Object(details) => details.get("name")?.as_str().map(String::from),
                _ => None,
            })
            .collect(),
        Value::Object(map) => map.into_iter().map(|(name, _)| name).collect(),
        _ => Vec::new(),
    }
}

/// Licenses are a name, a list, or for Quilt an object with a name.
fn license_name(value: Value) -> Option<String> {
    let names: Vec<String> = match value {
        Value::String(name) => vec![name],
        Value::Array(list) => list.into_iter().filter_map(license_name).collect(),
        Value::Object(details) => vec![
            details
                .get("name")
                .or(details.get("id"))?
                .as_str()?
                .to_string(),
        ],
        _ => Vec::new(),
    };
    (!names.is_empty()).then(|| names.join(", "))
}

fn contact(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|url| !url.is_empty())
        .map(String::from)
}

fn parse_fabric(text: &str) -> Option<ModInfo> {
    let info: FabricMod = serde_json::from_str(&relax_strings(text)).ok()?;
    Some(ModInfo {
        id: info.id,
        name: info.name,
        version: real_version(info.version),
        icon: icon_path(info.icon),
        description: info.description,
        authors: people(info.authors),
        license: license_name(info.license),
        homepage: contact(&info.contact, "homepage"),
        sources: contact(&info.contact, "sources"),
        issues: contact(&info.contact, "issues"),
    })
}

fn parse_quilt(text: &str) -> Option<ModInfo> {
    let info: QuiltMod = serde_json::from_str(&relax_strings(text)).ok()?;
    let loader = info.quilt_loader;
    let Some(metadata) = loader.metadata else {
        return Some(ModInfo {
            id: loader.id,
            version: real_version(loader.version),
            ..ModInfo::default()
        });
    };
    Some(ModInfo {
        id: loader.id,
        name: metadata.name,
        version: real_version(loader.version),
        icon: icon_path(metadata.icon),
        description: metadata.description,
        authors: people(metadata.contributors),
        license: license_name(metadata.license),
        homepage: contact(&metadata.contact, "homepage"),
        sources: contact(&metadata.contact, "sources"),
        issues: contact(&metadata.contact, "issues"),
    })
}

fn parse_forge(text: &str) -> Option<ModInfo> {
    let info: ForgeMods = toml::from_str(text).ok()?;
    let shared_logo = info.logo_file;
    let first = info.mods.into_iter().next()?;
    Some(ModInfo {
        id: first.mod_id,
        name: first.display_name,
        version: real_version(first.version),
        icon: first
            .logo_file
            .or(shared_logo)
            .filter(|logo| !logo.is_empty()),
        description: first.description,
        authors: first
            .authors
            .into_iter()
            .filter(|names| !names.is_empty())
            .collect(),
        license: info.license,
        homepage: first.display_url,
        sources: None,
        issues: info.issue_tracker,
    })
}

/// Returns what the jar says about itself, if it says anything we can read.
pub fn read(path: &Path) -> Option<ModInfo> {
    let mut archive = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
    if let Some(info) = entry(&mut archive, "fabric.mod.json").and_then(|t| parse_fabric(&t)) {
        return Some(info);
    }
    if let Some(info) = entry(&mut archive, "quilt.mod.json").and_then(|t| parse_quilt(&t)) {
        return Some(info);
    }
    ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"]
        .iter()
        .find_map(|name| entry(&mut archive, name).and_then(|t| parse_forge(&t)))
}

/// Pulls an icon out of a jar. Only PNGs of a sensible size are accepted,
/// because the bytes end up displayed in the app.
pub fn read_icon(jar: &Path, inside: &str) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(File::open(jar).ok()?).ok()?;
    let file = archive.by_name(inside.trim_start_matches('/')).ok()?;
    if file.size() > MAX_ICON_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_ICON_BYTES).read_to_end(&mut bytes).ok()?;
    bytes.starts_with(&PNG_SIGNATURE).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(dir: &Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = dir.join("mod.jar");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn reads_a_fabric_mod() {
        let dir = tempfile::tempdir().unwrap();
        let body = br#"{"id":"sodium","name":"Sodium","version":"0.6.13","icon":"assets/sodium/icon.png"}"#;
        let path = jar(dir.path(), &[("fabric.mod.json", body)]);
        let info = read(&path).unwrap();
        assert_eq!(info.name.unwrap(), "Sodium");
        assert_eq!(info.version.unwrap(), "0.6.13");
        assert_eq!(info.icon.unwrap(), "assets/sodium/icon.png");
        assert_eq!(info.id.unwrap(), "sodium");
    }

    #[test]
    fn reads_a_fabric_mod_with_a_line_break_in_a_string() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"{\"id\":\"sodium\",\"name\":\"Sodium\",\"description\":\"Faster.\nUnofficial \\\"backport\\\".\",\"icon\":\"icon.png\"}";
        let path = jar(dir.path(), &[("fabric.mod.json", body)]);
        let info = read(&path).unwrap();
        assert_eq!(info.name.unwrap(), "Sodium");
        assert_eq!(info.icon.unwrap(), "icon.png");
    }

    #[test]
    fn reads_a_fabric_mod_that_starts_with_a_byte_order_mark() {
        let dir = tempfile::tempdir().unwrap();
        let path = jar(
            dir.path(),
            &[(
                "fabric.mod.json",
                "\u{feff}{\"name\":\"Marked\"}".as_bytes(),
            )],
        );
        assert_eq!(read(&path).unwrap().name.unwrap(), "Marked");
    }

    #[test]
    fn relaxing_only_touches_control_characters_inside_strings() {
        assert_eq!(relax_strings("{\n\"a\":\"b\nc\"\n}"), "{\n\"a\":\"b c\"\n}");
        assert_eq!(relax_strings(r#"{"a":"b\"c\n"}"#), r#"{"a":"b\"c\n"}"#);
    }

    #[test]
    fn picks_the_largest_of_several_fabric_icons() {
        let dir = tempfile::tempdir().unwrap();
        let body = br#"{"name":"X","icon":{"16":"small.png","128":"large.png","32":"mid.png"}}"#;
        let path = jar(dir.path(), &[("fabric.mod.json", body)]);
        assert_eq!(read(&path).unwrap().icon.unwrap(), "large.png");
    }

    #[test]
    fn reads_a_quilt_mod() {
        let dir = tempfile::tempdir().unwrap();
        let body = br#"{"quilt_loader":{"version":"1.2","metadata":{"name":"Quilted","icon":"icon.png"}}}"#;
        let path = jar(dir.path(), &[("quilt.mod.json", body)]);
        assert_eq!(
            read(&path).unwrap(),
            ModInfo {
                name: Some("Quilted".into()),
                version: Some("1.2".into()),
                icon: Some("icon.png".into()),
                ..ModInfo::default()
            }
        );
    }

    #[test]
    fn reads_what_a_fabric_mod_says_about_itself() {
        let dir = tempfile::tempdir().unwrap();
        let body = br#"{"name":"X","description":"Does things.","authors":["Ann",{"name":"Bo","contact":{}}],
            "contact":{"homepage":"https://x.dev","sources":"https://github.com/x/x","email":"a@b.c"},"license":["MIT","Apache-2.0"]}"#;
        let info = read(&jar(dir.path(), &[("fabric.mod.json", body)])).unwrap();
        assert_eq!(info.description.unwrap(), "Does things.");
        assert_eq!(info.authors, ["Ann", "Bo"]);
        assert_eq!(info.license.unwrap(), "MIT, Apache-2.0");
        assert_eq!(info.homepage.unwrap(), "https://x.dev");
        assert_eq!(info.sources.unwrap(), "https://github.com/x/x");
        assert_eq!(info.issues, None);
    }

    #[test]
    fn reads_what_a_quilt_mod_says_about_itself() {
        let dir = tempfile::tempdir().unwrap();
        let body =
            br#"{"quilt_loader":{"id":"q","metadata":{"name":"Q","contributors":{"Ann":"Owner"},
            "license":{"name":"MIT","id":"mit"},"contact":{"issues":"https://x.dev/issues"}}}}"#;
        let info = read(&jar(dir.path(), &[("quilt.mod.json", body)])).unwrap();
        assert_eq!(info.authors, ["Ann"]);
        assert_eq!(info.license.unwrap(), "MIT");
        assert_eq!(info.issues.unwrap(), "https://x.dev/issues");
    }

    #[test]
    fn reads_what_a_forge_mod_says_about_itself() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"license=\"LGPL\"\nissueTrackerURL=\"https://x.dev/issues\"\n[[mods]]\nmodId=\"f\"\ndisplayName=\"F\"\ndescription='''Line one.'''\nauthors=\"Ann, Bo\"\ndisplayURL=\"https://x.dev\"\n";
        let info = read(&jar(dir.path(), &[("META-INF/mods.toml", body)])).unwrap();
        assert_eq!(info.description.unwrap(), "Line one.");
        assert_eq!(info.authors, ["Ann, Bo"]);
        assert_eq!(info.license.unwrap(), "LGPL");
        assert_eq!(info.homepage.unwrap(), "https://x.dev");
        assert_eq!(info.issues.unwrap(), "https://x.dev/issues");
    }

    #[test]
    fn odd_authors_do_not_cost_us_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let body = br#"{"name":"Kept","authors":42,"contact":"none","license":null}"#;
        let info = read(&jar(dir.path(), &[("fabric.mod.json", body)])).unwrap();
        assert_eq!(info.name.unwrap(), "Kept");
        assert!(info.authors.is_empty());
        assert_eq!(info.license, None);
    }

    #[test]
    fn reads_a_neoforge_mod() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"create\"\ndisplayName=\"Create\"\nversion=\"6.0.1\"\nlogoFile=\"logo.png\"\n";
        let path = jar(dir.path(), &[("META-INF/neoforge.mods.toml", body)]);
        assert_eq!(
            read(&path).unwrap(),
            ModInfo {
                id: Some("create".into()),
                name: Some("Create".into()),
                version: Some("6.0.1".into()),
                icon: Some("logo.png".into()),
                ..ModInfo::default()
            }
        );
    }

    #[test]
    fn ignores_a_placeholder_version() {
        let dir = tempfile::tempdir().unwrap();
        let body =
            b"[[mods]]\nmodId=\"x\"\ndisplayName=\"Thing\"\nversion=\"${file.jarVersion}\"\n";
        let path = jar(dir.path(), &[("META-INF/mods.toml", body)]);
        assert_eq!(read(&path).unwrap().version, None);
    }

    #[test]
    fn extracts_a_png_icon() {
        let dir = tempfile::tempdir().unwrap();
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(b"pixels");
        let path = jar(dir.path(), &[("assets/icon.png", &png)]);
        assert_eq!(read_icon(&path, "/assets/icon.png").unwrap(), png);
    }

    #[test]
    fn refuses_an_icon_that_is_not_a_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = jar(
            dir.path(),
            &[("assets/icon.png", b"<script>alert(1)</script>")],
        );
        assert!(read_icon(&path, "assets/icon.png").is_none());
    }

    #[test]
    fn gives_up_quietly_on_anything_else() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_zip = dir.path().join("broken.jar");
        std::fs::write(&not_a_zip, b"not a jar").unwrap();
        assert!(read(&not_a_zip).is_none());
        assert!(read(&jar(dir.path(), &[("readme.txt", b"hi")])).is_none());
    }
}
