//! The skin library. Skins the player adds are kept as PNGs in the data folder.
//! Minecraft's default skins aren't stored or shipped at all: they're loaded
//! from Mojang's own texture server, the same place the game gets them.

use std::fs;
use std::path::{Path, PathBuf};

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
/// Capes are 64 by 32 at the game's own size. The game also draws sharper ones
/// at whole multiples of that, and the cape maker goes up to 2048 by 1024.
const CAPE_SCALES: [u32; 6] = [1, 2, 4, 8, 16, 32];
/// Far larger than a 2048 by 1024 PNG gets.
const MAX_CAPE_BASE64: usize = 48 << 20;
/// The folder in the shared assets where the game keeps every skin and cape it
/// downloads. It loads them from there afterwards instead of downloading again.
const GAME_TEXTURES: &str = "skins";
/// The capes the player made, listed in the skin library's folder.
const MADE_CAPES_FILE: &str = "capes.json";
/// The pictures of the capes the player made, by id, in the skin library's folder.
pub(crate) const MADE_CAPES: &str = "capes";
const MAX_CAPE_NAME: usize = 64;

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
    /// Mojang's address for this skin, once it has been worn from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
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
    /// The cape the player made that the game on this computer shows in this
    /// one's place, by id, if there is one.
    #[serde(default, skip_deserializing)]
    pub showing: Option<String>,
}

/// A cape the player made from a picture, kept so it can be worn again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MadeCape {
    /// The BLAKE3 hash of its PNG, which is also the PNG's name.
    pub id: String,
    pub name: String,
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

/// How long Mojang's answer is used again before asking anew. Every page that
/// shows a skin asks, and Mojang answers 429 when that happens too often.
const PROFILE_FRESH: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Mojang's last answer for one account, and when it came.
pub(crate) struct KnownProfile {
    uuid: String,
    at: std::time::Instant,
    profile: SkinProfile,
}

/// Mojang lists textures at http addresses, which the window's security policy
/// won't load. The same files are served over https.
pub(crate) fn secure_texture(url: &str) -> String {
    match url.strip_prefix("http://textures.minecraft.net/") {
        Some(rest) => format!("https://textures.minecraft.net/{rest}"),
        None => url.to_string(),
    }
}

fn secured(mut profile: SkinProfile) -> SkinProfile {
    for skin in &mut profile.skins {
        skin.url = secure_texture(&skin.url);
    }
    for cape in &mut profile.capes {
        cape.url = secure_texture(&cape.url);
    }
    profile
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

/// A cape the window drew, sent as base64 PNG. Only a PNG at a cape's size is
/// taken, so the window can't use this to put anything else on disk.
fn cape_png(png_base64: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    if png_base64.len() > MAX_CAPE_BASE64 {
        return Err(Error::Other("that cape is too big".into()));
    }
    let not_a_cape = || Error::Other("that isn't a cape texture".into());
    let bytes = STANDARD.decode(png_base64).map_err(|_| not_a_cape())?;
    let fits = png_size(&bytes).is_some_and(|size| {
        CAPE_SCALES
            .iter()
            .any(|scale| size == (64 * scale, 32 * scale))
    });
    if !fits {
        return Err(not_a_cape());
    }
    Ok(bytes)
}

/// Saves a cape the window drew where the player chose, under a .png name.
pub fn save_cape(target: &Path, png_base64: &str) -> Result<()> {
    if !target
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return Err(Error::Other("a cape has to be saved as a .png file".into()));
    }
    crate::atomic::write(target, &cape_png(png_base64)?)?;
    Ok(())
}

/// The hash the game names a texture by: the last part of its address.
fn texture_hash(url: &str) -> Option<&str> {
    let hash = url.rsplit('/').next()?;
    let plain = !hash.is_empty() && hash.len() <= 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    plain.then_some(hash)
}

/// The files the game reads a texture from before it downloads it, and never
/// checks. Up to 1.20.1 the file is named after the texture's hash. From 1.20.2
/// on, it's named after a SHA-1 of that hash, taken over its UTF-16 code units
/// (`SkinManager.TextureCache` in the game).
fn cached_texture(textures: &Path, hash: &str) -> [PathBuf; 2] {
    use sha1::{Digest, Sha1};

    let units: Vec<u8> = hash.encode_utf16().flat_map(u16::to_le_bytes).collect();
    [hash.to_string(), hex::encode(Sha1::digest(&units))].map(|name| {
        let folder = if name.len() > 2 { &name[..2] } else { "xx" };
        textures.join(folder).join(&name)
    })
}

fn remove_if_there(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// The most a picture from a link may weigh. Big GIFs run to tens of megabytes.
const MAX_LINKED_PICTURE: usize = 40 << 20;
/// Far more than the head of a page, where preview tags are.
const MAX_LINKED_PAGE: usize = 2 << 20;

/// What a link the player pasted led to.
enum Linked {
    Picture(Vec<u8>),
    /// A web page, and where it ended up after redirects.
    Page(String, reqwest::Url),
}

async fn fetch_linked(client: &reqwest::Client, link: &str) -> Result<Linked> {
    let url = reqwest::Url::parse(link.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or_else(|| Error::Other("that isn't a web link".into()))?;
    let mut response = client.get(url).send().await?.error_for_status()?;
    let base = response.url().clone();
    let kind = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let page = kind.starts_with("text/html") || kind.starts_with("application/xhtml");
    let limit = if page {
        MAX_LINKED_PAGE
    } else {
        MAX_LINKED_PICTURE
    };
    let too_big = || Error::Other("that picture is too big".into());
    if !page
        && response
            .content_length()
            .is_some_and(|size| size > limit as u64)
    {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > limit {
            if page {
                break;
            }
            return Err(too_big());
        }
    }
    Ok(if page {
        Linked::Page(String::from_utf8_lossy(&bytes).into_owned(), base)
    } else {
        Linked::Picture(bytes)
    })
}

/// One attribute's value from an HTML tag, matched without regard to case.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let start = from + found;
        from = start + name.len();
        if !lower[..start].ends_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let Some(rest) = lower[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        // Lowercasing ASCII keeps every byte where it was, so this lines up with `tag`.
        let value = tag[tag.len() - rest.len()..].trim_start();
        let (body, end) = match value.chars().next()? {
            quote @ ('"' | '\'') => {
                let body = &value[1..];
                (body, body.find(quote)?)
            }
            _ => (
                value,
                value
                    .find(|c: char| c.is_ascii_whitespace() || c == '>')
                    .unwrap_or(value.len()),
            ),
        };
        return Some(body[..end].replace("&amp;", "&"));
    }
    None
}

/// The picture a page names for link previews, which on Tenor and Giphy is the GIF.
fn preview_picture(html: &str) -> Option<String> {
    const TAGS: [&str; 4] = [
        "og:image:secure_url",
        "og:image",
        "og:image:url",
        "twitter:image",
    ];
    let lower = html.to_ascii_lowercase();
    let mut best: Option<(usize, String)> = None;
    let mut at = 0;
    while let Some(found) = lower[at..].find("<meta") {
        let start = at + found;
        let end = lower[start..]
            .find('>')
            .map_or(lower.len(), |end| start + end);
        at = end;
        let tag = &html[start..end];
        let key = attribute(tag, "property").or_else(|| attribute(tag, "name"));
        let rank = key.and_then(|key| TAGS.iter().position(|each| key.eq_ignore_ascii_case(each)));
        if let (Some(rank), Some(content)) = (rank, attribute(tag, "content"))
            && best.as_ref().is_none_or(|(kept, _)| rank < *kept)
        {
            best = Some((rank, content));
        }
    }
    best.map(|(_, content)| content)
}

/// Downloads a picture from a link the player pasted into the cape maker. A
/// link to a page, like a GIF's page on Tenor or Giphy, gives the picture the
/// page names for previews.
pub async fn picture_from_link(client: &reqwest::Client, link: &str) -> Result<Vec<u8>> {
    let no_picture = || Error::Other("there's no picture at that link".into());
    match fetch_linked(client, link).await? {
        Linked::Picture(bytes) => Ok(bytes),
        Linked::Page(html, base) => {
            let preview = preview_picture(&html).ok_or_else(no_picture)?;
            let url = base.join(&preview).map_err(|_| no_picture())?;
            match fetch_linked(client, url.as_str()).await? {
                Linked::Picture(bytes) => Ok(bytes),
                Linked::Page(..) => Err(no_picture()),
            }
        }
    }
}

/// The real cape a made one takes the place of: the one the player has on, or
/// else their first.
fn base_cape(profile: &SkinProfile) -> Option<&Cape> {
    profile
        .capes
        .iter()
        .find(|cape| cape.state == "ACTIVE")
        .or_else(|| profile.capes.first())
}

fn cape_name(name: &str) -> String {
    let name: String = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CAPE_NAME)
        .collect();
    if name.is_empty() {
        "Cape".to_string()
    } else {
        name
    }
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
        let path = self.paths.skins().join(LIBRARY_FILE);
        crate::atomic::write(&path, &serde_json::to_vec_pretty(skins)?)?;
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
            texture: None,
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

    /// Sends a request to Minecraft's services as the active account. A session kept
    /// from an earlier run may have been revoked since, for example by a password
    /// change, so a 401 forgets it and signs in once more.
    async fn signed<F>(&self, build: F) -> Result<SkinProfile>
    where
        F: Fn(&reqwest::Client) -> Result<reqwest::RequestBuilder>,
    {
        let client = self.net.client();
        let mut session = self.web_session().await?;
        let mut response = build(client)?
            .bearer_auth(&session.access_token)
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.forget_session(&session.uuid);
            session = self.web_session().await?;
            response = build(client)?
                .bearer_auth(&session.access_token)
                .send()
                .await?;
        }
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(Error::Auth(
                "Minecraft asked Waymark Launcher to slow down. Try again in a minute.".into(),
            ));
        }
        let profile = secured(response.error_for_status()?.json().await?);
        self.note_skin(&profile);
        *self.profile.lock().expect("profile poisoned") = Some(KnownProfile {
            uuid: session.uuid.clone(),
            at: std::time::Instant::now(),
            profile: profile.clone(),
        });
        Ok(self.with_pictures(profile))
    }

    /// Mojang's last answer for this account, if it's no older than `within`.
    fn known_profile(&self, uuid: &str, within: std::time::Duration) -> Option<SkinProfile> {
        self.profile
            .lock()
            .expect("profile poisoned")
            .as_ref()
            .filter(|known| known.uuid == uuid && known.at.elapsed() <= within)
            .map(|known| known.profile.clone())
    }

    /// Keeps the account's skin current, so it can still be shown when Mojang can't be reached.
    fn note_skin(&self, profile: &SkinProfile) {
        let worn = profile.skins.iter().find(|skin| skin.state == "ACTIVE");
        let url = worn.map(|skin| skin.url.clone());
        let slim = worn.is_some_and(|skin| skin.variant.eq_ignore_ascii_case("slim"));
        let Ok(mut accounts) = self.accounts() else {
            return;
        };
        let Some(account) = accounts.iter_mut().find(|account| account.active) else {
            return;
        };
        if account.skin_url != url || account.skin_slim != slim {
            account.skin_url = url;
            account.skin_slim = slim;
            let _ = self.save_accounts(&accounts);
        }
    }

    /// What the player wears. A recent answer is used again, and if Mojang can't be
    /// asked, an older one is better than nothing.
    pub async fn skin_profile(&self) -> Result<SkinProfile> {
        let account = self.active_account()?;
        if let Some(known) = self.known_profile(&account.uuid, PROFILE_FRESH) {
            return Ok(self.with_pictures(known));
        }
        match self.signed(|client| Ok(client.get(PROFILE))).await {
            Ok(profile) => Ok(profile),
            Err(error) => self
                .known_profile(&account.uuid, std::time::Duration::MAX)
                .map(|known| self.with_pictures(known))
                .ok_or(error),
        }
    }

    /// Changes the skin on the player's Minecraft account.
    pub async fn wear_skin(&self, wear: Wear) -> Result<SkinProfile> {
        let url = format!("{PROFILE}/skins");
        match wear {
            Wear::Default { url: skin, variant } => {
                if !skin.starts_with(&format!("{TEXTURES}/")) {
                    return Err(Error::Other("that isn't one of Minecraft's skins".into()));
                }
                let body = json!({ "variant": variant.name(), "url": skin });
                self.signed(|client| Ok(client.post(&url).json(&body)))
                    .await
            }
            Wear::Saved { id } => {
                let skin = self
                    .saved_skins()?
                    .into_iter()
                    .find(|skin| skin.id == id)
                    .ok_or_else(|| Error::NotFound("that skin".into()))?;
                let bytes = fs::read(self.paths.skins().join(format!("{}.png", skin.id)))?;
                let profile = self
                    .signed(|client| {
                        let file = Part::bytes(bytes.clone())
                            .file_name("skin.png")
                            .mime_str("image/png")?;
                        let form = Form::new()
                            .text("variant", skin.variant.name())
                            .part("file", file);
                        Ok(client.post(&url).multipart(form))
                    })
                    .await?;
                let worn = profile.skins.iter().find(|each| each.state == "ACTIVE");
                if let Some(worn) = worn {
                    self.note_texture(&skin.id, &worn.url)?;
                }
                Ok(profile)
            }
        }
    }

    /// Remembers the address Mojang gave a saved skin, so the library can tell
    /// when it's the one worn. The same picture always gets the same address.
    fn note_texture(&self, id: &str, texture: &str) -> Result<()> {
        let mut skins = self.saved_skins()?;
        let Some(skin) = skins.iter_mut().find(|skin| skin.id == id) else {
            return Ok(());
        };
        if skin.texture.as_deref() != Some(texture) {
            skin.texture = Some(texture.to_string());
            self.save_skins(&skins)?;
        }
        Ok(())
    }

    /// Tells Mojang which of the player's capes to show, or none.
    async fn set_active_cape(&self, cape: Option<&str>) -> Result<SkinProfile> {
        let active = format!("{PROFILE}/capes/active");
        match cape {
            Some(id) => {
                let body = json!({ "capeId": id });
                self.signed(|client| Ok(client.put(&active).json(&body)))
                    .await
            }
            None => self.signed(|client| Ok(client.delete(&active))).await,
        }
    }

    /// Puts on one of the player's capes as Mojang made it, or takes it off. A
    /// made cape worn in its place comes off, so the game shows the real one.
    pub async fn wear_cape(&self, cape: Option<&str>) -> Result<SkinProfile> {
        let current = self.skin_profile().await?;
        let worn = current.capes.iter().find(|each| each.state == "ACTIVE");
        let profile = if worn.map(|each| each.id.as_str()) == cape {
            current
        } else {
            self.set_active_cape(cape).await?
        };
        self.take_off_made(None)?;
        // Also undoes a picture put there by hand, which can't be told from Mojang's.
        let chosen = profile
            .capes
            .iter()
            .find(|each| Some(each.id.as_str()) == cape);
        if let Some(hash) = chosen.and_then(|each| texture_hash(&each.url)) {
            self.take_off(hash)?;
        }
        Ok(self.with_pictures(profile))
    }

    /// The capes the player made, newest first.
    pub fn made_capes(&self) -> Result<Vec<MadeCape>> {
        match fs::read(self.paths.skins().join(MADE_CAPES_FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn save_made_capes(&self, capes: &[MadeCape]) -> Result<()> {
        let path = self.paths.skins().join(MADE_CAPES_FILE);
        crate::atomic::write(&path, &serde_json::to_vec_pretty(capes)?)?;
        Ok(())
    }

    fn made_cape_file(&self, id: &str) -> PathBuf {
        self.paths
            .skins()
            .join(MADE_CAPES)
            .join(format!("{id}.png"))
    }

    /// Keeps a cape the window drew, sent as base64 PNG. Making the same cape
    /// twice keeps one, under the newer name.
    pub fn make_cape(&self, name: &str, png_base64: &str) -> Result<MadeCape> {
        let bytes = cape_png(png_base64)?;
        let cape = MadeCape {
            id: blake3::hash(&bytes).to_hex().to_string(),
            name: cape_name(name),
        };
        crate::atomic::write(&self.made_cape_file(&cape.id), &bytes)?;
        let mut capes = self.made_capes()?;
        capes.retain(|each| each.id != cape.id);
        capes.insert(0, cape.clone());
        self.save_made_capes(&capes)?;
        Ok(cape)
    }

    /// Deletes a cape the player made. If the game shows it in place of a real
    /// cape, the real one comes back.
    pub fn remove_made_cape(&self, id: &str) -> Result<Vec<MadeCape>> {
        let mut capes = self.made_capes()?;
        if !capes.iter().any(|cape| cape.id == id) {
            return Err(Error::NotFound("that cape".into()));
        }
        self.take_off_made(Some(id))?;
        capes.retain(|cape| cape.id != id);
        // The id was found in the list, which only ever holds hashes, so this is a file in it.
        remove_if_there(&self.made_cape_file(id))?;
        self.save_made_capes(&capes)?;
        Ok(capes)
    }

    /// Wears a cape the player made. Its picture goes where the game keeps its
    /// copy of the player's real cape, and the game shows it from then on, on
    /// this computer only. Other players see the real cape, which is put on if
    /// none is. Everyone wearing the same real cape shares the game's copy, so
    /// on this computer they show the picture too.
    pub async fn wear_made_cape(&self, id: &str) -> Result<SkinProfile> {
        if !self.made_capes()?.iter().any(|cape| cape.id == id) {
            return Err(Error::NotFound("that cape".into()));
        }
        let mut profile = self.skin_profile().await?;
        let none = || {
            Error::Other(
                "a cape you make shows in place of one from Mojang, and this account has none"
                    .into(),
            )
        };
        let base = base_cape(&profile).ok_or_else(none)?;
        let (base_id, worn) = (base.id.clone(), base.state == "ACTIVE");
        let hash = texture_hash(&base.url).ok_or_else(none)?.to_string();
        if !worn {
            profile = self.set_active_cape(Some(&base_id)).await?;
        }
        self.take_off_made(None)?;
        let bytes = fs::read(self.made_cape_file(id))?;
        for copy in self.game_copies(&hash) {
            crate::atomic::write(&copy, &bytes)?;
        }
        Ok(self.with_pictures(profile))
    }

    /// The files the game shows a real cape from, once it has downloaded it.
    fn game_copies(&self, hash: &str) -> [PathBuf; 2] {
        cached_texture(&self.paths.assets().join(GAME_TEXTURES), hash)
    }

    /// The made cape the game shows in place of a real one, if its copy is one.
    fn showing(&self, hash: &str, made: &[MadeCape]) -> Option<String> {
        self.game_copies(hash).iter().find_map(|copy| {
            let id = blake3::hash(&fs::read(copy).ok()?).to_hex().to_string();
            made.iter().any(|cape| cape.id == id).then_some(id)
        })
    }

    fn with_pictures(&self, mut profile: SkinProfile) -> SkinProfile {
        let made = self.made_capes().unwrap_or_default();
        for cape in &mut profile.capes {
            cape.showing = match texture_hash(&cape.url) {
                Some(hash) if !made.is_empty() => self.showing(hash, &made),
                _ => None,
            };
        }
        profile
    }

    /// Deletes the game's copy of a cape, so it downloads the real one again.
    fn take_off(&self, hash: &str) -> Result<()> {
        for copy in self.game_copies(hash) {
            remove_if_there(&copy)?;
        }
        Ok(())
    }

    /// Deletes every copy the game has that is a made cape, or just `only`'s,
    /// so the real capes come back. Files are matched by size, then by hash.
    fn take_off_made(&self, only: Option<&str>) -> Result<()> {
        let wanted: Vec<(u64, String)> = self
            .made_capes()?
            .into_iter()
            .filter(|cape| only.is_none_or(|id| cape.id == id))
            .filter_map(|cape| {
                let size = fs::metadata(self.made_cape_file(&cape.id)).ok()?.len();
                Some((size, cape.id))
            })
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let Ok(folders) = fs::read_dir(self.paths.assets().join(GAME_TEXTURES)) else {
            return Ok(());
        };
        for copy in folders
            .flatten()
            .filter_map(|folder| fs::read_dir(folder.path()).ok())
            .flatten()
            .flatten()
        {
            let Ok(size) = copy.metadata().map(|meta| meta.len()) else {
                continue;
            };
            if !wanted.iter().any(|(each, _)| *each == size) {
                continue;
            }
            let Ok(bytes) = fs::read(copy.path()) else {
                continue;
            };
            let id = blake3::hash(&bytes).to_hex().to_string();
            if wanted.iter().any(|(_, each)| *each == id) {
                remove_if_there(&copy.path())?;
            }
        }
        Ok(())
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
    fn only_a_png_at_a_cape_size_is_saved_as_a_cape() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;

        let dir = tempfile::tempdir().unwrap();
        let encode = |bytes: &[u8]| STANDARD.encode(bytes);
        let target = dir.path().join("My Cape.PNG");

        save_cape(&target, &encode(&png(64, 32))).unwrap();
        assert_eq!(fs::read(&target).unwrap(), png(64, 32));
        save_cape(&target, &encode(&png(2048, 1024))).unwrap();
        assert_eq!(fs::read(&target).unwrap(), png(2048, 1024));

        for wrong in [
            png(64, 64),
            png(96, 48),
            png(4096, 2048),
            b"MZ not a png".to_vec(),
        ] {
            assert!(save_cape(&dir.path().join("wrong.png"), &encode(&wrong)).is_err());
        }
        assert!(save_cape(&dir.path().join("wrong.png"), "not base64!").is_err());
        assert!(save_cape(&dir.path().join("cape.exe"), &encode(&png(64, 32))).is_err());
        assert!(save_cape(&dir.path().join("cape"), &encode(&png(64, 32))).is_err());
        assert!(!dir.path().join("wrong.png").exists());
    }

    const STEVE_HASH: &str = "31f477eb1a7beee631c2ca64d06f8f68fa93a3386d04452ab27f43acdf1b60cb";

    #[test]
    fn textures_are_found_under_both_names_the_game_uses() {
        let textures = Path::new("assets").join("skins");
        let [older, newer] = cached_texture(&textures, STEVE_HASH);
        assert_eq!(older, textures.join("31").join(STEVE_HASH));
        // Checked against a skin the game cached itself, with 26.3.
        assert_eq!(
            newer,
            textures
                .join("d3")
                .join("d38251731a5a61c0406b7e78cf88ceae837fa345")
        );
        assert_eq!(
            cached_texture(&textures, "ab")[0],
            textures.join("xx").join("ab")
        );

        let url = format!("http://textures.minecraft.net/texture/{STEVE_HASH}");
        assert_eq!(texture_hash(&url), Some(STEVE_HASH));
        for wrong in [
            "",
            "http://textures.minecraft.net/texture/",
            "x/..",
            "x/not-hex",
        ] {
            assert_eq!(texture_hash(wrong), None, "{wrong}");
        }
    }

    fn encoded(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn made_capes_are_kept_named_and_deleted() {
        let (_dir, core) = core();
        let first = core
            .make_cape("  My   sunset ", &encoded(&png(2048, 1024)))
            .unwrap();
        assert_eq!(first.name, "My sunset");
        assert_eq!(
            core.local_image(&format!("cape/{}.png", first.id)).unwrap(),
            png(2048, 1024)
        );
        let second = core.make_cape("", &encoded(&png(64, 32))).unwrap();
        assert_eq!(second.name, "Cape");
        let again = core.make_cape("Dusk", &encoded(&png(2048, 1024))).unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(
            core.made_capes().unwrap(),
            vec![again.clone(), second.clone()]
        );
        assert!(core.make_cape("Skin", &encoded(&png(64, 64))).is_err());
        assert_eq!(core.made_capes().unwrap().len(), 2);

        assert_eq!(core.remove_made_cape(&first.id).unwrap(), vec![second]);
        assert_eq!(core.local_image(&format!("cape/{}.png", first.id)), None);
        assert!(core.remove_made_cape(&first.id).is_err());
        assert!(core.remove_made_cape("../capes").is_err());
    }

    #[test]
    fn a_made_cape_shows_in_place_of_a_real_one_until_it_comes_off() {
        let (_dir, core) = core();
        let sunset = core
            .make_cape("Sunset", &encoded(&png(2048, 1024)))
            .unwrap();
        let dusk = core.make_cape("Dusk", &encoded(&png(128, 64))).unwrap();
        let made = core.made_capes().unwrap();
        let [older, newer] = core.game_copies(STEVE_HASH);
        let real = png(64, 32);
        fs::create_dir_all(newer.parent().unwrap()).unwrap();
        fs::write(&newer, &real).unwrap();
        assert_eq!(core.showing(STEVE_HASH, &made), None);

        for copy in [&older, &newer] {
            crate::atomic::write(copy, &fs::read(core.made_cape_file(&sunset.id)).unwrap())
                .unwrap();
        }
        assert_eq!(core.showing(STEVE_HASH, &made), Some(sunset.id.clone()));

        // Taking off only the other cape leaves this one, and an unrelated texture stays.
        let other = core.game_copies("abcdef")[1].clone();
        crate::atomic::write(&other, &png(64, 64)).unwrap();
        core.take_off_made(Some(&dusk.id)).unwrap();
        assert_eq!(core.showing(STEVE_HASH, &made), Some(sunset.id.clone()));

        core.take_off_made(None).unwrap();
        assert!(!older.exists() && !newer.exists());
        assert!(other.exists());
        assert_eq!(core.showing(STEVE_HASH, &made), None);

        // Deleting a made cape that's worn brings the real one back too.
        crate::atomic::write(&newer, &fs::read(core.made_cape_file(&dusk.id)).unwrap()).unwrap();
        core.remove_made_cape(&dusk.id).unwrap();
        assert!(!newer.exists());
        core.take_off(STEVE_HASH).unwrap();
    }

    #[test]
    fn a_page_link_gives_its_preview_picture() {
        let tenor = r#"<html><head><meta name="viewport" content="width=device-width">
            <meta content="https://media.tenor.com/abc/tenor.gif?a=1&amp;b=2" property="og:image">
            <meta name="twitter:image" content='https://media.tenor.com/abc/small.gif'></head></html>"#;
        assert_eq!(
            preview_picture(tenor).as_deref(),
            Some("https://media.tenor.com/abc/tenor.gif?a=1&b=2")
        );
        let secure = r#"<META PROPERTY='og:image' CONTENT='http://x/a.png'><meta property="og:image:secure_url" content="https://x/a.png">"#;
        assert_eq!(preview_picture(secure).as_deref(), Some("https://x/a.png"));
        assert_eq!(
            preview_picture(r#"<meta name=twitter:image content=/pic.gif>"#).as_deref(),
            Some("/pic.gif")
        );
        assert_eq!(preview_picture("<p>no meta here</p>"), None);
        assert_eq!(
            preview_picture(r#"<meta data-property="og:image" content="x">"#),
            None
        );
    }

    #[test]
    fn saved_skins_from_before_keep_loading() {
        let skins: Vec<SavedSkin> =
            serde_json::from_str(r#"[{ "id": "a", "name": "Old", "variant": "slim" }]"#).unwrap();
        assert_eq!(skins[0].texture, None);
        assert!(!serde_json::to_string(&skins).unwrap().contains("texture"));
    }

    #[test]
    fn a_made_cape_goes_over_the_cape_worn_or_else_the_first() {
        let cape = |id: &str, state: &str| json!({ "id": id, "state": state, "url": format!("http://textures.minecraft.net/texture/{STEVE_HASH}"), "alias": id });
        let profile = |capes: Vec<serde_json::Value>| -> SkinProfile {
            serde_json::from_value(json!({ "name": "Player", "capes": capes })).unwrap()
        };
        let worn = profile(vec![cape("Pan", "INACTIVE"), cape("Migrator", "ACTIVE")]);
        assert_eq!(base_cape(&worn).unwrap().id, "Migrator");
        assert_eq!(worn.capes[1].showing, None);
        let none_on = profile(vec![cape("Pan", "INACTIVE"), cape("Migrator", "INACTIVE")]);
        assert_eq!(base_cape(&none_on).unwrap().id, "Pan");
        assert!(base_cape(&profile(vec![])).is_none());
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

    #[test]
    fn mojang_s_http_texture_addresses_become_https() {
        assert_eq!(
            secure_texture("http://textures.minecraft.net/texture/abc"),
            "https://textures.minecraft.net/texture/abc"
        );
        assert_eq!(
            secure_texture("https://textures.minecraft.net/texture/abc"),
            "https://textures.minecraft.net/texture/abc"
        );
        assert_eq!(
            secure_texture("http://example.com/texture/abc"),
            "http://example.com/texture/abc"
        );
        let profile = secured(
            serde_json::from_value(json!({
                "name": "me",
                "skins": [{ "state": "ACTIVE", "url": "http://textures.minecraft.net/texture/s" }],
                "capes": [{ "id": "c", "state": "ACTIVE", "url": "http://textures.minecraft.net/texture/c" }],
            }))
            .unwrap(),
        );
        assert!(profile.skins[0].url.starts_with("https://"));
        assert!(profile.capes[0].url.starts_with("https://"));
    }

    #[test]
    fn saved_accounts_with_an_http_skin_are_read_as_https() {
        let (_dir, core) = core();
        core.save_accounts(&[crate::auth::Account {
            uuid: "me".into(),
            name: "me".into(),
            owns_game: true,
            verified_at: time::OffsetDateTime::now_utc(),
            skin_url: Some("http://textures.minecraft.net/texture/old".into()),
            skin_slim: false,
            active: true,
        }])
        .unwrap();
        assert_eq!(
            core.accounts().unwrap()[0].skin_url.as_deref(),
            Some("https://textures.minecraft.net/texture/old")
        );
    }

    #[test]
    fn mojang_s_answer_is_shared_for_a_while_and_only_for_its_account() {
        let (_dir, core) = core();
        let profile: SkinProfile = serde_json::from_value(json!({ "name": "me" })).unwrap();
        *core.profile.lock().unwrap() = Some(KnownProfile {
            uuid: "me".into(),
            at: std::time::Instant::now(),
            profile,
        });
        assert!(core.known_profile("me", PROFILE_FRESH).is_some());
        assert!(core.known_profile("someone else", PROFILE_FRESH).is_none());
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(
            core.known_profile("me", std::time::Duration::from_millis(1))
                .is_none()
        );
        assert!(core.known_profile("me", std::time::Duration::MAX).is_some());
    }

    /// The account keeps the skin last seen, which is what shows when Mojang can't be reached.
    #[test]
    fn the_account_remembers_the_skin_last_seen() {
        let (_dir, core) = core();
        let account = |uuid: &str, active: bool| crate::auth::Account {
            uuid: uuid.into(),
            name: uuid.into(),
            owns_game: true,
            verified_at: time::OffsetDateTime::now_utc(),
            skin_url: None,
            skin_slim: false,
            active,
        };
        core.save_accounts(&[account("other", false), account("me", true)])
            .unwrap();
        let profile: SkinProfile = serde_json::from_value(json!({
            "name": "me",
            "skins": [
                { "state": "INACTIVE", "url": "https://textures.minecraft.net/texture/old", "variant": "CLASSIC" },
                { "state": "ACTIVE", "url": "https://textures.minecraft.net/texture/new", "variant": "SLIM" },
            ],
        }))
        .unwrap();
        core.note_skin(&profile);
        let accounts = core.accounts().unwrap();
        let me = accounts
            .iter()
            .find(|account| account.uuid == "me")
            .unwrap();
        assert_eq!(
            me.skin_url.as_deref(),
            Some("https://textures.minecraft.net/texture/new")
        );
        assert!(me.skin_slim);
        assert!(
            accounts[0].skin_url.is_none(),
            "another account was changed"
        );
    }
}
