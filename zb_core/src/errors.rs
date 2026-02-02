use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedBottle { name: String },
    ChecksumMismatch { expected: String, actual: String },
    LinkConflict { path: PathBuf },
    StoreCorruption { message: String },
    NetworkFailure { message: String },
    MissingFormula { name: String },
    MissingCask { name: String },
    UnsupportedTap { name: String },
    DependencyCycle { cycle: Vec<String> },
    NotInstalled { name: String },
    CaskNotInstalled { name: String },
    FileError { message: String },
    InvalidArgument { message: String },
    ExecutionError { message: String },
    CaskError { message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnsupportedBottle { name } => {
                write!(f, "unsupported bottle for formula '{name}'")
            }
            Error::ChecksumMismatch { expected, actual } => {
                write!(f, "checksum mismatch (expected {expected}, got {actual})")
            }
            Error::LinkConflict { path } => {
                write!(f, "link conflict at '{}'", path.to_string_lossy())
            }
            Error::StoreCorruption { message } => write!(f, "store corruption: {message}"),
            Error::NetworkFailure { message } => write!(f, "network failure: {message}"),
            Error::MissingFormula { name } => write!(f, "missing formula '{name}'"),
            Error::MissingCask { name } => write!(f, "missing cask '{name}'"),
            Error::UnsupportedTap { name } => {
                write!(
                    f,
                    "tap formula '{name}' is not supported (only homebrew/core)"
                )
            }
            Error::DependencyCycle { cycle } => {
                let rendered = cycle.join(" -> ");
                write!(f, "dependency cycle detected: {rendered}")
            }
            Error::NotInstalled { name } => write!(f, "formula '{name}' is not installed"),
            Error::CaskNotInstalled { name } => write!(f, "cask '{name}' is not installed"),
            Error::FileError { message } => write!(f, "file error: {message}"),
            Error::InvalidArgument { message } => write!(f, "invalid argument: {message}"),
            Error::ExecutionError { message } => write!(f, "{message}"),
            Error::CaskError { message } => write!(f, "cask error: {message}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_bottle_display_includes_name() {
        let err = Error::UnsupportedBottle {
            name: "libheif".to_string(),
        };

        assert!(err.to_string().contains("libheif"));
    }

    #[test]
    fn checksum_mismatch_shows_both_values() {
        let err = Error::ChecksumMismatch {
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("abc123"));
        assert!(msg.contains("def456"));
        assert!(msg.contains("mismatch"));
    }

    #[test]
    fn link_conflict_shows_path() {
        let err = Error::LinkConflict {
            path: std::path::PathBuf::from("/usr/local/bin/tool"),
        };

        assert!(err.to_string().contains("/usr/local/bin/tool"));
        assert!(err.to_string().contains("conflict"));
    }

    #[test]
    fn missing_formula_error() {
        let err = Error::MissingFormula {
            name: "nonexistent-pkg".to_string(),
        };

        assert!(err.to_string().contains("nonexistent-pkg"));
        assert!(err.to_string().contains("missing formula"));
    }

    #[test]
    fn missing_cask_error() {
        let err = Error::MissingCask {
            name: "nonexistent-app".to_string(),
        };

        assert!(err.to_string().contains("nonexistent-app"));
        assert!(err.to_string().contains("missing cask"));
    }

    #[test]
    fn not_installed_error() {
        let err = Error::NotInstalled {
            name: "not-here".to_string(),
        };

        assert!(err.to_string().contains("not-here"));
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn cask_not_installed_error() {
        let err = Error::CaskNotInstalled {
            name: "missing-app".to_string(),
        };

        assert!(err.to_string().contains("missing-app"));
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn dependency_cycle_shows_cycle() {
        let err = Error::DependencyCycle {
            cycle: vec!["a".to_string(), "b".to_string(), "c".to_string(), "a".to_string()],
        };

        let msg = err.to_string();
        assert!(msg.contains("a -> b -> c -> a"));
        assert!(msg.contains("cycle"));
    }

    #[test]
    fn unsupported_tap_error() {
        let err = Error::UnsupportedTap {
            name: "third-party/tap/formula".to_string(),
        };

        assert!(err.to_string().contains("third-party/tap/formula"));
        assert!(err.to_string().contains("not supported"));
        assert!(err.to_string().contains("homebrew/core"));
    }

    #[test]
    fn store_corruption_error() {
        let err = Error::StoreCorruption {
            message: "database corrupted".to_string(),
        };

        assert!(err.to_string().contains("database corrupted"));
        assert!(err.to_string().contains("corruption"));
    }

    #[test]
    fn network_failure_error() {
        let err = Error::NetworkFailure {
            message: "connection timeout".to_string(),
        };

        assert!(err.to_string().contains("connection timeout"));
        assert!(err.to_string().contains("network failure"));
    }

    #[test]
    fn file_error() {
        let err = Error::FileError {
            message: "permission denied".to_string(),
        };

        assert!(err.to_string().contains("permission denied"));
        assert!(err.to_string().contains("file error"));
    }

    #[test]
    fn invalid_argument_error() {
        let err = Error::InvalidArgument {
            message: "bad parameter value".to_string(),
        };

        assert!(err.to_string().contains("bad parameter value"));
        assert!(err.to_string().contains("invalid argument"));
    }

    #[test]
    fn cask_error() {
        let err = Error::CaskError {
            message: "failed to extract DMG".to_string(),
        };

        assert!(err.to_string().contains("failed to extract DMG"));
        assert!(err.to_string().contains("cask error"));
    }

    #[test]
    fn execution_error() {
        let err = Error::ExecutionError {
            message: "command failed".to_string(),
        };

        assert!(err.to_string().contains("command failed"));
        // ExecutionError shows message directly without prefix
    }

    #[test]
    fn error_implements_std_error() {
        let err = Error::NotInstalled {
            name: "test".to_string(),
        };

        // Should implement std::error::Error trait
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn error_is_cloneable() {
        let err1 = Error::MissingFormula {
            name: "test".to_string(),
        };
        let err2 = err1.clone();

        assert_eq!(err1, err2);
    }
}