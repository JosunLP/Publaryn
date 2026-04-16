use publaryn_core::{
    domain::namespace::Ecosystem,
    error::{Error, Result},
    validation,
};

pub fn validate_maven_package_name(name: &str) -> Result<()> {
    validation::validate_package_name(name, &Ecosystem::Maven)
}

pub fn package_name(group_id: &str, artifact_id: &str) -> Result<String> {
    let name = format!("{group_id}:{artifact_id}");
    validate_maven_package_name(&name)?;
    Ok(name)
}

pub fn parse_metadata_path(path: &str) -> Result<(String, String)> {
    let segments = split_path(path);
    if segments.len() < 3 || segments.last() != Some(&"maven-metadata.xml") {
        return Err(Error::Validation("Invalid Maven metadata path".into()));
    }

    let artifact_id = segments[segments.len() - 2];
    let group_id = segments[..segments.len() - 2].join(".");
    if group_id.is_empty() {
        return Err(Error::Validation("Maven metadata path is missing a groupId".into()));
    }

    Ok((group_id, artifact_id.to_owned()))
}

pub fn parse_artifact_path(path: &str) -> Result<(String, String, String, String)> {
    let segments = split_path(path);
    if segments.len() < 4 {
        return Err(Error::Validation("Invalid Maven artifact path".into()));
    }

    let filename = segments.last().unwrap().to_string();
    let version = segments[segments.len() - 2].to_string();
    let artifact_id = segments[segments.len() - 3].to_string();
    let group_id = segments[..segments.len() - 3].join(".");

    if group_id.is_empty() {
        return Err(Error::Validation("Maven artifact path is missing a groupId".into()));
    }

    Ok((group_id, artifact_id, version, filename))
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metadata_path() {
        let (group, artifact) = parse_metadata_path("com/example/demo/maven-metadata.xml").unwrap();
        assert_eq!(group, "com.example");
        assert_eq!(artifact, "demo");
    }

    #[test]
    fn parses_artifact_path() {
        let (group, artifact, version, filename) =
            parse_artifact_path("com/example/demo/1.0.0/demo-1.0.0.jar").unwrap();
        assert_eq!(group, "com.example");
        assert_eq!(artifact, "demo");
        assert_eq!(version, "1.0.0");
        assert_eq!(filename, "demo-1.0.0.jar");
    }
}
