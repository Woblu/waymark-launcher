use serde::{Serialize, Serializer, ser::SerializeStruct};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("network request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("could not read JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not read TOML: {0}")]
    TomlRead(#[from] toml::de::Error),
    #[error("could not write TOML: {0}")]
    TomlWrite(#[from] toml::ser::Error),
    #[error("could not read archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{url} returned HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{url} failed its integrity check")]
    HashMismatch { url: String },
    #[error("{0} was not found")]
    NotFound(String),
    #[error("Mojang has not approved this launcher for Minecraft sign-in yet")]
    AppNotApproved,
    #[error("this Microsoft account does not own Minecraft: Java Edition")]
    NotOwned,
    #[error("{0}")]
    Auth(String),
    #[error("sign in with an account that owns Minecraft before playing")]
    SignInRequired,
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// A stable identifier the UI can branch on, unlike the display text.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Http(_) => "network",
            Error::Json(_) | Error::TomlRead(_) | Error::TomlWrite(_) => "format",
            Error::Zip(_) => "archive",
            Error::Status { .. } => "http_status",
            Error::HashMismatch { .. } => "hash_mismatch",
            Error::NotFound(_) => "not_found",
            Error::AppNotApproved => "app_not_approved",
            Error::NotOwned => "not_owned",
            Error::Auth(_) => "auth",
            Error::SignInRequired => "sign_in_required",
            Error::Other(_) => "other",
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut out = s.serialize_struct("Error", 2)?;
        out.serialize_field("code", self.code())?;
        out.serialize_field("message", &self.to_string())?;
        out.end()
    }
}
