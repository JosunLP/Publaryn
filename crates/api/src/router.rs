use anyhow::{Context, Result};
use axum::{
    extract::MatchedPath,
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
        HeaderName, Method, Request, StatusCode,
    },
    middleware, Router,
};
use std::time::Duration;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, SetRequestIdLayer},
    services::{ServeDir, ServeFile},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::rate_limit;
use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Result<Router> {
    let cors_layer = build_cors_layer(&state).context("Invalid server CORS configuration")?;
    let static_dir = state.config.server.static_dir.clone();

    let api_router = Router::new()
        .merge(routes::health::router())
        .merge(routes::admin::router())
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
        .merge(routes::pypi_oidc::router())
        // npm registry protocol adapter
        .nest("/npm", publaryn_adapter_npm::routes::router())
        // PyPI Simple API adapter
        .nest("/pypi", publaryn_adapter_pypi::routes::router())
        // Composer / Packagist-style metadata adapter
        .nest("/composer", publaryn_adapter_composer::routes::router())
        // RubyGems metadata and gem download adapter
        .nest("/rubygems", publaryn_adapter_rubygems::routes::router())
        // Maven repository adapter
        .nest("/maven", publaryn_adapter_maven::routes::router())
        // Cargo alternative registry adapter
        .nest(
            "/cargo/index",
            publaryn_adapter_cargo_registry::routes::index_router(),
        )
        .nest(
            "/cargo/api/v1",
            publaryn_adapter_cargo_registry::routes::api_router(),
        )
        // NuGet V3 protocol adapter
        .nest("/nuget", publaryn_adapter_nuget::routes::router())
        // OCI distribution-spec adapter
        .nest("/oci", publaryn_adapter_oci::routes::router())
        // Swagger UI
        .merge(routes::openapi::router())
        // Platform statistics
        .merge(routes::stats::router())
        .with_state(state.clone());

    // Serve frontend SPA from static directory when configured.
    // Unknown paths fall back to index.html for client-side routing.
    let app = if let Some(ref dir) = static_dir {
        let index_path = format!("{}/index.html", dir);
        let serve_dir = ServeDir::new(dir).not_found_service(ServeFile::new(&index_path));
        api_router.fallback_service(serve_dir)
    } else {
        api_router
    };

    Ok(app.layer(
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
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(30),
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                rate_limit::rate_limit_middleware,
            )),
    ))
}

fn build_cors_layer(state: &AppState) -> Result<CorsLayer> {
    let request_id_header = HeaderName::from_static("x-request-id");
    let nuget_api_key_header = HeaderName::from_static("x-nuget-apikey");
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
            nuget_api_key_header,
        ])
        .expose_headers([request_id_header]);

    if allowed_origins.is_empty() {
        Ok(cors_layer)
    } else {
        Ok(cors_layer.allow_origin(allowed_origins))
    }
}
