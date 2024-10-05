use rocket::{
    http::Status,
    response::{Redirect, Responder},
    serde::json::Json,
    Request, Response,
};
use serde::{Deserialize, Serialize};
use url::Url;

pub type Tag = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodProperties {
    pub tag: Tag,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOptions {
    pub auth_methods: Vec<MethodProperties>,
    pub comm_methods: Vec<MethodProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartRequestAuthOnly {
    pub purpose: String,
    pub auth_method: Tag,
    pub comm_url: Url,
    pub attr_url: Option<Url>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientUrlResponse {
    pub client_url: Url,
}

impl<'r> Responder<'r, 'static> for ClientUrlResponse {
    fn respond_to(self, req: &'r Request<'_>) -> Result<Response<'static>, Status> {
        if req.headers().get_one("Accept") == Some("application/json") {
            return Some(Json(ClientUrlResponse {
                client_url: self.client_url,
            }))
            .respond_to(req);
        }

        Some(Redirect::to(self.client_url.to_string())).respond_to(req)
    }
}
