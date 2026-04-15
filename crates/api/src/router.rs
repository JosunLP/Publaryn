use axum::{
    extract::MatchedPath,
    http::Request,
    middleware,
    Router,
};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use std::time::Duration;

use crate::state::AppState;
use crate::routes;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::router())
        .merge(routes::auth::router())
        .merge(routes::users::router())
        .merge(routes::orgs::router())
        .merge(routes::packages::router())
        .merge(routes::search::router())
        .merge(routes::tokens::router())
        // Swagger UI
        .merge(routes::openapi::router())
        .with_state(state)
        .layer(
            tower::ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|req: &Request<_>| {
                            let path = req
                                .extensions()
                                .get::<MatchedPath>()
                                .map(|p| p.as_str())
                                .unwrap_or(req.uri().path());
                            tracing::info_span!(
                                "http_request",
                                method = %req.method(),
                                path,
                            )
                        }),
                )
                .layer(CorsLayer::permissive())
                .layer(CompressionLayer::new())
                .layer(TimeoutLayer::new(Duration::from_secs(30))),
        )
}
