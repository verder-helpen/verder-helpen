use std::{collections::HashMap, error::Error as StdError, fmt::Display};

use askama::Template;
use base64::URL_SAFE_NO_PAD;
use config::Config;
use rocket::{
    form::FromForm,
    get, launch, post,
    response::{content::RawHtml, Redirect},
    routes,
    serde::json::Json,
    State,
};
use verder_helpen_jwt::sign_and_encrypt_auth_result;
use verder_helpen_proto::{
    AuthResult, AuthStatus, SessionActivity, StartAuthRequest, StartAuthResponse,
};

mod config;

#[derive(Debug)]
enum Error {
    Config(config::Error),
    Decode(base64::DecodeError),
    Template(askama::Error),
    Json(serde_json::Error),
    Utf(std::str::Utf8Error),
    Jwt(verder_helpen_jwt::Error),
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

impl From<verder_helpen_jwt::Error> for Error {
    fn from(e: verder_helpen_jwt::Error) -> Error {
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

#[get("/confirm/<attributes>/<continuation>/<attr_url>")]
fn confirm_oob(
    config: &State<config::Config>,
    attributes: &str,
    continuation: &str,
    attr_url: &str,
) -> Result<RawHtml<String>, Error> {
    let values = config.map_attributes(&serde_json::from_slice::<Vec<String>>(
        &base64::decode_config(attributes, URL_SAFE_NO_PAD)?,
    )?)?;
    let template = ConfirmTemplate {
        dologin: format!(
            "{}/browser/{}/{}/{}",
            config.server_url(),
            attributes,
            continuation,
            attr_url
        ),
        dologout: format!(
            "{}/cancel/{}/{}",
            config.server_url(),
            continuation,
            attr_url
        ),
        attributes: values,
    };
    let output = template.render()?;
    Ok(RawHtml(output))
}

#[get("/confirm/<attributes>/<continuation>")]
fn confirm_inline(
    config: &State<config::Config>,
    attributes: &str,
    continuation: &str,
) -> Result<RawHtml<String>, Error> {
    let values = config.map_attributes(&serde_json::from_slice::<Vec<String>>(
        &base64::decode_config(attributes, URL_SAFE_NO_PAD)?,
    )?)?;
    let template = ConfirmTemplate {
        dologin: format!(
            "{}/browser/{}/{}",
            config.server_url(),
            attributes,
            continuation
        ),
        dologout: format!("{}/cancel/{}", config.server_url(), continuation),
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
    auth_result: AuthResult,
    config: &State<config::Config>,
    attr_url: &str,
) -> Result<(), Error> {
    let attr_url = base64::decode_config(attr_url, URL_SAFE_NO_PAD)?;
    let attr_url = std::str::from_utf8(&attr_url)?;

    let auth_result =
        sign_and_encrypt_auth_result(&auth_result, config.signer(), config.encrypter())?;

    let client = reqwest::Client::new();
    let result = client
        .post(attr_url)
        .header("Content-Type", "application/jwt")
        .body(auth_result.clone())
        .send()
        .await;
    if let Err(e) = result {
        // Log only
        println!("Failure reporting results: {e}");
    } else {
        println!("Reported result jwe {} to {attr_url}", &auth_result);
    }
    Ok(())
}

fn session_url(config: &config::Config) -> Option<String> {
    if config.with_session() {
        Some(format!("{}/session/update", config.internal_url()))
    } else {
        None
    }
}

#[post("/browser/<attributes>/<continuation>/<attr_url>")]
async fn user_oob(
    config: &State<config::Config>,
    attributes: &str,
    continuation: &str,
    attr_url: &str,
) -> Result<Redirect, Error> {
    let attributes = base64::decode_config(attributes, URL_SAFE_NO_PAD)?;
    let attributes: Vec<String> = serde_json::from_slice(&attributes)?;
    let attributes = config.map_attributes(&attributes)?;
    let auth_result = AuthResult {
        status: AuthStatus::Success,
        attributes: Some(attributes),
        session_url: session_url(config),
    };

    let continuation = base64::decode_config(continuation, URL_SAFE_NO_PAD)?;
    let continuation = std::str::from_utf8(&continuation)?;

    post_result(auth_result, config, attr_url).await?;

    println!("Redirecting user to {continuation}");
    Ok(Redirect::to(continuation.to_string()))
}

#[post("/cancel/<continuation>/<attr_url>")]
async fn cancel_oob(
    config: &State<config::Config>,
    continuation: &str,
    attr_url: &str,
) -> Result<Redirect, Error> {
    let auth_result = AuthResult {
        status: AuthStatus::Failed,
        attributes: Some(HashMap::new()),
        session_url: session_url(config),
    };

    let continuation = base64::decode_config(continuation, URL_SAFE_NO_PAD)?;
    let continuation = std::str::from_utf8(&continuation)?;

    post_result(auth_result, config, attr_url).await?;

    println!("Redirecting user to {continuation}");
    Ok(Redirect::to(continuation.to_string()))
}

fn redirect_user(
    auth_result: AuthResult,
    config: &State<config::Config>,
    continuation: &str,
) -> Result<Redirect, Error> {
    let auth_result =
        sign_and_encrypt_auth_result(&auth_result, config.signer(), config.encrypter())?;
    let continuation = base64::decode_config(continuation, URL_SAFE_NO_PAD)?;
    let continuation = std::str::from_utf8(&continuation)?;

    println!(
        "Redirecting user to {continuation} with auth result {}",
        &auth_result
    );
    if continuation.contains('?') {
        Ok(Redirect::to(format!("{continuation}&result={auth_result}")))
    } else {
        Ok(Redirect::to(format!("{continuation}?result={auth_result}")))
    }
}

#[post("/browser/<attributes>/<continuation>")]
fn user_inline(
    config: &State<config::Config>,
    attributes: &str,
    continuation: &str,
) -> Result<Redirect, Error> {
    let attributes = base64::decode_config(attributes, URL_SAFE_NO_PAD)?;
    let attributes: Vec<String> = serde_json::from_slice(&attributes)?;
    let attributes = config.map_attributes(&attributes)?;
    let auth_result = AuthResult {
        status: AuthStatus::Success,
        attributes: Some(attributes),
        session_url: session_url(config),
    };

    redirect_user(auth_result, config, continuation)
}

#[post("/cancel/<continuation>")]
fn cancel_inline(config: &State<config::Config>, continuation: &str) -> Result<Redirect, Error> {
    let auth_result = AuthResult {
        status: AuthStatus::Failed,
        attributes: Some(HashMap::new()),
        session_url: session_url(config),
    };

    redirect_user(auth_result, config, continuation)
}

#[post("/start_authentication", data = "<request>")]
fn start_authentication(
    config: &State<config::Config>,
    request: Json<StartAuthRequest>,
) -> Result<Json<StartAuthResponse>, Error> {
    config.verify_attributes(&request.attributes)?;

    let attributes =
        base64::encode_config(serde_json::to_vec(&request.attributes)?, URL_SAFE_NO_PAD);
    let continuation = base64::encode_config(&request.continuation, URL_SAFE_NO_PAD);

    if let Some(attr_url) = &request.attr_url {
        let attr_url = base64::encode_config(attr_url, URL_SAFE_NO_PAD);

        Ok(Json(StartAuthResponse {
            client_url: format!(
                "{}/confirm/{}/{}/{}",
                config.server_url(),
                attributes,
                continuation,
                attr_url,
            ),
        }))
    } else {
        Ok(Json(StartAuthResponse {
            client_url: format!(
                "{}/confirm/{}/{}",
                config.server_url(),
                attributes,
                continuation,
            ),
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
