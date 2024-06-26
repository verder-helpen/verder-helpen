use std::{error::Error as StdError, fmt::Display};

use josekit::{jwe::JweDecrypter, jws::JwsVerifier};
use serde::Deserialize;
use verder_helpen_jwt::{EncryptionKeyConfig, SignKeyConfig};

#[derive(Debug)]
pub enum Error {
    Yaml(serde_yaml::Error),
    Json(serde_json::Error),
    Jwt(verder_helpen_jwt::Error),
}

impl From<serde_yaml::Error> for Error {
    fn from(e: serde_yaml::Error) -> Error {
        Error::Yaml(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::Json(e)
    }
}

impl From<verder_helpen_jwt::Error> for Error {
    fn from(e: verder_helpen_jwt::Error) -> Error {
        Error::Jwt(e)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Yaml(e) => e.fmt(f),
            Error::Json(e) => e.fmt(f),
            Error::Jwt(e) => e.fmt(f),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Yaml(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Jwt(e) => Some(e),
        }
    }
}

fn default_as_true() -> bool {
    true
}

#[derive(Deserialize, Debug)]
struct RawConfig {
    server_url: String,
    internal_url: String,
    #[serde(default = "default_as_true")]
    use_attr_url: bool,
    decryption_privkey: EncryptionKeyConfig,
    signature_pubkey: SignKeyConfig,
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "RawConfig")]
pub struct Config {
    server_url: String,
    internal_url: String,
    use_attr_url: bool,
    decrypter: Box<dyn JweDecrypter>,
    verifier: Box<dyn JwsVerifier>,
}

// This tryfrom can be removed once try_from for fields lands in serde
impl TryFrom<RawConfig> for Config {
    type Error = Error;

    fn try_from(config: RawConfig) -> Result<Config, Error> {
        Ok(Config {
            server_url: config.server_url,
            internal_url: config.internal_url,
            use_attr_url: config.use_attr_url,
            decrypter: Box::<dyn JweDecrypter>::try_from(config.decryption_privkey)?,
            verifier: Box::<dyn JwsVerifier>::try_from(config.signature_pubkey)?,
        })
    }
}

impl Config {
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn internal_url(&self) -> &str {
        &self.internal_url
    }

    pub fn decrypter(&self) -> &dyn JweDecrypter {
        self.decrypter.as_ref()
    }

    pub fn verifier(&self) -> &dyn JwsVerifier {
        self.verifier.as_ref()
    }

    pub fn use_attr_url(&self) -> bool {
        self.use_attr_url
    }
}
