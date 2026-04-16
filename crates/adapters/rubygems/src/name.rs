use publaryn_core::{
    domain::namespace::Ecosystem,
    error::Result,
    validation,
};

pub fn validate_rubygems_package_name(name: &str) -> Result<()> {
    validation::validate_package_name(name, &Ecosystem::Rubygems)
}

pub fn normalize_rubygems_name(name: &str) -> String {
    name.trim().to_lowercase().replace('-', "_")
}

pub fn gem_filename(name: &str, version: &str) -> String {
    format!("{name}-{version}.gem")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hyphens_to_underscores() {
        assert_eq!(normalize_rubygems_name("My-Gem"), "my_gem");
    }

    #[test]
    fn builds_gem_filename() {
        assert_eq!(gem_filename("rails", "7.1.0"), "rails-7.1.0.gem");
    }
}
