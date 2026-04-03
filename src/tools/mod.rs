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
    Make,
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
pub fn detect_mode(arg: Option<&str>) -> ToolMode {
    let Some(arg) = arg else {
        return ToolMode::Make;
    };

    if SHELL_RE.is_match(arg) {
        ToolMode::Shell
    } else if DOCKERFILE_RE.is_match(arg) {
        ToolMode::Docker
    } else if COMPOSE_RE.is_match(arg) {
        ToolMode::Compose
    } else if RELEASE_RE.is_match(arg) {
        ToolMode::Release
    } else {
        ToolMode::Make
    }
}

impl std::fmt::Display for ToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell => write!(f, "shell"),
            Self::Docker => write!(f, "docker"),
            Self::Compose => write!(f, "compose"),
            Self::Make => write!(f, "make"),
            Self::Release => write!(f, "release"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell() {
        assert_eq!(detect_mode(Some("deploy.sh")), ToolMode::Shell);
        assert_eq!(detect_mode(Some("run.bash")), ToolMode::Shell);
        assert_eq!(detect_mode(Some("scripts/setup.sh")), ToolMode::Shell);
        assert_eq!(detect_mode(Some("scripts/anything")), ToolMode::Shell);
        assert_eq!(detect_mode(Some("DEPLOY.SH")), ToolMode::Shell);
    }

    #[test]
    fn detect_docker() {
        assert_eq!(detect_mode(Some("Dockerfile")), ToolMode::Docker);
        assert_eq!(detect_mode(Some("Dockerfile.prod")), ToolMode::Docker);
    }

    #[test]
    fn detect_compose() {
        assert_eq!(detect_mode(Some("compose.yml")), ToolMode::Compose);
        assert_eq!(detect_mode(Some("compose.yaml")), ToolMode::Compose);
        assert_eq!(detect_mode(Some("docker-compose.yml")), ToolMode::Compose);
        assert_eq!(
            detect_mode(Some("docker-compose.prod.yml")),
            ToolMode::Compose
        );
    }

    #[test]
    fn detect_release() {
        assert_eq!(
            detect_mode(Some("tool-darwin-arm64.tar.gz")),
            ToolMode::Release
        );
        assert_eq!(
            detect_mode(Some("tool-linux-amd64")),
            ToolMode::Release
        );
        assert_eq!(
            detect_mode(Some("app-x86_64.zip")),
            ToolMode::Release
        );
        assert_eq!(
            detect_mode(Some("thing-aarch64")),
            ToolMode::Release
        );
        assert_eq!(
            detect_mode(Some("app.exe")),
            ToolMode::Release
        );
    }

    #[test]
    fn detect_make_default() {
        assert_eq!(detect_mode(None), ToolMode::Make);
        assert_eq!(detect_mode(Some("build")), ToolMode::Make);
        assert_eq!(detect_mode(Some("test")), ToolMode::Make);
        assert_eq!(detect_mode(Some("deploy")), ToolMode::Make);
    }
}
