use serde::Serialize;
use tera::Context;

#[cfg(feature = "session_db")]
use crate::session::{Session, SessionDBConn};
#[cfg(feature = "session_db")]
use crate::types::platform_token::HostToken;
use crate::{
    config::Config,
    error::Error,
    templates::{RenderType, RenderedContent, TEMPLATES},
    translations::Translations,
    types::{Credentials, GuestAuthResult},
};

/// convert a list of guest jwt's to a list of credentials
pub fn collect_credentials(
    guest_auth_results: &[GuestAuthResult],
    config: &Config,
) -> Result<Vec<Credentials>, Error> {
    let mut credentials: Vec<Credentials> = vec![];

    for guest_auth_result in guest_auth_results {
        let attributes = if let Some(result) = &guest_auth_result.auth_result {
            verder_helpen_jwt::dangerous_decrypt_auth_result_without_verifying_expiration(
                result,
                config.verifier(),
                config.decrypter(),
            )?
            .attributes
        } else {
            None
        };

        credentials.push(Credentials {
            name: guest_auth_result.name.clone(),
            purpose: guest_auth_result.purpose.clone(),
            attributes,
            created_at: guest_auth_result.created_at,
        });
    }

    credentials.sort_by(|x, y| x.created_at.cmp(&y.created_at));

    Ok(credentials)
}

#[derive(Debug, Serialize)]
pub struct Sorted {
    pub purpose: Option<String>,
    pub name: Option<String>,
    pub attributes: Option<Vec<(String, String)>>,
}

/// sorted credentials are sorted by their name (key)
impl From<Credentials> for Sorted {
    fn from(credentials: Credentials) -> Self {
        let attributes = if let Some(attributes) = credentials.attributes {
            let mut attributes = attributes.into_iter().collect::<Vec<(String, String)>>();

            attributes.sort_by(|x, y| x.0.cmp(&y.0));
            Some(attributes)
        } else {
            None
        };

        Sorted {
            purpose: credentials.purpose,
            name: credentials.name,
            attributes,
        }
    }
}

/// render a list of users and credentials to html or json
pub fn render(
    config: &Config,
    credentials: Vec<Credentials>,
    render_type: RenderType,
    translations: &Translations,
) -> Result<RenderedContent, Error> {
    if render_type == RenderType::Json {
        let content = serde_json::to_string(&credentials)?;
        return Ok(RenderedContent {
            content,
            render_type,
        });
    }

    let mut context = Context::new();

    let sorted_credentials: Vec<Sorted> = credentials.into_iter().map(Sorted::from).collect();

    context.insert("translations", translations.all());
    context.insert("credentials", &sorted_credentials);
    if config.auth_provider().is_some() {
        let logout_url = format!("{}/auth/logout", config.external_host_url());
        context.insert("logout_url", &logout_url);
    }

    if let Some(custom_css) = &config.custom_css {
        context.insert("custom_css", &custom_css);
    }

    let content = TEMPLATES.render("credentials.html", &context)?;

    Ok(RenderedContent {
        content,
        render_type,
    })
}

/// retrieve authentication results for all users in a room
/// the id of the room is provided by a host jwt
#[cfg(feature = "session_db")]
pub async fn get_credentials_for_host(
    host_token: HostToken,
    config: &Config,
    db: &SessionDBConn,
) -> Result<Vec<Credentials>, Error> {
    let sessions = Session::find_by_room_id(host_token.room_id, db).await?;
    for session in &sessions {
        session.mark_active(db).await?;
    }

    let guest_auth_results = sessions
        .into_iter()
        .map(|session: Session| GuestAuthResult {
            purpose: Some(session.guest_token.purpose),
            name: Some(session.guest_token.name),
            auth_result: session.auth_result,
            created_at: session.created_at,
        })
        .collect::<Vec<GuestAuthResult>>();

    collect_credentials(&guest_auth_results, config)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::DateTime;
    use josekit::{
        jwe::{JweDecrypter, JweEncrypter},
        jws::{alg::hmac::HmacJwsAlgorithm, JwsSigner, JwsVerifier},
    };
    use verder_helpen_jwt::{sign_and_encrypt_auth_result, EncryptionKeyConfig, SignKeyConfig};
    use verder_helpen_proto::{AuthResult, AuthStatus};

    use super::*;
    use crate::config::AuthDuringCommConfig;

    const EC_PUBKEY: &str = r"
    type: EC
    key: |
        -----BEGIN PUBLIC KEY-----
        MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEZLquEijJ7cP7K9qIHG7EvCTph53N
        4nz61OgeuZWdvM7LyBVXuW53nY+b6NJmophgcZHqzSiLbk+jPvIGvVUxzQ==
        -----END PUBLIC KEY-----
    ";

    const EC_PRIVKEY: &str = r"
    type: EC
    key: |
        -----BEGIN PRIVATE KEY-----
        MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgJdHGkAfKUVshsNPQ
        5UA9sNCf74eALrLrtBQE1nDFlv+hRANCAARkuq4SKMntw/sr2ogcbsS8JOmHnc3i
        fPrU6B65lZ28zsvIFVe5bnedj5vo0maimGBxkerNKItuT6M+8ga9VTHN
        -----END PRIVATE KEY-----
    ";
    const HOST_SECRET: &str = "54f0a09305eaa1d3ffc3ccb6035e95871eecbfa964404332ffddad52d43bf7b1";
    const GUEST_SECRET: &str = "9e4ed6fdc6f7b8fb78f500d3abf3a042412140703249e2fe5671ecdab7e694bb";

    fn remove_whitespace(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn render_test() {
        let enc_config: EncryptionKeyConfig = serde_yaml::from_str(EC_PUBKEY).unwrap();
        let dec_config: EncryptionKeyConfig = serde_yaml::from_str(EC_PRIVKEY).unwrap();

        let decrypter = Box::<dyn JweDecrypter>::try_from(dec_config).unwrap();
        let encrypter = Box::<dyn JweEncrypter>::try_from(enc_config).unwrap();

        let sig_config: SignKeyConfig = serde_yaml::from_str(EC_PRIVKEY).unwrap();
        let ver_config: SignKeyConfig = serde_yaml::from_str(EC_PUBKEY).unwrap();
        let widget_sig_config: SignKeyConfig = serde_yaml::from_str(EC_PRIVKEY).unwrap();

        let signer = Box::<dyn JwsSigner>::try_from(sig_config).unwrap();
        let verifier = Box::<dyn JwsVerifier>::try_from(ver_config).unwrap();

        let widget_signer = Box::<dyn JwsSigner>::try_from(widget_sig_config).unwrap();
        let start_auth_signer = widget_signer.clone();
        let guest_verifier = HmacJwsAlgorithm::Hs256
            .verifier_from_bytes(GUEST_SECRET)
            .unwrap();
        let host_verifier = HmacJwsAlgorithm::Hs256
            .verifier_from_bytes(HOST_SECRET)
            .unwrap();

        let mut test_attributes: HashMap<String, String> = HashMap::new();

        test_attributes.insert("age".to_string(), "42".to_string());
        test_attributes.insert("email".to_string(), "email@example.com".to_string());

        let in_result = AuthResult {
            status: AuthStatus::Success,
            attributes: Some(test_attributes),
            session_url: None,
        };
        let jwe =
            sign_and_encrypt_auth_result(&in_result, signer.as_ref(), encrypter.as_ref()).unwrap();

        let guest_auth_results = vec![GuestAuthResult {
            purpose: Some("test_purpose".to_string()),
            name: Some("John Doe".to_string()),
            auth_result: Some(jwe),
            created_at: DateTime::from_timestamp_nanos(0),
        }];

        let auth_during_comm = AuthDuringCommConfig {
            core_url: "https://example.com".to_string(),
            widget_url: "https://example.com".to_string(),
            display_name: "comm-common".to_string(),
            widget_signer,
            start_auth_signer,
            start_auth_key_id: "not-needed".into(),
            guest_verifier: Box::new(guest_verifier),
            host_verifier: Box::new(host_verifier),
        };

        let config: Config = Config {
            internal_url: "https://example.com".to_string(),
            external_host_url: None,
            external_guest_url: None,
            default_locale: String::from("nl"),
            translations: HashMap::new(),
            decrypter,
            auth_provider: None,
            verifier,
            auth_during_comm,
            custom_css: None,
        };

        let translations = Translations {
            translations: HashMap::from([
                ("attributes".to_string(), "Gegevens".to_string()),
                ("title".to_string(), "Gegevens".to_string()),
                ("age".to_string(), "Leeftijd".to_string()),
                ("email".to_string(), "E-mailadres".to_string()),
                ("secured_by".to_string(), "Beveiligd door".to_string()),
            ]),
            language: "nl".to_string(),
        };

        let credentials = collect_credentials(&guest_auth_results, &config).unwrap();
        let actual = render(&config, credentials, RenderType::Html, &translations).unwrap();
        let expected: &str =
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta \
             name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Verder \
             Helpen</title><script src=\"attribute.js\"></script><link href=\"attribute.css\" \
             rel=\"stylesheet\" /></head><body><main><div class=\"container\"><div \
             class=\"credential\"><div><h4>John \
             Doe</h4></div><div><dl><dt><span>Leeftijd</span></dt><dd><div><span \
             class=\"icon\"></span><span \
             class=\"text\">42</span></div></dd><dt><span>E-mailadres</span></dt><dd><div><span \
             class=\"icon\"></span><span \
             class=\"text\">email@example.com</span></div></dd></dl></div></div><div \
             class=\"footer\"><span class=\"text\">Beveiligd door</span><span \
             class=\"logo\"></span></div></div></main></body></html>";

        assert_eq!(
            remove_whitespace(expected),
            remove_whitespace(actual.content())
        );

        let credentials = collect_credentials(&guest_auth_results, &config).unwrap();
        let rendered = render(&config, credentials, RenderType::Json, &translations).unwrap();
        let result: serde_json::Value = serde_json::from_str(rendered.content()).unwrap();
        let expected = serde_json::json! {
            [{
                "purpose": "test_purpose",
                "name": "John Doe",
                "attributes": { "age":"42", "email": "email@example.com" },
                "created_at": "1970-01-01T00:00:00Z",
            }]
        };

        assert_eq!(result, expected);
    }
}
