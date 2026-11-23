use std::sync::LazyLock;

use semver::Version;
use serde_json::Value;

static MIRA: LazyLock<Version> =
    LazyLock::new(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo versions are semver"));

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) main: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ManifestError {
    #[error("This isn't a Mira script package. Its package.json needs a \"mira\" field.")]
    NotAPackage,
    #[error("Its package.json isn't valid: {0}.")]
    Invalid(String),
    #[error("Needs Mira {0} or newer.")]
    NeedsNewerMira(Version),
}

impl Manifest {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        Self::parse_for(bytes, &MIRA)
    }

    fn parse_for(bytes: &[u8], mira: &Version) -> Result<Self, ManifestError> {
        let invalid = |message: String| ManifestError::Invalid(message);
        let value: Value =
            serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
        let Value::Object(root) = value else {
            return Err(ManifestError::NotAPackage);
        };
        let Some(marker) = root.get("mira") else {
            return Err(ManifestError::NotAPackage);
        };
        let Value::Object(marker) = marker else {
            return Err(invalid("\"mira\" must be an object".into()));
        };
        if let Some(requires) = marker.get("requires") {
            let requires = requires
                .as_str()
                .and_then(|text| Version::parse(text).ok())
                .ok_or_else(|| invalid("\"mira.requires\" must be a version like 0.2.0".into()))?;
            if *mira < requires {
                return Err(ManifestError::NeedsNewerMira(requires));
            }
        }
        if let Some(key) = marker.keys().find(|key| *key != "requires") {
            return Err(invalid(format!("Mira doesn't support \"mira.{key}\"")));
        }
        let text = |key: &str| -> Result<Option<String>, ManifestError> {
            match root.get(key) {
                None => Ok(None),
                Some(Value::String(text)) => Ok(Some(text.trim())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)),
                Some(_) => Err(invalid(format!("\"{key}\" must be a string"))),
            }
        };
        Ok(Self {
            name: text("name")?.ok_or_else(|| invalid("\"name\" is required".into()))?,
            version: text("version")?,
            description: text("description")?,
            main: text("main")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Manifest, ManifestError> {
        Manifest::parse_for(json.as_bytes(), &Version::new(0, 2, 0))
    }

    #[test]
    fn a_package_json_is_a_package_only_with_a_mira_object() {
        assert_eq!(
            parse(
                r#"{"name":"@mira/dailies","version":"1.0.0","description":" Daily runs. ","main":"index.ts","mira":{}}"#
            ),
            Ok(Manifest {
                name: "@mira/dailies".into(),
                version: Some("1.0.0".into()),
                description: Some("Daily runs.".into()),
                main: Some("index.ts".into()),
            })
        );
        assert_eq!(
            parse(r#"{"name":"left-pad","version":"1.3.0"}"#),
            Err(ManifestError::NotAPackage)
        );
        assert_eq!(parse("[]"), Err(ManifestError::NotAPackage));
        for (json, problem) in [
            (r#"{"name":"x","mira":true}"#, "\"mira\" must be an object"),
            (r#"{"mira":{}}"#, "\"name\" is required"),
            (r#"{"name":"  ","mira":{}}"#, "\"name\" is required"),
            (
                r#"{"name":"x","version":1,"mira":{}}"#,
                "\"version\" must be a string",
            ),
            (
                r#"{"name":"x","main":["index.ts"],"mira":{}}"#,
                "\"main\" must be a string",
            ),
            (
                r#"{"name":"x","mira":{"main":"index.ts"}}"#,
                "Mira doesn't support \"mira.main\"",
            ),
            (
                r#"{"name":"x","mira":{"requires":"soon"}}"#,
                "\"mira.requires\" must be a version like 0.2.0",
            ),
        ] {
            assert_eq!(
                parse(json),
                Err(ManifestError::Invalid(problem.into())),
                "{json}"
            );
        }
        assert!(matches!(parse("{"), Err(ManifestError::Invalid(_))));
    }

    #[test]
    fn a_package_can_require_a_newer_mira() {
        assert!(parse(r#"{"name":"x","mira":{"requires":"0.2.0"}}"#).is_ok());
        assert!(parse(r#"{"name":"x","mira":{"requires":"0.1.9"}}"#).is_ok());
        assert_eq!(
            parse(r#"{"name":"x","mira":{"requires":"0.3.0","later":true}}"#),
            Err(ManifestError::NeedsNewerMira(Version::new(0, 3, 0)))
        );
    }
}
