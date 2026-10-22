//! Crate-wide request extractors. axum's own `Json` and `Query` answer a bad request
//! with a plain-text body; these answer with the standard `ApiError` envelope instead,
//! so every rejection carries `error.code`. Use these, never axum's `Json`, `Query` or `Path`.
use crate::error::ApiError;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A JSON body (extractor) or a JSON response.
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    axum::Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Json(v)),
            Err(r) => Err(json_rejection(r)),
        }
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// 400 `bad_json` for text that is not JSON, 422 `bad_body` for JSON of the wrong
/// shape, 415 `json_required` without a JSON content type, 413 `body_too_large` over
/// the body limit (axum's default, 2 MiB).
#[must_use]
pub fn json_rejection(r: JsonRejection) -> ApiError {
    let detail = r.body_text();
    match r {
        JsonRejection::JsonSyntaxError(_) => ApiError::bad_request(
            "bad_json",
            format!("The request body is not valid JSON. {detail}"),
        ),
        JsonRejection::JsonDataError(_) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "bad_body",
            format!("The request body is not what this call expects. {detail}"),
        ),
        JsonRejection::MissingJsonContentType(_) => ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "json_required",
            "Send the request body as JSON, with Content-Type: application/json",
        ),
        r if r.status() == StatusCode::PAYLOAD_TOO_LARGE => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_too_large",
            "The request body is too large",
        ),
        r => ApiError::new(
            r.status(),
            "bad_body",
            format!("The request body could not be read. {detail}"),
        ),
    }
}

/// A query string. Any rejection (a repeated or malformed parameter) is 400 `bad_query`.
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    axum::extract::Query<T>: FromRequestParts<S, Rejection = QueryRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(v)) => Ok(Query(v)),
            Err(r) => Err(ApiError::bad_request(
                "bad_query",
                format!(
                    "The query string is not what this call expects. {}",
                    r.body_text()
                ),
            )),
        }
    }
}

/// URL path parameters. A part that can't be read (for example `%FF`, which is not
/// UTF-8) is 400 `bad_url`; anything else is the route's own bug, so 500.
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    axum::extract::Path<T>: FromRequestParts<S, Rejection = PathRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(v)) => Ok(Path(v)),
            Err(r) if r.status() == StatusCode::BAD_REQUEST => Err(ApiError::bad_request(
                "bad_url",
                format!("A part of this address can't be read. {}", r.body_text()),
            )),
            Err(r) => Err(ApiError::internal(r.body_text())),
        }
    }
}

/// 405 for a route that exists but not with this method.
pub async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "This address does not take that kind of request",
    )
}
