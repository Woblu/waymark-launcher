//! Waymark accounts and friends. An account is a username and a password on a
//! small hosted database, with no email address. It has nothing to do with the
//! Minecraft sign-in in `auth.rs`, and nothing from that is ever sent here.
//!
//! The server only offers named functions that act as the signed-in player, so
//! `supabase/migrations` is the place to look for what a player can do.

use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Core;
use crate::brand::{FRIENDS_KEY, FRIENDS_URL, KEYCHAIN_SERVICE};
use crate::error::{Error, Result};
use crate::share::SharedResult;

/// Tokens are renewed this long before they run out.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);
const ACCOUNT_DOMAIN: &str = "users.waymark.invalid";
const UNAVAILABLE: &str = "Friends aren't available right now";

/// Who's signed in. The access token only lives in memory.
#[derive(Clone)]
pub(crate) struct Login {
    access: String,
    expires: Instant,
    username: String,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    user: TokenUser,
}

#[derive(Deserialize)]
struct TokenUser {
    user_metadata: TokenMetadata,
}

#[derive(Deserialize)]
struct TokenMetadata {
    username: String,
}

#[derive(Deserialize)]
struct ServerError {
    #[serde(default)]
    message: String,
    #[serde(default)]
    error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Friend {
    pub id: String,
    pub username: String,
    pub online: bool,
    pub last_seen: Option<String>,
    pub playing: Option<String>,
}

/// A friend request, or someone blocked.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherPlayer {
    pub id: String,
    pub username: String,
}

/// An instance a friend sent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedByFriend {
    pub id: String,
    pub name: String,
    pub from: String,
    pub at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendOverview {
    pub username: String,
    pub show_status: bool,
    pub friends: Vec<Friend>,
    pub incoming: Vec<OtherPlayer>,
    pub outgoing: Vec<OtherPlayer>,
    pub blocked: Vec<OtherPlayer>,
    pub shares: Vec<SharedByFriend>,
}

pub const fn friends_available() -> bool {
    !FRIENDS_URL.is_empty()
}

/// Sign-in goes by email underneath, so each username has an address of its own
/// that can never receive anything.
fn account_address(username: &str) -> String {
    format!("{}@{ACCOUNT_DOMAIN}", username.trim().to_lowercase())
}

fn valid_username(username: &str) -> bool {
    (3..=16).contains(&username.len())
        && username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// What the server's short codes mean to a player.
fn explain(code: &str) -> String {
    match code {
        "no_such_player" => "There's nobody with that username",
        "not_friends" => "You can only send that to a friend",
        "too_many_requests" => "You have too many friend requests waiting",
        "too_many_shares" => "You've sent too many instances today",
        "too_many_signups" => "Too many accounts were made from here. Try again in an hour",
        "username_taken" => "That username is taken",
        "bad_username" => "Usernames are 3 to 16 letters, numbers or underscores",
        "bad_password" => "Passwords need at least 8 characters",
        _ => UNAVAILABLE,
    }
    .to_string()
}

impl Core {
    /// Kept per data folder, so a test copy can't sign the real one out.
    fn friends_keychain(&self) -> Result<keyring::Entry> {
        let folder = blake3::hash(self.paths.root().to_string_lossy().as_bytes()).to_hex();
        keyring::Entry::new(KEYCHAIN_SERVICE, &format!("friends:{}", &folder[..16]))
            .map_err(|error| Error::Auth(error.to_string()))
    }

    async fn store_tokens(&self, tokens: Tokens) -> Result<Login> {
        self.friends_keychain()?
            .set_password(&tokens.refresh_token)
            .map_err(|error| Error::Auth(error.to_string()))?;
        let login = Login {
            access: tokens.access_token,
            expires: Instant::now()
                + Duration::from_secs(tokens.expires_in).saturating_sub(TOKEN_MARGIN),
            username: tokens.user.user_metadata.username,
        };
        *self.login.lock().await = Some(login.clone());
        Ok(login)
    }

    async fn token_request(&self, grant: &str, body: Value) -> Result<reqwest::Response> {
        Ok(self
            .net
            .client()
            .post(format!("{FRIENDS_URL}/auth/v1/token"))
            .query(&[("grant_type", grant)])
            .header("apikey", FRIENDS_KEY)
            .json(&body)
            .send()
            .await?)
    }

    /// The signed-in player, renewing the token if it's run out.
    async fn login(&self) -> Result<Option<Login>> {
        if let Some(login) = self.login.lock().await.clone()
            && login.expires > Instant::now()
        {
            return Ok(Some(login));
        }
        let entry = self.friends_keychain()?;
        let Ok(refresh) = entry.get_password() else {
            return Ok(None);
        };
        let response = self
            .token_request("refresh_token", json!({ "refresh_token": refresh }))
            .await?;
        if response.status().is_client_error() {
            // The account is gone, or signed out from somewhere else.
            let _ = entry.delete_credential();
            *self.login.lock().await = None;
            return Ok(None);
        }
        let tokens: Tokens = response.error_for_status()?.json().await?;
        Ok(Some(self.store_tokens(tokens).await?))
    }

    async fn call_value(&self, function: &str, arguments: Value) -> Result<Value> {
        let Some(login) = self.login().await? else {
            return Err(Error::Auth("Sign in to your Waymark account first".into()));
        };
        let response = self
            .net
            .client()
            .post(format!("{FRIENDS_URL}/rest/v1/rpc/{function}"))
            .header("apikey", FRIENDS_KEY)
            .bearer_auth(login.access)
            .json(&arguments)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            let error: Option<ServerError> = serde_json::from_str(&text).ok();
            return Err(Error::Other(explain(
                error.as_ref().map_or("", |error| error.message.as_str()),
            )));
        }
        Ok(if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text)?
        })
    }

    async fn call<T: DeserializeOwned>(&self, function: &str, arguments: Value) -> Result<T> {
        Ok(serde_json::from_value(
            self.call_value(function, arguments).await?,
        )?)
    }

    /// The signed-in username, if there is one.
    pub async fn friend_account(&self) -> Result<Option<String>> {
        Ok(self.login().await?.map(|login| login.username))
    }

    pub async fn friend_sign_in(&self, username: &str, password: &str) -> Result<String> {
        let response = self
            .token_request(
                "password",
                json!({ "email": account_address(username), "password": password }),
            )
            .await?;
        if response.status().is_client_error() {
            return Err(Error::Auth("That username or password isn't right".into()));
        }
        let tokens: Tokens = response.error_for_status()?.json().await?;
        Ok(self.store_tokens(tokens).await?.username)
    }

    pub async fn friend_sign_up(&self, username: &str, password: &str) -> Result<String> {
        let username = username.trim();
        if !valid_username(username) {
            return Err(Error::Auth(explain("bad_username")));
        }
        let response = self
            .net
            .client()
            .post(format!("{FRIENDS_URL}/functions/v1/create-account"))
            .header("apikey", FRIENDS_KEY)
            .bearer_auth(FRIENDS_KEY)
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await?;
        if !response.status().is_success() {
            let error: Option<ServerError> = response.json().await.ok();
            return Err(Error::Auth(explain(
                error.as_ref().map_or("", |error| error.error.as_str()),
            )));
        }
        self.friend_sign_in(username, password).await
    }

    pub async fn friend_sign_out(&self) -> Result<()> {
        if let Ok(Some(login)) = self.login().await {
            let _ = self
                .net
                .client()
                .post(format!("{FRIENDS_URL}/auth/v1/logout"))
                .header("apikey", FRIENDS_KEY)
                .bearer_auth(login.access)
                .send()
                .await;
        }
        let _ = self.friends_keychain()?.delete_credential();
        *self.login.lock().await = None;
        Ok(())
    }

    /// Removes the account, its friends and anything sent to it, for good.
    pub async fn friend_delete_account(&self) -> Result<()> {
        self.call_value("delete_account", json!({})).await?;
        let _ = self.friends_keychain()?.delete_credential();
        *self.login.lock().await = None;
        Ok(())
    }

    pub async fn friend_overview(&self) -> Result<FriendOverview> {
        self.call("friend_overview", json!({})).await
    }

    pub async fn friend_request(&self, username: &str) -> Result<()> {
        self.call_value("request_friend", json!({ "target": username.trim() }))
            .await
            .map(drop)
    }

    pub async fn friend_respond(&self, request: &str, accept: bool) -> Result<()> {
        self.call_value(
            "respond_friend",
            json!({ "request": request, "accept": accept }),
        )
        .await
        .map(drop)
    }

    pub async fn friend_remove(&self, other: &str) -> Result<()> {
        self.call_value("remove_friend", json!({ "other": other }))
            .await
            .map(drop)
    }

    pub async fn friend_block(&self, other: &str) -> Result<()> {
        self.call_value("block_player", json!({ "other": other }))
            .await
            .map(drop)
    }

    pub async fn friend_unblock(&self, other: &str) -> Result<()> {
        self.call_value("unblock_player", json!({ "other": other }))
            .await
            .map(drop)
    }

    pub async fn friend_show_status(&self, shown: bool) -> Result<()> {
        self.call_value("set_show_status", json!({ "shown": shown }))
            .await
            .map(drop)
    }

    /// Tells friends this player is around, and what they're playing. Does
    /// nothing unless someone is signed in.
    pub async fn friend_heartbeat(&self) -> Result<()> {
        if !friends_available() || self.login().await?.is_none() {
            return Ok(());
        }
        let running: Vec<String> = self
            .games
            .lock()
            .expect("game list poisoned")
            .keys()
            .cloned()
            .collect();
        let playing = running
            .iter()
            .find_map(|id| self.instance(id).ok())
            .map(|instance| instance.name);
        self.call_value("heartbeat", json!({ "now_playing": playing }))
            .await
            .map(drop)
    }

    /// Sends an instance to a friend, and says what couldn't travel with it.
    pub async fn friend_send_instance(&self, friend: &str, instance: &str) -> Result<Vec<String>> {
        let share = self.share_code(instance)?;
        let name = self.instance(instance)?.name;
        self.call_value(
            "send_share",
            json!({ "friend": friend, "title": name, "share_code": share.code }),
        )
        .await?;
        Ok(share.left_out)
    }

    pub async fn friend_install_share(&self, share: &str) -> Result<SharedResult> {
        let code: Option<String> = self.call("share_code", json!({ "share": share })).await?;
        let code = code.ok_or_else(|| Error::NotFound("that shared instance".into()))?;
        let result = self.install_share(&code).await?;
        self.friend_drop_share(share).await?;
        Ok(result)
    }

    pub async fn friend_drop_share(&self, share: &str) -> Result<()> {
        self.call_value("drop_share", json!({ "share": share }))
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_short_and_plain() {
        assert!(
            valid_username("Steve_123")
                && valid_username("abc")
                && valid_username("abcdefghijklmnop")
        );
        assert!(!valid_username("ab") && !valid_username("abcdefghijklmnopq"));
        assert!(
            !valid_username("two words") && !valid_username("nöpe") && !valid_username("a@b.com")
        );
    }

    #[test]
    fn each_username_has_one_address() {
        assert_eq!(account_address("  Steve "), account_address("steve"));
        assert!(account_address("steve").ends_with(".invalid"));
    }

    #[test]
    fn server_codes_become_sentences() {
        assert_eq!(
            explain("no_such_player"),
            "There's nobody with that username"
        );
        assert_eq!(explain("something new"), UNAVAILABLE);
    }

    #[test]
    fn reads_what_the_server_sends() {
        let overview: FriendOverview = serde_json::from_value(json!({
            "username": "Steve",
            "showStatus": true,
            "friends": [{ "id": "1", "username": "Alex", "online": true, "lastSeen": null, "playing": "Vanilla" }],
            "incoming": [{ "id": "2", "username": "Sam" }],
            "outgoing": [],
            "blocked": [],
            "shares": [{ "id": "3", "name": "Pack", "from": "Alex", "at": "2026-11-01T10:00:00Z" }],
        }))
        .unwrap();
        assert_eq!(overview.friends[0].playing.as_deref(), Some("Vanilla"));
        assert_eq!(overview.shares[0].from, "Alex");
    }
}
