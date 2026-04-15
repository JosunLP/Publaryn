use axum::{routing::get, Router};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Publaryn API",
        version = "0.1.0",
        description = "Publaryn — secure package publishing across ecosystems",
        license(name = "Apache-2.0"),
    ),
    paths(
        // Health
        crate::routes::health::health_handler_doc,
    ),
    tags(
        (name = "health", description = "Service health and readiness"),
        (name = "auth",   description = "Authentication and session management"),
        (name = "users",  description = "User accounts"),
        (name = "orgs",   description = "Organizations and teams"),
        (name = "packages", description = "Package registry management"),
        (name = "search", description = "Package search and discovery"),
        (name = "tokens", description = "API token management"),
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().merge(
        SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", ApiDoc::openapi()),
    )
}
