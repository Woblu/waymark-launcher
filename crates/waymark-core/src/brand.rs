//! Every name the product answers to, in one place.

pub const PRODUCT_NAME: &str = "Waymark";
pub const DATA_DIR_NAME: &str = "Waymark";
pub const KEYCHAIN_SERVICE: &str = "app.waymark.launcher";
pub const USER_AGENT: &str = concat!("waymark-launcher/", env!("CARGO_PKG_VERSION"));
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Azure app registration. A client ID is public by design.
pub const MSA_CLIENT_ID: &str = "aea84696-cbfe-470c-8138-023b193ce3cb";

/// Baked in at build time so the key never lives in the repository.
pub const CURSEFORGE_API_KEY: Option<&str> = option_env!("WAYMARK_CURSEFORGE_API_KEY");
