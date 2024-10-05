use std::time::Duration;

use serde::Deserialize;
use verder_helpen_common::{StartCommRequest, StartCommResponse};

use super::{Method, Tag};

#[derive(Debug, Deserialize, Clone)]
pub struct CommunicationMethod {
    tag: Tag,
    name: String,
    start_url: String,
    #[serde(default = "bool::default")]
    disable_attributes_at_start: bool,
}

impl Method for CommunicationMethod {
    fn tag(&self) -> &Tag {
        &self.tag
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl CommunicationMethod {
    // Start a communication session to be composed with an authentication session
    pub async fn start(&self, purpose: &str) -> Result<StartCommResponse, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        client
            .post(&format!("{}/start_communication", &self.start_url))
            .json(&StartCommRequest {
                purpose: purpose.to_string(),
                auth_result: None,
            })
            .send()
            .await?
            .json::<StartCommResponse>()
            .await
    }

    // Falback for plugins not supporting attribute reception on startup
    async fn start_with_attributes_fallback(
        &self,
        purpose: &str,
        auth_result: &str,
    ) -> Result<StartCommResponse, reqwest::Error> {
        let comm_data = self.start(purpose).await?;

        if let Some(attr_url) = comm_data.attr_url {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()?;

            client
                .post(attr_url)
                .header("Content-Type", "application/jwt")
                .body(auth_result.to_string())
                .send()
                .await?;

            Ok(StartCommResponse {
                client_url: comm_data.client_url,
                attr_url: None,
            })
        } else {
            let mut client_url = comm_data.client_url;
            client_url
                .query_pairs_mut()
                .append_pair("result", auth_result);
            Ok(StartCommResponse {
                client_url,
                attr_url: None,
            })
        }
    }

    // Start a communication session for which we already have authentication
    // results.
    pub async fn start_with_auth_result(
        &self,
        purpose: &str,
        auth_result: &str,
    ) -> Result<StartCommResponse, reqwest::Error> {
        if self.disable_attributes_at_start {
            return self
                .start_with_attributes_fallback(purpose, auth_result)
                .await;
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        client
            .post(&format!("{}/start_communication", &self.start_url))
            .json(&StartCommRequest {
                purpose: purpose.to_string(),
                auth_result: Some(auth_result.to_string()),
            })
            .send()
            .await?
            .error_for_status()?
            .json::<StartCommResponse>()
            .await
    }
}

#[cfg(test)]
mod tests {
    use httpmock::MockServer;
    use serde_json::json;

    #[test]
    fn test_start_without_attributes_no_attrurl() {
        let server = MockServer::start();
        let start_mock = server.mock(|when, then| {
            when.path("/start_communication")
                .method(httpmock::Method::POST)
                .json_body(json!({
                    "purpose": "something"
                }));
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "client_url": "https://example.com/client_url",
                }));
        });

        let method = super::CommunicationMethod {
            tag: "test".into(),
            name: "test".into(),
            start_url: server.base_url().parse().unwrap(),
            disable_attributes_at_start: false,
        };

        let result = tokio_test::block_on(method.start("something"));

        start_mock.assert();
        let result = result.unwrap();
        assert_eq!(
            result.client_url,
            "https://example.com/client_url".parse().unwrap()
        );
        assert_eq!(result.attr_url, None);
    }

    #[test]
    fn test_start_without_attributes_attrurl() {
        let server = MockServer::start();
        let start_mock = server.mock(|when, then| {
            when.path("/start_communication")
                .method(httpmock::Method::POST)
                .json_body(json!({
                    "purpose": "something"
                }));
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "client_url": "https://example.com/client_url",
                    "attr_url": "https://example.com/attr_url",
                }));
        });

        let method = super::CommunicationMethod {
            tag: "test".into(),
            name: "test".into(),
            start_url: server.base_url().parse().unwrap(),
            disable_attributes_at_start: false,
        };

        let result = tokio_test::block_on(method.start("something"));

        start_mock.assert();
        let result = result.unwrap();
        assert_eq!(
            result.client_url,
            "https://example.com/client_url".parse().unwrap()
        );
        assert_eq!(
            result.attr_url,
            Some("https://example.com/attr_url".parse().unwrap())
        );
    }

    #[test]
    fn test_start_with_attributes() {
        let server = MockServer::start();
        let start_mock = server.mock(|when, then| {
            when.path("/start_communication")
                .method(httpmock::Method::POST)
                .json_body(json!({
                    "purpose": "something",
                    "auth_result": "test",
                }));
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "client_url": "https://example.com/client_url",
                }));
        });

        let method = super::CommunicationMethod {
            tag: "test".into(),
            name: "test".into(),
            start_url: server.base_url(),
            disable_attributes_at_start: false,
        };

        let result = tokio_test::block_on(method.start_with_auth_result("something", "test"));

        start_mock.assert();
        let result = result.unwrap();
        assert_eq!(
            result.client_url,
            "https://example.com/client_url".parse().unwrap()
        );
        assert_eq!(result.attr_url, None);
    }

    #[test]
    fn test_auth_result_fallback() {
        let server = MockServer::start();
        let start_mock = server.mock(|when, then| {
            when.path("/start_communication")
                .method(httpmock::Method::POST)
                .json_body(json!({
                    "purpose": "something",
                }));
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "client_url": "https://example.com/client_url",
                    "attr_url": format!("{}/attr_url", server.base_url()),
                }));
        });
        let auth_mock = server.mock(|when, then| {
            when.path("/attr_url")
                .method(httpmock::Method::POST)
                .header("Content-Type", "application/jwt")
                .body("test");
            then.status(200);
        });

        let method = super::CommunicationMethod {
            tag: "test".into(),
            name: "test".into(),
            start_url: server.base_url(),
            disable_attributes_at_start: true,
        };

        let result = tokio_test::block_on(method.start_with_auth_result("something", "test"));

        start_mock.assert();
        auth_mock.assert();
        let result = result.unwrap();
        assert_eq!(
            result.client_url,
            "https://example.com/client_url".parse().unwrap()
        );
        assert_eq!(result.attr_url, None);
    }

    #[test]
    fn test_auth_result_fallback_no_attr_url() {
        let server = MockServer::start();
        let start_mock = server.mock(|when, then| {
            when.path("/start_communication")
                .method(httpmock::Method::POST)
                .json_body(json!({
                    "purpose": "something",
                }));
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "client_url": "https://example.com/client_url",
                }));
        });

        let method = super::CommunicationMethod {
            tag: "test".into(),
            name: "test".into(),
            start_url: server.base_url(),
            disable_attributes_at_start: true,
        };

        let result = tokio_test::block_on(method.start_with_auth_result("something", "test"));

        start_mock.assert();
        let result = result.unwrap();
        assert_eq!(
            result.client_url,
            "https://example.com/client_url?result=test"
                .parse()
                .unwrap()
        );
        assert_eq!(result.attr_url, None);
    }
}
