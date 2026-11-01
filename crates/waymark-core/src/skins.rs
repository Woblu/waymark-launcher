//! The skin library. Skins the player adds are kept as PNGs in the data folder.
//! Minecraft's default skins aren't stored or shipped at all: they're loaded
//! from Mojang's own texture server, the same place the game gets them.

use std::fs;
use std::path::Path;

use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Core;
use crate::error::{Error, Result};

const LIBRARY_FILE: &str = "library.json";
const PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const TEXTURES: &str = "https://textures.minecraft.net/texture";
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Far larger than any real skin, so a wrong file is caught before it's read.
const MAX_SKIN_BYTES: u64 = 1 << 20;
const SKIN_SIZES: [(u32, u32); 2] = [(64, 64), (64, 32)];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinVariant {
    #[default]
    Classic,
    Slim,
}

impl SkinVariant {
    fn name(self) -> &'static str {
        match self {
            SkinVariant::Classic => "classic",
            SkinVariant::Slim => "slim",
        }
    }
}

const DEFAULTS: [(&str, SkinVariant, &str); 18] = [
    (
        "Steve",
        SkinVariant::Classic,
        "31f477eb1a7beee631c2ca64d06f8f68fa93a3386d04452ab27f43acdf1b60cb",
    ),
    (
        "Steve",
        SkinVariant::Slim,
        "d5c4ee5ce20aed9e33e866c66caa37178606234b3721084bf01d13320fb2eb3f",
    ),
    (
        "Alex",
        SkinVariant::Slim,
        "46acd06e8483b176e8ea39fc12fe105eb3a2a4970f5100057e9d84d4b60bdfa7",
    ),
    (
        "Alex",
        SkinVariant::Classic,
        "1abc803022d8300ab7578b189294cce39622d9a404cdc00d3feacfdf45be6981",
    ),
    (
        "Ari",
        SkinVariant::Slim,
        "6ac6ca262d67bcfb3dbc924ba8215a18195497c780058a5749de674217721892",
    ),
    (
        "Ari",
        SkinVariant::Classic,
        "4c05ab9e07b3505dc3ec11370c3bdce5570ad2fb2b562e9b9dd9cf271f81aa44",
    ),
    (
        "Efe",
        SkinVariant::Slim,
        "fece7017b1bb13926d1158864b283b8b930271f80a90482f174cca6a17e88236",
    ),
    (
        "Efe",
        SkinVariant::Classic,
        "daf3d88ccb38f11f74814e92053d92f7728ddb1a7955652a60e30cb27ae6659f",
    ),
    (
        "Kai",
        SkinVariant::Slim,
        "226c617fde5b1ba569aa08bd2cb6fd84c93337532a872b3eb7bf66bdd5b395f8",
    ),
    (
        "Kai",
        SkinVariant::Classic,
        "e5cdc3243b2153ab28a159861be643a4fc1e3c17d291cdd3e57a7f370ad676f3",
    ),
    (
        "Makena",
        SkinVariant::Slim,
        "7cb3ba52ddd5cc82c0b050c3f920f87da36add80165846f479079663805433db",
    ),
    (
        "Makena",
        SkinVariant::Classic,
        "dc0fcfaf2aa040a83dc0de4e56058d1bbb2ea40157501f3e7d15dc245e493095",
    ),
    (
        "Noor",
        SkinVariant::Slim,
        "6c160fbd16adbc4bff2409e70180d911002aebcfa811eb6ec3d1040761aea6dd",
    ),
    (
        "Noor",
        SkinVariant::Classic,
        "90e75cd429ba6331cd210b9bd19399527ee3bab467b5a9f61cb8a27b177f6789",
    ),
    (
        "Sunny",
        SkinVariant::Slim,
        "b66bc80f002b10371e2fa23de6f230dd5e2f3affc2e15786f65bc9be4c6eb71a",
    ),
    (
        "Sunny",
        SkinVariant::Classic,
        "a3bd16079f764cd541e072e888fe43885e711f98658323db0f9a6045da91ee7a",
    ),
    (
        "Zuri",
        SkinVariant::Slim,
        "eee522611005acf256dbd152e992c60c0bb7978cb0f3127807700e478ad97664",
    ),
    (
        "Zuri",
        SkinVariant::Classic,
        "f5dddb41dcafef616e959c2817808e0be741c89ffbfed39134a13e75b811863d",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSkin {
    pub id: String,
    pub name: String,
    pub variant: SkinVariant,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultSkin {
    pub name: String,
    pub variant: SkinVariant,
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinLibrary {
    pub saved: Vec<SavedSkin>,
    pub defaults: Vec<DefaultSkin>,
}

/// What to wear: something from the library, or one of the game's own.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Wear {
    Saved { id: String },
    Default { url: String, variant: SkinVariant },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cape {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WornSkin {
    pub url: String,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub state: String,
}

/// What the player is wearing right now, from Mojang.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinProfile {
    pub name: String,
    #[serde(default)]
    pub skins: Vec<WornSkin>,
    #[serde(default)]
    pub capes: Vec<Cape>,
}

/// Reads a PNG's size from its header, without decoding the image.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let number = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    Some((number(16)?, number(20)?))
}

fn check_skin(bytes: &[u8]) -> Result<()> {
    let size = png_size(bytes).ok_or_else(|| Error::Other("that isn't a PNG image".into()))?;
    if !SKIN_SIZES.contains(&size) {
        return Err(Error::Other(format!(
            "skins are 64 by 64 pixels, and this one is {} by {}",
            size.0, size.1
        )));
    }
    Ok(())
}

impl Core {
    fn saved_skins(&self) -> Result<Vec<SavedSkin>> {
        match fs::read(self.paths.skins().join(LIBRARY_FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn save_skins(&self, skins: &[SavedSkin]) -> Result<()> {
        fs::create_dir_all(self.paths.skins())?;
        let path = self.paths.skins().join(LIBRARY_FILE);
        let part = path.with_extension("json.part");
        fs::write(&part, serde_json::to_vec_pretty(skins)?)?;
        fs::rename(part, path)?;
        Ok(())
    }

    pub fn skin_library(&self) -> Result<SkinLibrary> {
        Ok(SkinLibrary {
            saved: self.saved_skins()?,
            defaults: DEFAULTS
                .iter()
                .map(|(name, variant, key)| DefaultSkin {
                    name: (*name).to_string(),
                    variant: *variant,
                    url: format!("{TEXTURES}/{key}"),
                })
                .collect(),
        })
    }

    /// Copies a PNG into the library. Adding the same image twice just finds the first.
    pub fn add_skin(&self, path: &Path) -> Result<SavedSkin> {
        if fs::metadata(path)?.len() > MAX_SKIN_BYTES {
            return Err(Error::Other("that file is too big to be a skin".into()));
        }
        let bytes = fs::read(path)?;
        check_skin(&bytes)?;
        let id = blake3::hash(&bytes).to_hex().to_string();
        let mut skins = self.saved_skins()?;
        if let Some(found) = skins.iter().find(|skin| skin.id == id) {
            return Ok(found.clone());
        }
        fs::create_dir_all(self.paths.skins())?;
        fs::write(self.paths.skins().join(format!("{id}.png")), bytes)?;
        let name = path.file_stem().map_or_else(
            || "Skin".to_string(),
            |stem| stem.to_string_lossy().replace(['-', '_'], " "),
        );
        let skin = SavedSkin {
            id,
            name,
            variant: SkinVariant::Classic,
        };
        skins.insert(0, skin.clone());
        self.save_skins(&skins)?;
        Ok(skin)
    }

    pub fn update_skin(
        &self,
        id: &str,
        name: &str,
        variant: SkinVariant,
    ) -> Result<Vec<SavedSkin>> {
        let mut skins = self.saved_skins()?;
        let skin = skins
            .iter_mut()
            .find(|skin| skin.id == id)
            .ok_or_else(|| Error::NotFound("that skin".into()))?;
        skin.name = name.trim().to_string();
        skin.variant = variant;
        self.save_skins(&skins)?;
        Ok(skins)
    }

    pub fn remove_skin(&self, id: &str) -> Result<Vec<SavedSkin>> {
        let mut skins = self.saved_skins()?;
        let before = skins.len();
        skins.retain(|skin| skin.id != id);
        if skins.len() == before {
            return Err(Error::NotFound("that skin".into()));
        }
        // Ids are hashes this library made, so this can only ever name a file in it.
        let _ = fs::remove_file(self.paths.skins().join(format!("{id}.png")));
        self.save_skins(&skins)?;
        Ok(skins)
    }

    pub async fn skin_profile(&self) -> Result<SkinProfile> {
        let session = self.session().await?;
        Ok(self
            .net
            .client()
            .get(PROFILE)
            .bearer_auth(&session.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Changes the skin on the player's Minecraft account.
    pub async fn wear_skin(&self, wear: Wear) -> Result<SkinProfile> {
        let session = self.session().await?;
        let client = self.net.client();
        let request = client
            .post(format!("{PROFILE}/skins"))
            .bearer_auth(&session.access_token);
        let response = match wear {
            Wear::Default { url, variant } => {
                if !url.starts_with(&format!("{TEXTURES}/")) {
                    return Err(Error::Other("that isn't one of Minecraft's skins".into()));
                }
                request
                    .json(&json!({ "variant": variant.name(), "url": url }))
                    .send()
                    .await?
            }
            Wear::Saved { id } => {
                let skin = self
                    .saved_skins()?
                    .into_iter()
                    .find(|skin| skin.id == id)
                    .ok_or_else(|| Error::NotFound("that skin".into()))?;
                let bytes = fs::read(self.paths.skins().join(format!("{}.png", skin.id)))?;
                let file = Part::bytes(bytes)
                    .file_name("skin.png")
                    .mime_str("image/png")?;
                request
                    .multipart(
                        Form::new()
                            .text("variant", skin.variant.name())
                            .part("file", file),
                    )
                    .send()
                    .await?
            }
        };
        Ok(response.error_for_status()?.json().await?)
    }

    /// Puts on one of the player's capes, or takes it off.
    pub async fn wear_cape(&self, cape: Option<&str>) -> Result<SkinProfile> {
        let session = self.session().await?;
        let client = self.net.client();
        let active = format!("{PROFILE}/capes/active");
        let response = match cape {
            Some(id) => {
                client
                    .put(active)
                    .bearer_auth(&session.access_token)
                    .json(&json!({ "capeId": id }))
                    .send()
                    .await?
            }
            None => {
                client
                    .delete(active)
                    .bearer_auth(&session.access_token)
                    .send()
                    .await?
            }
        };
        Ok(response.error_for_status()?.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    fn core() -> (tempfile::TempDir, Core) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().join("data"), crate::event::silent()).unwrap();
        (dir, core)
    }

    #[test]
    fn only_real_skin_sizes_are_accepted() {
        assert!(check_skin(&png(64, 64)).is_ok());
        assert!(check_skin(&png(64, 32)).is_ok());
        assert!(check_skin(&png(128, 128)).is_err());
        assert!(check_skin(b"not a png at all, not even close").is_err());
    }

    #[test]
    fn the_library_keeps_added_skins_and_finds_duplicates() {
        let (dir, core) = core();
        let path = dir.path().join("my-cool_skin.png");
        fs::write(&path, png(64, 64)).unwrap();

        let skin = core.add_skin(&path).unwrap();
        assert_eq!(skin.name, "my cool skin");
        assert_eq!(core.add_skin(&path).unwrap().id, skin.id);
        assert_eq!(core.skin_library().unwrap().saved.len(), 1);
        assert_eq!(
            core.local_image(&format!("skin/{}.png", skin.id)).unwrap(),
            png(64, 64)
        );

        core.update_skin(&skin.id, "Cape Rider", SkinVariant::Slim)
            .unwrap();
        let saved = core.skin_library().unwrap().saved;
        assert_eq!(saved[0].name, "Cape Rider");
        assert_eq!(saved[0].variant, SkinVariant::Slim);

        assert!(core.remove_skin(&skin.id).unwrap().is_empty());
        assert_eq!(core.local_image(&format!("skin/{}.png", skin.id)), None);
    }

    #[test]
    fn every_default_skin_comes_from_mojang() {
        let (_dir, core) = core();
        let defaults = core.skin_library().unwrap().defaults;
        assert_eq!(defaults.len(), 18);
        assert!(defaults.iter().all(|skin| {
            skin.url
                .starts_with("https://textures.minecraft.net/texture/")
        }));
        for name in [
            "Steve", "Alex", "Ari", "Efe", "Kai", "Makena", "Noor", "Sunny", "Zuri",
        ] {
            assert_eq!(
                defaults.iter().filter(|skin| skin.name == name).count(),
                2,
                "{name}"
            );
        }
    }

    #[test]
    fn mojang_profiles_are_read_as_they_arrive() {
        let profile: SkinProfile = serde_json::from_value(json!({
            "id": "abc",
            "name": "Wayfarer",
            "skins": [{ "id": "1", "state": "ACTIVE", "url": "https://textures.minecraft.net/texture/x", "variant": "SLIM" }],
            "capes": [{ "id": "c", "state": "INACTIVE", "url": "https://textures.minecraft.net/texture/y", "alias": "Migrator" }],
        }))
        .unwrap();
        assert_eq!(profile.name, "Wayfarer");
        assert_eq!(profile.skins[0].variant, "SLIM");
        assert_eq!(profile.capes[0].alias.as_deref(), Some("Migrator"));
    }
}
