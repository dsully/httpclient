use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use http::Uri;

use crate::{Error, Response, ResponseExt};
use crate::client::Client;
use crate::error::ProtocolError;
use crate::request::Request;
pub use recorder::*;

mod recorder;
mod oauth2;

pub type MiddlewareStack = Vec<Arc<dyn Middleware>>;

#[derive(Debug, Copy, Clone)]
pub struct Next<'a> {
    pub(crate) client: &'a Client,
    pub(crate) middlewares: &'a [Arc<dyn Middleware>],
}

impl Next<'_> {
    pub async fn run(self, request: Request) -> Result<Response, Error> {
        if let Some((middleware, rest)) = self.middlewares.split_first() {
            let next = Next {
                client: self.client,
                middlewares: rest,
            };
            middleware.handle(request, next).await
        } else {
            self.client.start_request(request).await
        }
    }
}

#[async_trait]
pub trait Middleware: Send + Sync + Debug {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response, Error> {
        next.run(request).await
    }
}

#[derive(Debug)]
/// Retry a request up to 3 times.
pub struct Retry;

#[async_trait]
impl Middleware for Retry {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response, Error> {
        let mut i = 0usize;
        let request = request.into_memory().await?;
        loop {
            match next.run(request.clone().into()).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    if i == 3 {
                        return Err(err);
                    }
                    i += 1;
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Logger;

fn headers_to_string(headers: &http::HeaderMap) -> String {
    headers
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
impl Middleware for Logger {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response, Error> {
        let url = request.uri().to_string();
        let method = request.method().as_str().to_uppercase();
        let version = request.version();
        let headers = headers_to_string(request.headers());
        let body = request.body();
        println!("Request:
{method} {url} HTTP/{version:?}
{headers}");
        if !body.is_empty() {
            println!("{:?}", body);
        }
        let res = next.run(request).await;
        // let version = res.v
        match res {
            Err(Error::Protocol(e)) => {
                println!("Response to {url}:\n{e}");
                Err(Error::Protocol(e))
            },
            | Ok(res)
            | Err(Error::HttpError(res)) => {
                let version = res.version();
                let status = res.status();
                let headers = headers_to_string(res.headers());
                println!("Response to {url}:
HTTP/{version:?} {status}
{headers}");
                println!("{:?}", res.body());
                res.error_for_status()
            }
        }
    }
}

#[derive(Debug, Clone)]
/// Follow redirects.
pub struct Follow;

/// Given an original Url, redirect to the new path.
fn fix_url(original: &Uri, redirect_url: &str) -> Uri {
    let url = Uri::from_str(redirect_url).unwrap();
    let mut parts = url.into_parts();
    if parts.authority.is_none() {
        parts.authority = original.authority().cloned();
    }
    if parts.scheme.is_none() {
        parts.scheme = original.scheme().cloned();
    }
    Uri::from_parts(parts).unwrap()
}

#[async_trait]
impl Middleware for Follow {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response, Error> {
        let request = request.into_memory().await?;
        let mut res = next.run(request.clone().into()).await?;
        let mut allowed_redirects = 10;
        while res.status().is_redirection() {
            if allowed_redirects == 0 {
                return Err(Error::Protocol(ProtocolError::TooManyRedirects));
            }
            let redirect = res.headers().get(http::header::LOCATION).expect("Received a 3xx status code, but no location header was sent.").to_str().unwrap();
            let url = fix_url(request.url(), redirect);
            let request = request.clone();
            let request = request.set_url(url);
            allowed_redirects -= 1;
            res = next.run(request.into()).await?;
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_route() {
        let original = Uri::from_str("https://www.google.com/").unwrap();
        let url = fix_url(&original, "/test");
        assert_eq!(url.to_string(), "https://www.google.com/test");
    }
}