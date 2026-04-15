use anyhow::{Context, Result};
use axum::{
    extract::MatchedPath,
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
        HeaderName, Method, Request,
    },
    Router,
};
use std::time::Duration;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Result<Router> {
    let cors_layer = build_cors_layer(&state).context("Invalid server CORS configuration")?;

    Ok(Router::new()
        .merge(routes::health::router())
        .merge(routes::audit::router())
        .merge(routes::auth::router())
        .merge(routes::namespaces::router())
        .merge(routes::users::router())
        .merge(routes::orgs::router())
        .merge(routes::org_invitations::router())
        .merge(routes::repositories::router())
        .merge(routes::packages::router())
        .merge(routes::search::router())
        .merge(routes::security::router())
        .merge(routes::tokens::router())
        .merge(routes::trusted_publishers::router())
        // npm registry protocol adapter
        .nest("/npm", publaryn_adapter_npm::routes::router())
        // PyPI Simple API adapter
        .nest("/pypi", publaryn_adapter_pypi::routes::router())
        // Swagger UI
        .merge(routes::openapi::router())
        .with_state(state)
        .layer(
            tower::ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(
                    TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
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
                .layer(cors_layer)
                .layer(CompressionLayer::new())
                .layer(TimeoutLayer::new(Duration::from_secs(30))),
        ))
}

fn build_cors_layer(state: &AppState) -> Result<CorsLayer> {
    let request_id_header = HeaderName::from_static("x-request-id");
    let allowed_origins = state.config.server.cors_allowed_origins()?;

    let cors_layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            ACCEPT,
            AUTHORIZATION,
            CONTENT_TYPE,
            request_id_header.clone(),
        ])
        .expose_headers([request_id_header]);

    if allowed_origins.is_empty() {
        Ok(cors_layer)
    } else {
        Ok(cors_layer.allow_origin(allowed_origins))
    }
}
