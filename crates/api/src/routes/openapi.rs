use axum::Router;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::state::AppState;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);

        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    info(
        title = "Publaryn API",
        version = "0.1.0",
        description = "Publaryn — secure package publishing across ecosystems",
        license(name = "Apache-2.0"),
    ),
    paths(
        // Health
        crate::routes::health::health_handler_doc,
        crate::routes::health::readiness_handler_doc,
        crate::routes::stats::platform_stats_doc,
        crate::routes::admin::list_background_jobs_doc,
        crate::routes::admin::retry_background_job_doc,
        crate::routes::admin::recover_stale_background_jobs_doc,
    ),
    tags(
        (name = "health", description = "Service health and readiness"),
        (name = "stats", description = "Public platform statistics"),
        (name = "admin", description = "Platform-admin operational visibility"),
        (name = "audit", description = "Append-only audit log access"),
        (name = "auth",   description = "Authentication and session management"),
        (name = "namespaces", description = "Namespace claim management"),
        (name = "users",  description = "User accounts"),
        (name = "orgs",   description = "Organizations and teams"),
        (name = "repositories", description = "Repository and visibility management"),
        (name = "packages", description = "Package registry management"),
        (name = "search", description = "Package search and discovery"),
        (name = "security", description = "Package security findings and review"),
        (name = "tokens", description = "API token management"),
        (name = "trusted-publishing", description = "OIDC trusted publisher management"),
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}

#[cfg(test)]
mod tests {
    use super::ApiDoc;
    use serde_json::Value;
    use utoipa::OpenApi;

    fn openapi_spec() -> Value {
        serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI document should serialize")
    }

    #[test]
    fn operational_paths_are_present_in_generated_spec() {
        let spec = openapi_spec();
        let paths = spec["paths"]
            .as_object()
            .expect("OpenAPI paths should be an object");

        assert!(paths.contains_key("/health"));
        assert!(paths.contains_key("/readiness"));
        assert!(paths.contains_key("/v1/stats"));
        assert!(paths.contains_key("/v1/admin/jobs"));
        assert!(paths.contains_key("/v1/admin/jobs/{job_id}/retry"));
        assert!(paths.contains_key("/v1/admin/jobs/recover-stale"));
    }

    #[test]
    fn stats_schema_documents_stable_top_level_fields() {
        let spec = openapi_spec();
        let schema = &spec["components"]["schemas"]["PlatformStatsResponse"];
        let properties = schema["properties"]
            .as_object()
            .expect("stats schema properties should be present");
        let required = schema["required"]
            .as_array()
            .expect("stats schema should list required fields");

        for field in [
            "packages",
            "releases",
            "organizations",
            "security_findings_total",
            "security_findings_unresolved",
            "artifacts_stored",
            "job_queue_pending",
        ] {
            assert!(
                properties.contains_key(field),
                "missing stats field: {field}"
            );
            assert!(
                required.iter().any(|value| value.as_str() == Some(field)),
                "missing required stats field: {field}"
            );
        }
    }

    #[test]
    fn admin_jobs_path_documents_filters_and_bearer_auth() {
        let spec = openapi_spec();
        let get_operation = &spec["paths"]["/v1/admin/jobs"]["get"];
        let parameters = get_operation["parameters"]
            .as_array()
            .expect("admin jobs parameters should be documented");

        for expected in ["state", "kind", "page", "per_page"] {
            assert!(
                parameters
                    .iter()
                    .any(|parameter| parameter["name"].as_str() == Some(expected)),
                "missing admin jobs parameter: {expected}"
            );
        }

        let security = get_operation["security"]
            .as_array()
            .expect("admin jobs security requirement should be documented");
        assert!(security
            .iter()
            .any(|entry| entry.get("bearer_auth").is_some()));

        let scheme = &spec["components"]["securitySchemes"]["bearer_auth"];
        assert_eq!(scheme["type"], "http");
        assert_eq!(scheme["scheme"], "bearer");
        assert_eq!(scheme["bearerFormat"], "JWT");
    }
}
