pub mod compose;
pub mod docker;
pub mod make;
pub mod release;
pub mod shell;
pub mod update;

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolMode {
    Shell,
    Docker,
    Compose,
    Release,
}

static SHELL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(sh|bash|zsh)$|^scripts/").unwrap());

static DOCKERFILE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Dockerfile(\..+)?$").unwrap());

static COMPOSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(docker-)?compose[^/]*\.(yml|yaml)$").unwrap());

static RELEASE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(-darwin-|-linux-|-windows-|-amd64|-arm64|-x86_64|-aarch64|\.tar\.gz$|\.tgz$|\.zip$|\.exe$)").unwrap()
});

/// Auto-detect the tool mode from the argument (the script/file/target name).
/// Returns `None` when the argument does not match any known file pattern.
pub fn detect_mode(arg: Option<&str>) -> Option<ToolMode> {
    let arg = arg?;

    if SHELL_RE.is_match(arg) {
        Some(ToolMode::Shell)
    } else if DOCKERFILE_RE.is_match(arg) {
        Some(ToolMode::Docker)
    } else if COMPOSE_RE.is_match(arg) {
        Some(ToolMode::Compose)
    } else if RELEASE_RE.is_match(arg) {
        Some(ToolMode::Release)
    } else {
        None
    }
}


impl std::fmt::Display for ToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell => write!(f, "shell"),
            Self::Docker => write!(f, "docker"),
            Self::Compose => write!(f, "compose"),
            Self::Release => write!(f, "release"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell() {
        assert_eq!(detect_mode(Some("deploy.sh")), Some(ToolMode::Shell));
        assert_eq!(detect_mode(Some("run.bash")), Some(ToolMode::Shell));
        assert_eq!(detect_mode(Some("scripts/setup.sh")), Some(ToolMode::Shell));
        assert_eq!(detect_mode(Some("scripts/anything")), Some(ToolMode::Shell));
        assert_eq!(detect_mode(Some("DEPLOY.SH")), Some(ToolMode::Shell));
    }

    #[test]
    fn detect_docker() {
        assert_eq!(detect_mode(Some("Dockerfile")), Some(ToolMode::Docker));
        assert_eq!(detect_mode(Some("Dockerfile.prod")), Some(ToolMode::Docker));
    }

    #[test]
    fn detect_compose() {
        assert_eq!(detect_mode(Some("compose.yml")), Some(ToolMode::Compose));
        assert_eq!(detect_mode(Some("compose.yaml")), Some(ToolMode::Compose));
        assert_eq!(detect_mode(Some("docker-compose.yml")), Some(ToolMode::Compose));
        assert_eq!(
            detect_mode(Some("docker-compose.prod.yml")),
            Some(ToolMode::Compose)
        );
    }

    #[test]
    fn detect_release() {
        assert_eq!(
            detect_mode(Some("tool-darwin-arm64.tar.gz")),
            Some(ToolMode::Release)
        );
        assert_eq!(
            detect_mode(Some("tool-linux-amd64")),
            Some(ToolMode::Release)
        );
        assert_eq!(
            detect_mode(Some("app-x86_64.zip")),
            Some(ToolMode::Release)
        );
        assert_eq!(
            detect_mode(Some("thing-aarch64")),
            Some(ToolMode::Release)
        );
        assert_eq!(
            detect_mode(Some("app.exe")),
            Some(ToolMode::Release)
        );
    }

    #[test]
    fn detect_unknown_returns_none() {
        assert_eq!(detect_mode(None), None);
        assert_eq!(detect_mode(Some("build")), None);
        assert_eq!(detect_mode(Some("test")), None);
        assert_eq!(detect_mode(Some("deploy")), None);
    }

    #[test]
    fn detect_mode_keywords_return_none() {
        // Mode keywords are handled by the CLI keyword match, not detect_mode.
        assert_eq!(detect_mode(Some("sh")), None);
        assert_eq!(detect_mode(Some("make")), None);
        assert_eq!(detect_mode(Some("docker")), None);
        assert_eq!(detect_mode(Some("compose")), None);
        assert_eq!(detect_mode(Some("bin")), None);
    }

    #[test]
    fn detect_empty_string_returns_none() {
        assert_eq!(detect_mode(Some("")), None);
    }

    #[test]
    fn detect_near_misses_return_none() {
        assert_eq!(detect_mode(Some("deploy.shell")), None);  // not .sh
        assert_eq!(detect_mode(Some("Dockerfiles")), None);   // plural
        assert_eq!(detect_mode(Some("compose.json")), None);  // wrong ext
        assert_eq!(detect_mode(Some("Makefile")), None);       // not a pattern match target
        assert_eq!(detect_mode(Some("my-script")), None);      // no extension
    }

    #[test]
    fn detect_docker_case_insensitive() {
        assert_eq!(detect_mode(Some("dockerfile")), Some(ToolMode::Docker));
        assert_eq!(detect_mode(Some("DOCKERFILE")), Some(ToolMode::Docker));
        assert_eq!(detect_mode(Some("dockerfile.dev")), Some(ToolMode::Docker));
    }

    #[test]
    fn detect_release_additional_patterns() {
        assert_eq!(detect_mode(Some("tool.tgz")), Some(ToolMode::Release));
        assert_eq!(detect_mode(Some("app-windows-amd64.zip")), Some(ToolMode::Release));
    }

    #[test]
    fn detect_compose_case_insensitive() {
        assert_eq!(detect_mode(Some("COMPOSE.YML")), Some(ToolMode::Compose));
        assert_eq!(detect_mode(Some("DOCKER-COMPOSE.YAML")), Some(ToolMode::Compose));
        assert_eq!(detect_mode(Some("Compose.Yaml")), Some(ToolMode::Compose));
    }

    #[test]
    fn detect_shell_nested_scripts_path() {
        assert_eq!(detect_mode(Some("scripts/sub/deep.sh")), Some(ToolMode::Shell));
        assert_eq!(detect_mode(Some("scripts/setup")), Some(ToolMode::Shell));
    }
}
