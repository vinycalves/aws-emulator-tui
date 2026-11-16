//! Discovery of a running emulator: endpoint, credentials, certificate, and the
//! files the emulator writes under `~/.aws/emulator/<port>/`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::sigv4::Credentials;

/// Where the credentials came from, for display in the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialSource {
    Environment,
    EmulatorFile,
    SharedFile,
    Placeholder,
}

impl CredentialSource {
    pub fn label(&self) -> &'static str {
        match self {
            CredentialSource::Environment => "environment",
            CredentialSource::EmulatorFile => "emulator credentials",
            CredentialSource::SharedFile => "~/.aws/credentials",
            CredentialSource::Placeholder => "missing (using placeholder)",
        }
    }
}

/// Files and directories the emulator owns for one port.
#[derive(Clone, Debug)]
pub struct EmulatorPaths {
    pub port: u16,
    pub directory: PathBuf,
    pub config: PathBuf,
    pub credentials: PathBuf,
    pub ca: PathBuf,
    pub truststore: PathBuf,
}

impl EmulatorPaths {
    pub fn for_port(port: u16) -> Self {
        let directory = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".aws")
            .join("emulator")
            .join(port.to_string());
        Self {
            port,
            config: directory.join("config"),
            credentials: directory.join("emulator_credentials"),
            ca: directory.join("ca.pem"),
            truststore: directory.join("truststore.p12"),
            directory,
        }
    }
}

/// Everything needed to talk to one emulator.
#[derive(Clone, Debug)]
pub struct ConnectionConfig {
    pub endpoint: String,
    pub region: String,
    pub credentials: Credentials,
    pub credentials_source: CredentialSource,
    pub ca_pem: Option<Vec<u8>>,
    pub paths: EmulatorPaths,
    pub binary: Option<PathBuf>,
    /// Non-fatal problems encountered during discovery.
    pub warnings: Vec<String>,
}

impl ConnectionConfig {
    /// Port parsed from the endpoint URL.
    pub fn port(&self) -> u16 {
        self.paths.port
    }

    pub fn host(&self) -> String {
        self.endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string()
    }
}

/// Discover the emulator configured for this user.
///
/// Order of precedence: environment variables, then the emulator's own files
/// under `~/.aws/emulator/<port>/`, then the shared AWS config and credentials
/// files. Discovery never fails: when no credentials exist, placeholder
/// credentials are returned with a warning so the UI can start and explain the
/// problem.
pub fn discover() -> ConnectionConfig {
    let mut warnings = Vec::new();

    let mut endpoint = std::env::var("EMULATOR_ENDPOINT")
        .ok()
        .or_else(|| std::env::var("AWS_ENDPOINT_URL").ok())
        .unwrap_or_else(|| "https://127.0.0.1:9297".to_string());
    endpoint = endpoint.trim_end_matches('/').to_string();

    let port = port_from_endpoint(&endpoint).unwrap_or(9297);
    let paths = EmulatorPaths::for_port(port);

    let emulator_config = if paths.config.is_file() {
        parse_ini(&paths.config)
    } else {
        HashMap::new()
    };
    let default_section = emulator_config.get("default").cloned().unwrap_or_default();

    if let Some(url) = default_section.get("endpoint_url") {
        if std::env::var("EMULATOR_ENDPOINT").is_err() && std::env::var("AWS_ENDPOINT_URL").is_err()
        {
            endpoint = url.trim_end_matches('/').to_string();
        }
    }

    let profile = std::env::var("AWS_PROFILE")
        .or_else(|_| std::env::var("AWS_DEFAULT_PROFILE"))
        .unwrap_or_else(|_| "default".to_string());

    let (credentials, credentials_source) = if let Some(creds) = env_credentials() {
        (creds, CredentialSource::Environment)
    } else if paths.credentials.is_file() {
        match credentials_from_file(&paths.credentials, &profile) {
            Some(creds) => (creds, CredentialSource::EmulatorFile),
            None => {
                warnings.push(format!(
                    "{} exists but has no credentials for profile [{}]",
                    paths.credentials.display(),
                    profile
                ));
                (placeholder(), CredentialSource::Placeholder)
            }
        }
    } else if let Some(creds) =
        shared_credentials_path().and_then(|path| credentials_from_file(&path, &profile))
    {
        (creds, CredentialSource::SharedFile)
    } else {
        warnings.push(
            "No credentials found. Start the emulator first, or export AWS_ACCESS_KEY_ID and \
             AWS_SECRET_ACCESS_KEY."
                .to_string(),
        );
        (placeholder(), CredentialSource::Placeholder)
    };

    let region = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .ok()
        .or_else(|| default_section.get("region").cloned())
        .or_else(|| shared_region(&profile))
        .unwrap_or_else(|| "us-east-1".to_string());

    let ca_pem = fs::read(&paths.ca).ok().or_else(|| {
        default_section
            .get("ca_bundle")
            .and_then(|path| fs::read(path).ok())
    });
    if ca_pem.is_none() && endpoint.starts_with("https://") {
        warnings.push(format!(
            "No emulator CA certificate at {}. The client will accept the endpoint certificate \
             without pinning it.",
            paths.ca.display()
        ));
    }

    let binary = find_binary();

    ConnectionConfig {
        endpoint,
        region,
        credentials,
        credentials_source,
        ca_pem,
        paths,
        binary,
        warnings,
    }
}

fn placeholder() -> Credentials {
    Credentials::new("EMULATOR", "EMULATOR")
}

fn env_credentials() -> Option<Credentials> {
    let access = std::env::var("AWS_ACCESS_KEY_ID").ok()?;
    let secret = std::env::var("AWS_SECRET_ACCESS_KEY").ok()?;
    let mut credentials = Credentials::new(access, secret);
    if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
        credentials = credentials.with_session_token(token);
    }
    Some(credentials)
}

fn shared_credentials_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("AWS_SHARED_CREDENTIALS_FILE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    dirs::home_dir().map(|home| home.join(".aws").join("credentials"))
}

fn shared_region(profile: &str) -> Option<String> {
    let path = std::env::var("AWS_CONFIG_FILE")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".aws").join("config")))?;
    let sections = parse_ini(&path);
    let key = if profile == "default" {
        "default".to_string()
    } else {
        format!("profile {profile}")
    };
    sections.get(&key)?.get("region").cloned()
}

fn credentials_from_file(path: &Path, profile: &str) -> Option<Credentials> {
    let sections = parse_ini(path);
    let values = sections.get(profile).or_else(|| sections.get("default"))?;
    let access = values.get("aws_access_key_id")?.clone();
    let secret = values.get("aws_secret_access_key")?.clone();
    let mut credentials = Credentials::new(access, secret);
    if let Some(token) = values.get("aws_session_token") {
        credentials = credentials.with_session_token(token.clone());
    }
    Some(credentials)
}

/// Minimal INI reader for AWS config/credentials files. Section names are kept
/// verbatim (including `profile ` prefixes).
pub fn parse_ini(path: &Path) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return sections;
    };
    let mut current = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim().to_string();
            sections.entry(current.clone()).or_default();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    sections
}

/// `9297` from `https://127.0.0.1:9297`.
pub fn port_from_endpoint(endpoint: &str) -> Option<u16> {
    let authority = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint);
    let authority = authority.split('/').next().unwrap_or(authority);
    let (_, port) = authority.rsplit_once(':')?;
    port.parse().ok()
}

/// One emulator instance found under `~/.aws/emulator/`.
#[derive(Clone, Debug)]
pub struct DiscoveredEndpoint {
    pub port: u16,
    pub endpoint: String,
    pub region: String,
    pub directory: PathBuf,
}

/// Scan `~/.aws/emulator/*/config` for emulator instances.
pub fn discovered_endpoints() -> Vec<DiscoveredEndpoint> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let root = home.join(".aws").join("emulator");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut endpoints = Vec::new();
    for entry in entries.flatten() {
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        let Some(port) = directory
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u16>().ok())
        else {
            continue;
        };
        let config = directory.join("config");
        let sections = parse_ini(&config);
        let default = sections.get("default").cloned().unwrap_or_default();
        let endpoint = default
            .get("endpoint_url")
            .cloned()
            .unwrap_or_else(|| format!("https://127.0.0.1:{port}"));
        let region = default
            .get("region")
            .cloned()
            .unwrap_or_else(|| "us-east-1".to_string());
        endpoints.push(DiscoveredEndpoint {
            port,
            endpoint,
            region,
            directory,
        });
    }
    endpoints.sort_by_key(|endpoint| endpoint.port);
    endpoints
}

/// Locate the emulator binary.
pub fn find_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("AWS_EMULATOR_BIN") {
        let path = PathBuf::from(path);
        if is_executable(&path) {
            return Some(path);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("aws-emulator");
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".cargo").join("bin").join("aws-emulator"));
    }
    candidates.push(PathBuf::from("/opt/aws-emulator/bin/aws-emulator"));
    candidates.push(PathBuf::from("/usr/bin/aws-emulator"));
    candidates.push(PathBuf::from("/usr/local/bin/aws-emulator"));
    candidates.into_iter().find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_from_endpoint() {
        assert_eq!(port_from_endpoint("https://127.0.0.1:9297"), Some(9297));
        assert_eq!(port_from_endpoint("http://localhost:4566/"), Some(4566));
        assert_eq!(port_from_endpoint("https://127.0.0.1"), None);
    }

    #[test]
    fn parses_ini_sections() {
        let dir = std::env::temp_dir().join("emu-core-ini-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials");
        std::fs::write(
            &path,
            "[default]\naws_access_key_id = A\naws_secret_access_key = B\n\n[x]\naws_access_key_id = C\n",
        )
        .unwrap();
        let sections = parse_ini(&path);
        assert_eq!(sections["default"]["aws_access_key_id"], "A");
        assert_eq!(sections["x"]["aws_access_key_id"], "C");
        assert_eq!(sections["x"].get("aws_secret_access_key"), None);
    }
}
