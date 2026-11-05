//! Every name the product answers to, in one place.

pub const PRODUCT_NAME: &str = "Waymark Launcher";
/// Still "Waymark" from before the app was renamed, so players keep their instances.
/// The keychain service stays the same for the same reason: changing either one
/// would leave every existing install with no data and no sign-in.
pub const DATA_DIR_NAME: &str = "Waymark";
pub const KEYCHAIN_SERVICE: &str = "app.waymark.launcher";
pub const USER_AGENT: &str = concat!("waymark-launcher/", env!("CARGO_PKG_VERSION"));
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Discord application rich presence shows as. Discord takes the name it
/// shows ("Playing ...") and the small icon in voice channels from the
/// application's own settings in the Developer Portal, not from this app.
/// An application ID is public by design. An empty one turns rich presence off.
pub const DISCORD_CLIENT_ID: &str = "1567673605003944039";

/// The Azure app registration. A client ID is public by design.
pub const MSA_CLIENT_ID: &str = "aea84696-cbfe-470c-8138-023b193ce3cb";

/// The friends server. Both are public by design, like a client ID, because the
/// database only lets a signed-in player act as themselves.
pub const FRIENDS_URL: &str = "https://exxqoiymrbpjzqslvijs.supabase.co";
pub const FRIENDS_KEY: &str = "sb_publishable_L3au0kJ_7GcshgPDG31_7A_XZHUyOAd";

/// Baked in at build time so the key never lives in the repository.
pub const CURSEFORGE_API_KEY: Option<&str> = option_env!("WAYMARK_CURSEFORGE_API_KEY");

/// Every published release, newest first. Pre-releases are only ever looked
/// for here, by players who turned on beta updates; everyone else follows the
/// site's `/updates/latest.json`, which GitHub points at the newest full release.
pub const RELEASES_API: &str = "https://api.github.com/repos/Woblu/waymark-launcher/releases?per_page=20";
