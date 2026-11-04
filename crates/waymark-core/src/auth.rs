//! Microsoft sign-in and the Minecraft ownership check.
//!
//! The chain is Microsoft, Xbox Live, XSTS, Minecraft, then entitlements. An
//! account is only stored once it's proven to own the game, and every launch
//! goes through `Core::session`, which is what stops a release build from
//! reaching gameplay without that proof.

use std::fs;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::Core;
use crate::brand::{KEYCHAIN_SERVICE, MSA_CLIENT_ID};
use crate::error::{Error, Result};

const AUTHORITY: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const SCOPE: &str = "XboxLive.signin offline_access";
const XBOX_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const ENTITLEMENTS: &str = "https://api.minecraftservices.com/entitlements/license";
const PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";

/// How long a verified account may keep playing with no internet connection.
const OFFLINE_GRACE: time::Duration = time::Duration::days(30);
/// Minecraft's tokens last a day. One is let go this long before it runs out.
const TOKEN_MARGIN: Duration = Duration::from_secs(600);
const A_DAY: u64 = 86_400;

/// What the game needs to start. Fields are only visible inside this crate, so
/// a session can't be assembled by the UI or any other caller.
#[derive(Clone)]
pub struct Session {
    pub(crate) name: String,
    pub(crate) uuid: String,
    pub(crate) access_token: String,
    pub(crate) xuid: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("name", &self.name)
            .field("uuid", &self.uuid)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// For testing launches on a developer's machine. It doesn't exist in
    /// release builds.
    #[cfg(debug_assertions)]
    pub fn development() -> Session {
        Session {
            name: "Developer".into(),
            uuid: "00000000-0000-0000-0000-000000000000".into(),
            access_token: "0".into(),
            xuid: "0".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub owns_game: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub verified_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_url: Option<String>,
    /// Whether that skin has slim arms, so it can be drawn right without asking Mojang.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skin_slim: bool,
    #[serde(default)]
    pub active: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLogin {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    #[serde(skip)]
    pub(crate) device_code: String,
    #[serde(skip)]
    pub(crate) interval: u64,
}

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxResponse {
    token: String,
    display_claims: XboxClaims,
}

#[derive(Deserialize)]
struct XboxClaims {
    xui: Vec<XboxUser>,
}

#[derive(Deserialize)]
struct XboxUser {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}

#[derive(Deserialize)]
struct XboxError {
    #[serde(rename = "XErr")]
    code: Option<u64>,
}

#[derive(Deserialize)]
struct MinecraftLogin {
    access_token: String,
    #[serde(default = "a_day")]
    expires_in: u64,
}

fn a_day() -> u64 {
    A_DAY
}

/// A session Minecraft accepted, kept because Mojang turns down sign-ins that come
/// too often. Opening the skins page and wearing a skin used to be two in a row.
#[derive(Clone)]
pub(crate) struct Recent {
    session: Session,
    until: Instant,
}

/// Where a verified session waits between runs. Without it every start signs in
/// from scratch, and Mojang turns down sign-ins that come too close together.
pub(crate) trait SessionVault: Send + Sync {
    fn load(&self, uuid: &str) -> Option<Vec<u8>>;
    fn save(&self, uuid: &str, saved: &[u8]);
    fn forget(&self, uuid: &str);
}

/// The system keychain, beside the refresh token.
pub(crate) struct Keychain;

impl SessionVault for Keychain {
    fn load(&self, uuid: &str) -> Option<Vec<u8>> {
        session_keychain(uuid).ok()?.get_secret().ok()
    }

    fn save(&self, uuid: &str, saved: &[u8]) {
        if let Err(error) = session_keychain(uuid).and_then(|entry| {
            entry
                .set_secret(saved)
                .map_err(|error| Error::Auth(error.to_string()))
        }) {
            tracing::warn!(%error, "could not keep the Minecraft session for next time");
        }
    }

    fn forget(&self, uuid: &str) {
        if let Ok(entry) = session_keychain(uuid) {
            let _ = entry.delete_credential();
        }
    }
}

/// Tests keep sessions here, so they never touch the computer's real keychain.
#[derive(Default)]
pub(crate) struct MemoryVault(Mutex<std::collections::HashMap<String, Vec<u8>>>);

impl SessionVault for MemoryVault {
    fn load(&self, uuid: &str) -> Option<Vec<u8>> {
        self.0.lock().expect("vault poisoned").get(uuid).cloned()
    }

    fn save(&self, uuid: &str, saved: &[u8]) {
        self.0
            .lock()
            .expect("vault poisoned")
            .insert(uuid.to_string(), saved.to_vec());
    }

    fn forget(&self, uuid: &str) {
        self.0.lock().expect("vault poisoned").remove(uuid);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedSession {
    name: String,
    uuid: String,
    access_token: String,
    xuid: String,
    /// When to stop using it, in seconds since 1970, already short of the real expiry.
    until: i64,
}

/// A saved session for this account, with how long it has left, or nothing if it
/// belongs to someone else, has run out, or can't be read.
fn saved_session(saved: &[u8], uuid: &str, now: OffsetDateTime) -> Option<(Session, Duration)> {
    let saved: SavedSession = serde_json::from_slice(saved).ok()?;
    let left = saved.until - now.unix_timestamp();
    if saved.uuid != uuid || left <= 0 {
        return None;
    }
    Some((
        Session {
            name: saved.name,
            uuid: saved.uuid,
            access_token: saved.access_token,
            xuid: saved.xuid,
        },
        Duration::from_secs(left as u64),
    ))
}

/// Mojang saying no isn't the same as being offline, so it never leads to the
/// offline session, whose token Minecraft's services would reject.
fn refused(status: Option<reqwest::StatusCode>) -> Error {
    match status.map(|status| status.as_u16()) {
        Some(429) => {
            Error::Auth("Minecraft is getting too many sign-ins. Try again in a minute.".into())
        }
        Some(code) => Error::Auth(format!(
            "Minecraft's servers turned the sign-in down (HTTP {code}). Try again shortly."
        )),
        None => Error::Auth("Minecraft's servers turned the sign-in down.".into()),
    }
}

#[derive(Deserialize)]
struct Entitlements {
    #[serde(default)]
    items: Vec<Entitlement>,
}

#[derive(Deserialize)]
struct Entitlement {
    name: String,
}

#[derive(Deserialize)]
struct Profile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<Skin>,
}

#[derive(Deserialize)]
struct Skin {
    state: String,
    url: String,
    #[serde(default)]
    variant: String,
}

fn xbox_problem(code: Option<u64>) -> Error {
    match code {
        Some(2148916233) => Error::Auth("This Microsoft account has no Xbox profile yet. Create one at xbox.com, then sign in again.".into()),
        Some(2148916235) => Error::Auth("Xbox Live isn't available in your country.".into()),
        Some(2148916236 | 2148916237) => Error::Auth("This account needs adult verification on xbox.com.".into()),
        Some(2148916238) => Error::Auth("This is a child account. An adult needs to add it to a Microsoft family group.".into()),
        Some(other) => Error::Auth(format!("Xbox Live refused the sign-in (code {other}).")),
        None => Error::Auth("Xbox Live refused the sign-in.".into()),
    }
}

/// Decides ownership from the entitlement list. Game Pass counts, which is why
/// this reads `game_minecraft` and `product_minecraft` rather than a store receipt.
fn owns_minecraft(entitlements: &Entitlements) -> bool {
    entitlements
        .items
        .iter()
        .any(|item| item.name == "game_minecraft" || item.name == "product_minecraft")
}

fn keychain(uuid: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("refresh:{uuid}"))
        .map_err(|error| Error::Auth(format!("the system keychain isn't available: {error}")))
}

fn session_keychain(uuid: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("session:{uuid}"))
        .map_err(|error| Error::Auth(format!("the system keychain isn't available: {error}")))
}

fn may_play_offline(account: &Account) -> bool {
    account.owns_game && OffsetDateTime::now_utc() - account.verified_at < OFFLINE_GRACE
}

impl Core {
    pub fn accounts(&self) -> Result<Vec<Account>> {
        match fs::read(self.paths.accounts()) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn save_accounts(&self, accounts: &[Account]) -> Result<()> {
        fs::create_dir_all(self.paths.root())?;
        let path = self.paths.accounts();
        let part = path.with_extension("json.part");
        fs::write(&part, serde_json::to_vec_pretty(accounts)?)?;
        fs::rename(part, path)?;
        Ok(())
    }

    pub fn set_active_account(&self, uuid: &str) -> Result<Vec<Account>> {
        let mut accounts = self.accounts()?;
        if !accounts.iter().any(|a| a.uuid == uuid) {
            return Err(Error::NotFound(format!("account {uuid}")));
        }
        for account in &mut accounts {
            account.active = account.uuid == uuid;
        }
        self.save_accounts(&accounts)?;
        Ok(accounts)
    }

    pub fn remove_account(&self, uuid: &str) -> Result<Vec<Account>> {
        self.forget_session(uuid);
        let mut accounts = self.accounts()?;
        accounts.retain(|a| a.uuid != uuid);
        if !accounts.iter().any(|a| a.active)
            && let Some(first) = accounts.first_mut()
        {
            first.active = true;
        }
        self.save_accounts(&accounts)?;
        if let Ok(entry) = keychain(uuid) {
            let _ = entry.delete_credential();
        }
        Ok(accounts)
    }

    /// Starts a device sign-in. The player enters the code in their browser.
    pub async fn begin_sign_in(&self) -> Result<DeviceLogin> {
        let response = self
            .net
            .client()
            .post(format!("{AUTHORITY}/devicecode"))
            .form(&[("client_id", MSA_CLIENT_ID), ("scope", SCOPE)])
            .send()
            .await?;
        let device: DeviceResponse = response.json().await?;
        Ok(DeviceLogin {
            user_code: device.user_code,
            verification_uri: device.verification_uri,
            expires_in: device.expires_in,
            device_code: device.device_code,
            interval: device.interval,
        })
    }

    /// Waits for the player to finish in their browser, then runs the rest of
    /// the chain. Fails with `NotOwned` unless the account owns the game.
    pub async fn finish_sign_in(&self, login: &DeviceLogin) -> Result<Account> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(login.expires_in);
        let tokens = loop {
            tokio::time::sleep(Duration::from_secs(login.interval.max(1))).await;
            if tokio::time::Instant::now() > deadline {
                return Err(Error::Auth(
                    "The sign-in code expired. Please try again.".into(),
                ));
            }
            let response: TokenResponse = self
                .net
                .client()
                .post(format!("{AUTHORITY}/token"))
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", MSA_CLIENT_ID),
                    ("device_code", login.device_code.as_str()),
                ])
                .send()
                .await?
                .json()
                .await?;
            if response.access_token.is_some() {
                break response;
            }
            match response.error.as_deref() {
                Some("authorization_pending" | "slow_down") => {}
                _ => {
                    let detail = response
                        .error_description
                        .or(response.error)
                        .unwrap_or_default();
                    return Err(Error::Auth(detail));
                }
            }
        };

        let access = tokens
            .access_token
            .ok_or_else(|| Error::Auth("Microsoft sent no token".into()))?;
        let refresh = tokens
            .refresh_token
            .ok_or_else(|| Error::Auth("Microsoft sent no refresh token".into()))?;
        let verified = self.minecraft_session(&access).await?;
        self.remember(&verified);

        keychain(&verified.session.uuid)?
            .set_password(&refresh)
            .map_err(|error| Error::Auth(format!("could not save your sign-in: {error}")))?;

        let mut accounts = self.accounts()?;
        accounts.retain(|a| a.uuid != verified.account.uuid);
        for account in &mut accounts {
            account.active = false;
        }
        accounts.push(verified.account.clone());
        self.save_accounts(&accounts)?;
        Ok(verified.account)
    }

    async fn minecraft_session(&self, microsoft_token: &str) -> Result<Verified> {
        let client = self.net.client();
        let xbox: XboxResponse = client
            .post(XBOX_AUTH)
            .json(&json!({
                "Properties": {
                    "AuthMethod": "RPS",
                    "SiteName": "user.auth.xboxlive.com",
                    "RpsTicket": format!("d={microsoft_token}"),
                },
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT",
            }))
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let xsts = client
            .post(XSTS_AUTH)
            .json(&json!({
                "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbox.token] },
                "RelyingParty": "rp://api.minecraftservices.com/",
                "TokenType": "JWT",
            }))
            .header("Accept", "application/json")
            .send()
            .await?;
        if !xsts.status().is_success() {
            let body: XboxError = xsts.json().await.unwrap_or(XboxError { code: None });
            return Err(xbox_problem(body.code));
        }
        let xsts: XboxResponse = xsts.json().await?;
        let user = xsts
            .display_claims
            .xui
            .first()
            .ok_or_else(|| Error::Auth("Xbox Live sent no user".into()))?;

        let login = client
            .post(MINECRAFT_LOGIN)
            .json(&json!({ "identityToken": format!("XBL3.0 x={};{}", user.uhs, xsts.token) }))
            .header("Accept", "application/json")
            .send()
            .await?;
        if login.status().as_u16() == 403 {
            let body = login.text().await.unwrap_or_default();
            if body
                .to_ascii_lowercase()
                .contains("invalid app registration")
            {
                return Err(Error::AppNotApproved);
            }
            return Err(Error::Auth("Minecraft refused the sign-in.".into()));
        }
        let login: MinecraftLogin = login.error_for_status()?.json().await?;

        let entitlements: Entitlements = client
            .get(ENTITLEMENTS)
            .query(&[("requestId", uuid::Uuid::new_v4().to_string())])
            .bearer_auth(&login.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if !owns_minecraft(&entitlements) {
            return Err(Error::NotOwned);
        }

        let profile = client
            .get(PROFILE)
            .bearer_auth(&login.access_token)
            .send()
            .await?;
        if profile.status().as_u16() == 404 {
            return Err(Error::Auth(
                "Choose a Minecraft username at minecraft.net first.".into(),
            ));
        }
        let profile: Profile = profile.error_for_status()?.json().await?;
        let worn = profile.skins.iter().find(|s| s.state == "ACTIVE");
        let skin_url = worn.map(|s| s.url.clone());
        let skin_slim = worn.is_some_and(|s| s.variant.eq_ignore_ascii_case("slim"));

        Ok(Verified {
            lasts: Duration::from_secs(login.expires_in),
            account: Account {
                uuid: profile.id.clone(),
                name: profile.name.clone(),
                owns_game: true,
                verified_at: OffsetDateTime::now_utc(),
                skin_url,
                skin_slim,
                active: true,
            },
            session: Session {
                name: profile.name,
                uuid: profile.id,
                access_token: login.access_token,
                xuid: user.xid.clone().unwrap_or_default(),
            },
        })
    }

    /// The session for the active account. Each launch re-verifies ownership
    /// online. With no internet, an account verified within the grace period
    /// can still play, which never lets a new or unverified account through.
    pub async fn session(&self) -> Result<Session> {
        let account = self.active_account()?;
        let _turn = self.signing.lock().await;
        match self.refresh(&account).await {
            Ok(session) => Ok(session),
            // A session verified minutes ago is still proof, so a busy server doesn't stop play.
            Err(Error::Http(error)) if error.is_status() => self
                .kept_session(&account.uuid)
                .ok_or_else(|| refused(error.status())),
            Err(Error::Http(_)) if may_play_offline(&account) => Ok(Session {
                name: account.name,
                uuid: account.uuid,
                access_token: "0".into(),
                xuid: "0".into(),
            }),
            Err(Error::Http(_)) => Err(Error::Auth(
                "Connect to the internet so we can confirm your account.".into(),
            )),
            Err(error) => Err(error),
        }
    }

    /// A session for Minecraft's own services, like changing a skin. Those need a real
    /// token, so there's no offline session here, and a recent one is used again.
    pub(crate) async fn web_session(&self) -> Result<Session> {
        let account = self.active_account()?;
        let _turn = self.signing.lock().await;
        if let Some(kept) = self.kept_session(&account.uuid) {
            return Ok(kept);
        }
        match self.refresh(&account).await {
            Err(Error::Http(error)) if error.is_status() => Err(refused(error.status())),
            Err(Error::Http(_)) => Err(Error::Auth(
                "Connect to the internet to change this.".into(),
            )),
            other => other,
        }
    }

    fn active_account(&self) -> Result<Account> {
        let account = self
            .accounts()?
            .into_iter()
            .find(|a| a.active)
            .ok_or(Error::SignInRequired)?;
        if !account.owns_game {
            return Err(Error::NotOwned);
        }
        Ok(account)
    }

    fn remember(&self, verified: &Verified) {
        let left = verified.lasts.saturating_sub(TOKEN_MARGIN);
        *self.recent.lock().expect("session poisoned") = Some(Recent {
            session: verified.session.clone(),
            until: Instant::now() + left,
        });
        let session = &verified.session;
        if left.is_zero() {
            self.vault.forget(&session.uuid);
            return;
        }
        let saved = SavedSession {
            name: session.name.clone(),
            uuid: session.uuid.clone(),
            access_token: session.access_token.clone(),
            xuid: session.xuid.clone(),
            until: OffsetDateTime::now_utc().unix_timestamp() + left.as_secs() as i64,
        };
        if let Ok(bytes) = serde_json::to_vec(&saved) {
            self.vault.save(&session.uuid, &bytes);
        }
    }

    /// The session verified most recently for this account, in this run or the last one.
    pub(crate) fn kept_session(&self, uuid: &str) -> Option<Session> {
        let mut recent = self.recent.lock().expect("session poisoned");
        if let Some(kept) = recent.as_ref().filter(|kept| kept.session.uuid == uuid) {
            return (Instant::now() < kept.until).then(|| kept.session.clone());
        }
        let (session, left) =
            saved_session(&self.vault.load(uuid)?, uuid, OffsetDateTime::now_utc())?;
        *recent = Some(Recent {
            session: session.clone(),
            until: Instant::now() + left,
        });
        Some(session)
    }

    /// Drops a session Minecraft no longer accepts, here and in the keychain.
    pub(crate) fn forget_session(&self, uuid: &str) {
        let mut recent = self.recent.lock().expect("session poisoned");
        if recent
            .as_ref()
            .is_some_and(|kept| kept.session.uuid == uuid)
        {
            *recent = None;
        }
        drop(recent);
        self.vault.forget(uuid);
    }

    async fn refresh(&self, account: &Account) -> Result<Session> {
        let entry = keychain(&account.uuid)?;
        let refresh = entry.get_password().map_err(|_| Error::SignInRequired)?;
        let response: TokenResponse = self
            .net
            .client()
            .post(format!("{AUTHORITY}/token"))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", MSA_CLIENT_ID),
                ("refresh_token", refresh.as_str()),
                ("scope", SCOPE),
            ])
            .send()
            .await?
            .json()
            .await?;
        let Some(access) = response.access_token else {
            return Err(Error::SignInRequired);
        };
        if let Some(next) = response.refresh_token {
            let _ = entry.set_password(&next);
        }

        let verified = self.minecraft_session(&access).await?;
        self.remember(&verified);
        let mut accounts = self.accounts()?;
        if let Some(stored) = accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            let active = stored.active;
            *stored = Account {
                active,
                ..verified.account
            };
            self.save_accounts(&accounts)?;
        }
        Ok(verified.session)
    }
}

struct Verified {
    account: Account,
    session: Session,
    /// How long Minecraft said the token is good for.
    lasts: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(owns_game: bool, age_days: i64) -> Account {
        Account {
            uuid: "abc".into(),
            name: "Steve".into(),
            owns_game,
            verified_at: OffsetDateTime::now_utc() - time::Duration::days(age_days),
            skin_url: None,
            skin_slim: false,
            active: true,
        }
    }

    fn entitlements(json: &str) -> Entitlements {
        serde_json::from_str(json).unwrap()
    }

    fn verified(uuid: &str, lasts: Duration) -> Verified {
        Verified {
            account: account(true, 0),
            session: Session {
                name: "Steve".into(),
                uuid: uuid.into(),
                access_token: "real".into(),
                xuid: "1".into(),
            },
            lasts,
        }
    }

    #[test]
    fn a_verified_session_is_kept_for_its_own_account_until_it_nearly_runs_out() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        core.remember(&verified("abc", Duration::from_secs(A_DAY)));
        assert_eq!(core.kept_session("abc").unwrap().access_token, "real");
        assert!(core.kept_session("someone else").is_none());
        core.remember(&verified("abc", TOKEN_MARGIN));
        assert!(core.kept_session("abc").is_none());
    }

    #[test]
    fn a_verified_session_survives_a_restart_until_it_runs_out() {
        let dir = tempfile::tempdir().unwrap();
        let first = Core::new(dir.path(), crate::event::silent()).unwrap();
        first.remember(&verified("abc", Duration::from_secs(A_DAY)));
        // A new run starts with nothing in memory but the same keychain.
        let mut next = Core::new(dir.path(), crate::event::silent()).unwrap();
        next.vault = first.vault.clone();
        assert_eq!(next.kept_session("abc").unwrap().access_token, "real");
        assert!(next.kept_session("someone else").is_none());

        let now = OffsetDateTime::now_utc();
        let saved = first.vault.load("abc").unwrap();
        assert!(saved_session(&saved, "abc", now).is_some());
        let later = now + time::Duration::seconds(A_DAY as i64);
        assert!(saved_session(&saved, "abc", later).is_none());
        assert!(saved_session(b"not json", "abc", now).is_none());

        next.forget_session("abc");
        assert!(first.vault.load("abc").is_none());
        let mut third = Core::new(dir.path(), crate::event::silent()).unwrap();
        third.vault = first.vault.clone();
        assert!(third.kept_session("abc").is_none());
    }

    #[test]
    fn a_session_that_is_nearly_out_is_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        core.remember(&verified("abc", Duration::from_secs(A_DAY)));
        core.remember(&verified("abc", TOKEN_MARGIN));
        assert!(core.vault.load("abc").is_none());
    }

    #[test]
    fn signing_out_forgets_the_kept_session() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        core.remember(&verified("abc", Duration::from_secs(A_DAY)));
        core.remove_account("abc").unwrap();
        assert!(core.kept_session("abc").is_none());
        assert!(core.vault.load("abc").is_none());
    }

    #[test]
    fn a_busy_server_is_not_mistaken_for_being_offline() {
        let busy = refused(Some(reqwest::StatusCode::TOO_MANY_REQUESTS)).to_string();
        assert!(busy.contains("too many sign-ins"), "{busy}");
        let down = refused(Some(reqwest::StatusCode::SERVICE_UNAVAILABLE)).to_string();
        assert!(down.contains("HTTP 503"), "{down}");
    }

    #[test]
    fn an_empty_entitlement_list_is_not_ownership() {
        assert!(!owns_minecraft(&entitlements(r#"{"items":[]}"#)));
        assert!(!owns_minecraft(&entitlements(r#"{}"#)));
    }

    #[test]
    fn unrelated_entitlements_are_not_ownership() {
        assert!(!owns_minecraft(&entitlements(
            r#"{"items":[{"name":"game_dungeons"}]}"#
        )));
    }

    #[test]
    fn store_purchases_and_game_pass_both_count() {
        assert!(owns_minecraft(&entitlements(
            r#"{"items":[{"name":"game_minecraft"}]}"#
        )));
        assert!(owns_minecraft(&entitlements(
            r#"{"items":[{"name":"product_minecraft"}]}"#
        )));
    }

    #[test]
    fn offline_play_needs_recent_verified_ownership() {
        assert!(may_play_offline(&account(true, 1)));
        assert!(!may_play_offline(&account(true, 31)));
        assert!(!may_play_offline(&account(false, 1)));
    }

    #[test]
    fn explains_xbox_errors() {
        assert!(
            xbox_problem(Some(2148916233))
                .to_string()
                .contains("Xbox profile")
        );
        assert!(
            xbox_problem(Some(2148916238))
                .to_string()
                .contains("child account")
        );
    }

    #[tokio::test]
    async fn no_account_means_no_session() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        assert!(matches!(core.session().await, Err(Error::SignInRequired)));
    }

    #[tokio::test]
    async fn an_account_without_the_game_gets_no_session() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        core.save_accounts(&[account(false, 0)]).unwrap();
        assert!(matches!(core.session().await, Err(Error::NotOwned)));
    }
}
