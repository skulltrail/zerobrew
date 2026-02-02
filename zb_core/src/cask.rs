use serde::Deserialize;
use std::collections::BTreeMap;

/// Represents a Homebrew Cask (typically macOS GUI applications)
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Cask {
    /// The cask token (identifier), e.g., "visual-studio-code"
    pub token: String,
    /// Display names for the cask
    #[serde(default)]
    pub name: Vec<String>,
    /// Version of the cask
    pub version: String,
    /// Download URL
    pub url: String,
    /// SHA256 checksum (may be "no_check" for some casks)
    #[serde(default)]
    pub sha256: Option<String>,
    /// Artifacts to install (apps, pkgs, binaries, etc.)
    #[serde(default)]
    pub artifacts: Vec<CaskArtifact>,
    /// Dependencies
    #[serde(default)]
    pub depends_on: CaskDependencies,
    /// Optional caveats/notes
    #[serde(default)]
    pub caveats: Option<String>,
    /// Homepage URL
    #[serde(default)]
    pub homepage: Option<String>,
    /// Description
    #[serde(default)]
    pub desc: Option<String>,
}

impl Cask {
    /// Returns the primary display name or the token
    pub fn display_name(&self) -> &str {
        self.name.first().map(|s| s.as_str()).unwrap_or(&self.token)
    }

    /// Returns app artifacts (the .app bundles to install)
    pub fn app_artifacts(&self) -> Vec<&str> {
        self.artifacts
            .iter()
            .filter_map(|a| match a {
                CaskArtifact::App(apps) => Some(apps.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Returns pkg artifacts (installer packages)
    pub fn pkg_artifacts(&self) -> Vec<&str> {
        self.artifacts
            .iter()
            .filter_map(|a| match a {
                CaskArtifact::Pkg(pkgs) => Some(pkgs.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Returns binary artifacts (symlinks to create in /usr/local/bin)
    pub fn binary_artifacts(&self) -> Vec<&str> {
        self.artifacts
            .iter()
            .filter_map(|a| match a {
                CaskArtifact::Binary(bins) => {
                    Some(bins.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect()
    }
}

/// Cask artifacts define what gets installed
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaskArtifact {
    /// Application bundles (.app)
    App(Vec<String>),
    /// Installer packages (.pkg)
    Pkg(Vec<String>),
    /// Binary symlinks
    Binary(Vec<String>),
    /// Zap (cleanup) definitions
    Zap(Vec<serde_json::Value>),
    /// Uninstall definitions
    Uninstall(Vec<serde_json::Value>),
    /// Other artifact types we capture but don't process
    #[serde(other)]
    Other,
}

/// Dependencies a cask may have
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct CaskDependencies {
    /// Required formulas
    #[serde(default)]
    pub formula: Vec<String>,
    /// Required casks
    #[serde(default)]
    pub cask: Vec<String>,
    /// macOS version requirements
    #[serde(default)]
    pub macos: Option<MacOsRequirement>,
}

/// macOS version requirements
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MacOsRequirement {
    /// Minimum version string (e.g., ">= :monterey")
    Version(String),
    /// Specific versions map
    Versions(BTreeMap<String, String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CASK: &str = r#"{
        "token": "visual-studio-code",
        "name": ["Microsoft Visual Studio Code", "VS Code"],
        "version": "1.85.0",
        "url": "https://update.code.visualstudio.com/1.85.0/darwin-arm64/stable",
        "sha256": "abc123def456",
        "homepage": "https://code.visualstudio.com/",
        "desc": "Open-source code editor",
        "artifacts": [
            {"app": ["Visual Studio Code.app"]},
            {"binary": ["Visual Studio Code.app/Contents/Resources/app/bin/code"]}
        ],
        "depends_on": {
            "macos": ">= :monterey"
        }
    }"#;

    #[test]
    fn deserialize_cask() {
        let cask: Cask = serde_json::from_str(SAMPLE_CASK).unwrap();
        assert_eq!(cask.token, "visual-studio-code");
        assert_eq!(cask.version, "1.85.0");
        assert_eq!(cask.display_name(), "Microsoft Visual Studio Code");
    }

    #[test]
    fn cask_with_minimal_fields() {
        let json = r#"{
            "token": "my-app",
            "version": "1.0.0",
            "url": "https://example.com/app.dmg"
        }"#;
        let cask: Cask = serde_json::from_str(json).unwrap();
        assert_eq!(cask.token, "my-app");
        assert_eq!(cask.version, "1.0.0");
        assert!(cask.artifacts.is_empty());
    }

    #[test]
    fn display_name_falls_back_to_token() {
        let json = r#"{
            "token": "my-app",
            "version": "1.0.0",
            "url": "https://example.com/app.dmg"
        }"#;
        let cask: Cask = serde_json::from_str(json).unwrap();
        assert_eq!(cask.display_name(), "my-app");
    }
}
