pub use ::http::{Method, StatusCode, Uri};

pub use body::{Body, InMemoryBody};
pub use client::Client;
pub use error::{Error, InMemoryError, InMemoryResult, Result};
pub use middleware::Middleware;
pub use request::{InMemoryRequest, Request, RequestBuilder};
pub use response::{InMemoryResponse, Response};

mod client;
mod error;
pub mod request_recorder;
mod request;
mod response;
pub mod middleware;
mod body;
mod http;
mod sanitize;