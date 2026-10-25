//! [`HttpClient`] over reqwest.

use std::time::Duration;

use crate::providers::{BoxFuture, HttpClient, HttpResponse};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// reqwest drops the `Authorization` header on cross-host redirects, so a key never leaks to
/// another domain.
impl HttpClient for reqwest::Client {
    fn get<'a>(
        &'a self,
        url: &'a str,
        headers: Vec<(&'static str, String)>,
    ) -> BoxFuture<'a, Result<HttpResponse, String>> {
        Box::pin(async move {
            let mut request = reqwest::Client::get(self, url).timeout(REQUEST_TIMEOUT);
            for (name, value) in headers {
                request = request.header(name, value);
            }
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status().as_u16();
            let body = response.bytes().await.map_err(|e| e.to_string())?;
            Ok(HttpResponse {
                status,
                body: body.to_vec(),
            })
        })
    }
}
