//! Microsoft sign-in and the Minecraft ownership check.
//!
//! The chain is Microsoft, Xbox Live, XSTS, Minecraft, then entitlements. An
//! account is only stored once it's proven to own the game, and every launch
//! goes through `Core::session`, which is what stops a release build from
//! reaching gameplay without that proof.

use std::fs;
use std::time::Duration;

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
        f.debug_struct("Session").field("name", &self.name).field("uuid", &self.uuid).finish_non_exhaustive()
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
    entitlements.items.iter().any(|item| item.name == "game_minecraft" || item.name == "product_minecraft")
}

fn keychain(uuid: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("refresh:{uuid}"))
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

    fn save_accounts(&self, accounts: &[Account]) -> Result<()> {
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
        let mut accounts = self.accounts()?;
        accounts.retain(|a| a.uuid != uuid);
        if !accounts.iter().any(|a| a.active)
            && let Some(first) = accounts.first_mut() {
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
                return Err(Error::Auth("The sign-in code expired. Please try again.".into()));
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
                    let detail = response.error_description.or(response.error).unwrap_or_default();
                    return Err(Error::Auth(detail));
                }
            }
        };

        let access = tokens.access_token.ok_or_else(|| Error::Auth("Microsoft sent no token".into()))?;
        let refresh = tokens.refresh_token.ok_or_else(|| Error::Auth("Microsoft sent no refresh token".into()))?;
        let verified = self.minecraft_session(&access).await?;

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
        let user = xsts.display_claims.xui.first().ok_or_else(|| Error::Auth("Xbox Live sent no user".into()))?;

        let login = client
            .post(MINECRAFT_LOGIN)
            .json(&json!({ "identityToken": format!("XBL3.0 x={};{}", user.uhs, xsts.token) }))
            .header("Accept", "application/json")
            .send()
            .await?;
        if login.status().as_u16() == 403 {
            let body = login.text().await.unwrap_or_default();
            if body.to_ascii_lowercase().contains("invalid app registration") {
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

        let profile = client.get(PROFILE).bearer_auth(&login.access_token).send().await?;
        if profile.status().as_u16() == 404 {
            return Err(Error::Auth("Choose a Minecraft username at minecraft.net first.".into()));
        }
        let profile: Profile = profile.error_for_status()?.json().await?;
        let skin_url = profile.skins.iter().find(|s| s.state == "ACTIVE").map(|s| s.url.clone());

        Ok(Verified {
            account: Account {
                uuid: profile.id.clone(),
                name: profile.name.clone(),
                owns_game: true,
                verified_at: OffsetDateTime::now_utc(),
                skin_url,
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
        let account = self.accounts()?.into_iter().find(|a| a.active).ok_or(Error::SignInRequired)?;
        if !account.owns_game {
            return Err(Error::NotOwned);
        }
        match self.refresh(&account).await {
            Ok(session) => Ok(session),
            Err(Error::Http(_)) if may_play_offline(&account) => Ok(Session {
                name: account.name,
                uuid: account.uuid,
                access_token: "0".into(),
                xuid: "0".into(),
            }),
            Err(Error::Http(_)) => Err(Error::Auth("Connect to the internet so we can confirm your account.".into())),
            Err(error) => Err(error),
        }
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
        let mut accounts = self.accounts()?;
        if let Some(stored) = accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            let active = stored.active;
            *stored = Account { active, ..verified.account };
            self.save_accounts(&accounts)?;
        }
        Ok(verified.session)
    }
}

struct Verified {
    account: Account,
    session: Session,
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
            active: true,
        }
    }

    fn entitlements(json: &str) -> Entitlements {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn an_empty_entitlement_list_is_not_ownership() {
        assert!(!owns_minecraft(&entitlements(r#"{"items":[]}"#)));
        assert!(!owns_minecraft(&entitlements(r#"{}"#)));
    }

    #[test]
    fn unrelated_entitlements_are_not_ownership() {
        assert!(!owns_minecraft(&entitlements(r#"{"items":[{"name":"game_dungeons"}]}"#)));
    }

    #[test]
    fn store_purchases_and_game_pass_both_count() {
        assert!(owns_minecraft(&entitlements(r#"{"items":[{"name":"game_minecraft"}]}"#)));
        assert!(owns_minecraft(&entitlements(r#"{"items":[{"name":"product_minecraft"}]}"#)));
    }

    #[test]
    fn offline_play_needs_recent_verified_ownership() {
        assert!(may_play_offline(&account(true, 1)));
        assert!(!may_play_offline(&account(true, 31)));
        assert!(!may_play_offline(&account(false, 1)));
    }

    #[test]
    fn explains_xbox_errors() {
        assert!(xbox_problem(Some(2148916233)).to_string().contains("Xbox profile"));
        assert!(xbox_problem(Some(2148916238)).to_string().contains("child account"));
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
