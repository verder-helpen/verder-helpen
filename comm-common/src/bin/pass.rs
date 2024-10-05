use rocket::{
    get, post,
    response::Redirect,
    routes,
    serde::json::Json,
    tokio::sync::broadcast::{channel, Sender},
    State,
};
use serde::{Deserialize, Serialize};
use verder_helpen_comm_common::{
    config::Config,
    error::Error,
    jwt,
    session::{self, Session, SessionDBConn},
    types::{AuthSelectParams, FromPlatformJwt, GuestToken, StartRequest},
    util,
};
use verder_helpen_common::{ClientUrlResponse, StartRequestAuthOnly};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct AttributesUpdateEvent {
    pub attr_id: String,
}

#[get("/init/<guest_token>")]
fn init(guest_token: &str, config: &State<Config>) -> Result<Redirect, Error> {
    let GuestToken {
        purpose,
        redirect_url,
        ..
    } = GuestToken::from_platform_jwt(guest_token, config.auth_during_comm().guest_verifier())?;

    let auth_select_params = AuthSelectParams {
        purpose,
        start_url: config
            .external_guest_url()
            .join(&format!("start/{guest_token}")),
        cancel_url: redirect_url,
        display_name: config.auth_during_comm().display_name().to_owned(),
    };

    let auth_select_params = jwt::sign_auth_select_params(
        &auth_select_params,
        config.auth_during_comm().widget_signer(),
    )?;
    let uri = format!(
        "{}{}",
        config.auth_during_comm().widget_url(),
        auth_select_params
    );

    Ok(Redirect::to(uri))
}

#[post("/start/<guest_token>", data = "<start_request>")]
async fn start(
    guest_token: &str,
    start_request: &str,
    config: &State<Config>,
    db: SessionDBConn,
    queue: &State<Sender<AttributesUpdateEvent>>,
) -> Result<Json<ClientUrlResponse>, Error> {
    let guest_token =
        GuestToken::from_platform_jwt(guest_token, config.auth_during_comm().guest_verifier())?;
    let StartRequest {
        purpose,
        auth_method,
    } = serde_json::from_str(start_request)?;

    if purpose != guest_token.purpose {
        return Err(Error::BadRequest(
            "Purpose from start request does not match guest token purpose.".to_owned(),
        ));
    }

    let attr_id = util::random_string(64);
    let comm_url = guest_token.redirect_url.clone();
    let attr_url = config
        .internal_url() // TODO should be renamed
        .join(&format!("auth_result/{attr_id}"));
    let purpose = guest_token.purpose.clone();
    if !Session::restart_auth(guest_token.clone(), attr_id.clone(), &db).await? {
        let session = Session::new(guest_token, attr_id.clone());

        session.persist(&db).await?;
    }

    let start_request = StartRequestAuthOnly {
        purpose,
        auth_method,
        comm_url,
        attr_url: Some(attr_url),
    };

    let start_request = jwt::sign_start_auth_request(
        start_request,
        config.auth_during_comm().start_auth_key_id(),
        config.auth_during_comm().start_auth_signer(),
    )?;

    let client = reqwest::Client::new();
    let client_url_response = client
        .post(config.auth_during_comm().core_url().join("start"))
        .header(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        )
        .header(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/jwt"),
        )
        .body(start_request)
        .send()
        .await?
        .text()
        .await?;

    // may fail when there are no subscribers
    let res = queue.send(AttributesUpdateEvent { attr_id });
    match res {
        Ok(_) => println!("Update sent"),
        Err(_) => println!("Err, no update sent"),
    }

    let client_url_response = serde_json::from_str::<ClientUrlResponse>(&client_url_response)?;
    Ok(Json(client_url_response))
}

#[get("/clean_db")]
async fn clean_db(db: SessionDBConn) -> Result<(), Error> {
    session::clean_db(&db).await
}

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    let base = rocket::build()
        .manage(channel::<AttributesUpdateEvent>(1024).0)
        .mount("/internal", routes![clean_db,])
        .mount("/guest", routes![init, start,])
        .attach(SessionDBConn::fairing());

    let config = base
        .figment()
        .extract::<Config>()
        .unwrap_or_else(|e| panic!("Failure to parse configuration: {e:?}"));

    let base = base
        .manage(config)
        .ignite()
        .await
        .expect("Failed to ignite");

    let connection = SessionDBConn::get_one(&base)
        .await
        .expect("Failed to fetch database connection for periodic cleanup");
    rocket::tokio::spawn(async move {
        session::periodic_cleanup(&connection, None)
            .await
            .expect("Failed cleanup");
    });

    base.launch().await?;
    Ok(())
}
