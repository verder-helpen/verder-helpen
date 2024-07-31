use std::{collections::HashMap, error::Error as StdError, fmt::Display};

use askama::Template;
use base64::URL_SAFE_NO_PAD;
use config::Config;
use reqwest::Url;
use rocket::{
    form::FromForm, get, launch, post, request::FromParam, response::{content::RawHtml, Redirect}, routes, serde::json::Json, State
};
use verder_helpen_common::{
    sign_and_encrypt_auth_result, AuthResult, AuthStatus, SessionActivity, StartAuthRequest,
    StartAuthResponse,
};

mod config;

#[derive(Debug)]
enum Error {
    Config(config::Error),
    Decode(base64::DecodeError),
    Template(askama::Error),
    Json(serde_json::Error),
    Utf(std::str::Utf8Error),
    Jwt(verder_helpen_common::Error),
}

impl<'r, 'o: 'r> rocket::response::Responder<'r, 'o> for Error {
    fn respond_to(self, request: &'r rocket::Request<'_>) -> rocket::response::Result<'o> {
        let debug_error = rocket::response::Debug::from(self);
        debug_error.respond_to(request)
    }
}

impl From<config::Error> for Error {
    fn from(e: config::Error) -> Error {
        Error::Config(e)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Error {
        Error::Decode(e)
    }
}

impl From<askama::Error> for Error {
    fn from(e: askama::Error) -> Error {
        Error::Template(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::Json(e)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Error {
        Error::Utf(e)
    }
}

impl From<verder_helpen_common::Error> for Error {
    fn from(e: verder_helpen_common::Error) -> Error {
        Error::Jwt(e)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Config(e) => e.fmt(f),
            Error::Decode(e) => e.fmt(f),
            Error::Template(e) => e.fmt(f),
            Error::Utf(e) => e.fmt(f),
            Error::Json(e) => e.fmt(f),
            Error::Jwt(e) => e.fmt(f),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Config(e) => Some(e),
            Error::Decode(e) => Some(e),
            Error::Template(e) => Some(e),
            Error::Utf(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Jwt(e) => Some(e),
        }
    }
}

#[derive(Template)]
#[template(path = "confirm.html")]
struct ConfirmTemplate {
    dologin: String,
    dologout: String,
    attributes: HashMap<String, String>,
}

#[derive(FromForm, Debug)]
struct SessionUpdateData {
    r#type: SessionActivity,
}

struct Base64UrlSafeNoPadUrl(Url);

impl<'a> FromParam<'a> for Base64UrlSafeNoPadUrl {
    type Error = &'a str;

    fn from_param(param: &'a str) -> Result<Self, Self::Error> {
        Ok(Self(base64::decode_config(param, URL_SAFE_NO_PAD)
                .ok()
                .and_then(|s| String::from_utf8(s).ok())
                .and_then(|s| s.parse().ok())
                .ok_or(param)?))
    }
}

#[get("/confirm/<attributes>/<continuation_url>/<attr_url>")]
fn confirm_oob(
    config: &State<config::Config>,
    attributes: &str,
    continuation_url: Base64UrlSafeNoPadUrl,
    attr_url: Base64UrlSafeNoPadUrl,
) -> Result<RawHtml<String>, Error> {
    let values = config.map_attributes(&serde_json::from_slice::<Vec<String>>(
        &base64::decode_config(attributes, URL_SAFE_NO_PAD)?,
    )?)?;
    let template = ConfirmTemplate {
        dologin: format!(
            "{}/browser/{}/{}/{}",
            config.server_url(),
            attributes,
            continuation_url.0,
            attr_url.0
        ),
        dologout: format!(
            "{}/cancel/{}/{}",
            config.server_url(),
            continuation_url.0,
            attr_url.0
        ),
        attributes: values,
    };
    let output = template.render()?;
    Ok(RawHtml(output))
}

#[get("/confirm/<attributes>/<continuation_url>")]
fn confirm_inline(
    config: &State<config::Config>,
    attributes: &str,
    continuation_url: Base64UrlSafeNoPadUrl,
) -> Result<RawHtml<String>, Error> {
    let values = config.map_attributes(&serde_json::from_slice::<Vec<String>>(
        &base64::decode_config(attributes, URL_SAFE_NO_PAD)?,
    )?)?;
    let template = ConfirmTemplate {
        dologin: format!(
            "{}/browser/{}/{}",
            config.server_url(),
            attributes,
            continuation_url.0
        ),
        dologout: format!("{}/cancel/{}", config.server_url(), continuation_url.0),
        attributes: values,
    };
    let output = template.render()?;
    Ok(RawHtml(output))
}

#[post("/session/update?<typedata..>")]
fn session_update(typedata: SessionUpdateData) -> Result<(), Error> {
    println!("Session update received: {:?}", typedata.r#type);
    Ok(())
}

async fn post_result(
    auth_result: &AuthResult,
    config: &State<config::Config>,
    attr_url: &Url,
) -> Result<(), Error> {
    let auth_result =
        sign_and_encrypt_auth_result(auth_result, config.signer(), config.encrypter())?;

    let client = reqwest::Client::new();
    let result = client
        .post(attr_url.to_owned())
        .header("Content-Type", "application/jwt")
        .body(auth_result.clone())
        .send()
        .await;
    if let Err(e) = result {
        // Log only
        println!("Failure reporting results: {e}");
    } else {
        println!("Reported result jwe {auth_result} to {attr_url}");
    }
    Ok(())
}

fn session_url(config: &config::Config) -> Option<Url> {
    if config.with_session() {
        Some(config.internal_url().join("session/update"))
    } else {
        None
    }
}

#[post("/browser/<attributes>/<continuation_url>/<attr_url>")]
async fn user_oob(
    config: &State<config::Config>,
    attributes: &str,
    continuation_url: Base64UrlSafeNoPadUrl,
    attr_url: Base64UrlSafeNoPadUrl,
) -> Result<Redirect, Error> {
    let attributes = base64::decode_config(attributes, URL_SAFE_NO_PAD)?;
    let attributes: Vec<String> = serde_json::from_slice(&attributes)?;
    let attributes = config.map_attributes(&attributes)?;
    let auth_result = AuthResult {
        status: AuthStatus::Success,
        attributes: Some(attributes),
        session_url: session_url(config),
    };

    post_result(&auth_result, config, &attr_url.0).await?;

    println!("Redirecting user to {}", continuation_url.0);
    Ok(Redirect::to(continuation_url.0.to_string()))
}

#[post("/cancel/<continuation_url>/<attr_url>")]
async fn cancel_oob(
    config: &State<config::Config>,
    continuation_url: &str,
    attr_url: Base64UrlSafeNoPadUrl,
) -> Result<Redirect, Error> {
    let auth_result = AuthResult {
        status: AuthStatus::Failed,
        attributes: Some(HashMap::new()),
        session_url: session_url(config),
    };

    post_result(&auth_result, config, &attr_url.0).await?;

    println!("Redirecting user to {continuation_url}");
    Ok(Redirect::to(continuation_url.to_string()))
}

fn redirect_user(
    auth_result: &AuthResult,
    config: &State<config::Config>,
    mut continuation_url: Url,
) -> Result<Redirect, Error> {
    let auth_result =
        sign_and_encrypt_auth_result(auth_result, config.signer(), config.encrypter())?;

    println!("Redirecting user to {continuation_url} with auth result {auth_result}");

    continuation_url
        .query_pairs_mut()
        .append_pair("result", &auth_result);

    Ok(Redirect::to(continuation_url.to_string()))
}

#[post("/browser/<attributes>/<continuation_url>")]
fn user_inline(
    config: &State<config::Config>,
    attributes: &str,
    continuation_url: Base64UrlSafeNoPadUrl,
) -> Result<Redirect, Error> {
    let attributes = base64::decode_config(attributes, URL_SAFE_NO_PAD)?;
    let attributes: Vec<String> = serde_json::from_slice(&attributes)?;
    let attributes = config.map_attributes(&attributes)?;
    let auth_result = AuthResult {
        status: AuthStatus::Success,
        attributes: Some(attributes),
        session_url: session_url(config),
    };

    redirect_user(&auth_result, config, continuation_url.0)
}

#[post("/cancel/<continuation_url>")]
fn cancel_inline(config: &State<config::Config>, continuation_url: Base64UrlSafeNoPadUrl) -> Result<Redirect, Error> {
    let auth_result = AuthResult {
        status: AuthStatus::Failed,
        attributes: Some(HashMap::new()),
        session_url: session_url(config),
    };

    redirect_user(&auth_result, config, continuation_url.0)
}

#[post("/start_authentication", data = "<request>")]
fn start_authentication(
    config: &State<config::Config>,
    request: Json<StartAuthRequest>,
) -> Result<Json<StartAuthResponse>, Error> {
    config.verify_attributes(&request.attributes)?;

    let attributes =
        base64::encode_config(serde_json::to_vec(&request.attributes)?, URL_SAFE_NO_PAD);
    let continuation_url =
        base64::encode_config(request.continuation_url.to_string(), URL_SAFE_NO_PAD);

    if let Some(attr_url) = &request.attr_url {
        let attr_url = base64::encode_config(attr_url.to_string(), URL_SAFE_NO_PAD);

        Ok(Json(StartAuthResponse {
            client_url: config.server_url().join(&format!(
                "/confirm/{}/{}/{}",
                attributes, continuation_url, attr_url
            )),
        }))
    } else {
        Ok(Json(StartAuthResponse {
            client_url: config
                .server_url()
                .join(&format!("/confirm/{}/{}", attributes, continuation_url)),
        }))
    }
}

#[launch]
fn rocket() -> _ {
    let base = rocket::build().mount(
        "/",
        routes![
            cancel_oob,
            cancel_inline,
            confirm_inline,
            confirm_oob,
            session_update,
            start_authentication,
            user_inline,
            user_oob,
        ],
    );

    let config = base
        .figment()
        .extract::<Config>()
        .unwrap_or_else(|e| panic!("Failure to parse configuration: {e:?}"));

    base.manage(config)
}
