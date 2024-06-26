use std::{collections::HashMap, error::Error as StdError, fmt::Display};

use josekit::{jwe::JweEncrypter, jws::JwsSigner};
use serde::Deserialize;
use verder_helpen_jwt::{EncryptionKeyConfig, SignKeyConfig};

#[derive(Debug)]
pub enum Error {
    UnknownAttribute(String),
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
            Error::UnknownAttribute(a) => f.write_fmt(format_args!("Unknown attribute {a}")),
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
            Error::UnknownAttribute(_) => None,
        }
    }
}

#[derive(Deserialize, Debug)]
struct RawConfig {
    server_url: String,
    internal_url: String,
    attributes: HashMap<String, String>,
    #[serde(default = "bool::default")]
    with_session: bool,
    encryption_pubkey: EncryptionKeyConfig,
    signing_privkey: SignKeyConfig,
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "RawConfig")]
pub struct Config {
    server_url: String,
    internal_url: String,
    attributes: HashMap<String, String>,
    with_session: bool,
    encrypter: Box<dyn JweEncrypter>,
    signer: Box<dyn JwsSigner>,
}

// This tryfrom can be removed once try_from for fields lands in serde
impl TryFrom<RawConfig> for Config {
    type Error = Error;

    fn try_from(config: RawConfig) -> Result<Config, Error> {
        Ok(Config {
            server_url: config.server_url,
            internal_url: config.internal_url,
            attributes: config.attributes,
            with_session: config.with_session,
            encrypter: Box::<dyn JweEncrypter>::try_from(config.encryption_pubkey)?,
            signer: Box::<dyn JwsSigner>::try_from(config.signing_privkey)?,
        })
    }
}

impl Config {
    pub fn verify_attributes(&self, attributes: &[String]) -> Result<(), Error> {
        for attribute in attributes {
            self.attributes
                .get(attribute)
                .ok_or_else(|| Error::UnknownAttribute(attribute.clone()))?;
        }

        Ok(())
    }

    pub fn map_attributes(&self, attributes: &[String]) -> Result<HashMap<String, String>, Error> {
        let mut result: HashMap<String, String> = HashMap::new();
        for attribute in attributes {
            result.insert(
                attribute.clone(),
                self.attributes
                    .get(attribute)
                    .ok_or_else(|| Error::UnknownAttribute(attribute.clone()))?
                    .clone(),
            );
        }

        Ok(result)
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn internal_url(&self) -> &str {
        &self.internal_url
    }

    pub fn with_session(&self) -> bool {
        self.with_session
    }

    pub fn encrypter(&self) -> &dyn JweEncrypter {
        self.encrypter.as_ref()
    }

    pub fn signer(&self) -> &dyn JwsSigner {
        self.signer.as_ref()
    }
}
