use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetupConfig {
    pub setup: SetupMeta,
    #[serde(default)]
    pub machine: Option<MachineConfig>,
    pub providers: ProvidersConfig,
    pub routing: Option<RoutingConfig>,
    pub mcp: Option<McpConfig>,
    #[serde(default)]
    pub calvin_archive: CalvinConfig,
    #[serde(default)]
    pub twilight_bark: TwilightBarkConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetupMeta {
    pub name: String,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    pub platform: String,
    pub anythingllm: Option<bool>,
    pub openclaw: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MachineConfig {
    pub name: String,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<MachineFingerprint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MachineFingerprint {
    pub platform: String,
    pub arch: String,
    pub git: bool,
    pub cargo: bool,
    pub node: bool,
    pub npm: bool,
    pub docker: bool,
    pub podman: bool,
    pub openclaw: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProvidersConfig {
    pub default: String,
    pub claude: Option<ProviderConfig>,
    pub gemini: Option<ProviderConfig>,
    pub codex: Option<ProviderConfig>,
    /// Any additional named providers (e.g. claude-opus, claude-haiku, claude-sonnet).
    /// TOML: [providers.claude-opus] type = "anthropic" model = "..." ...
    #[serde(flatten)]
    pub extras: HashMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub model: String,
    pub api_key_env: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub credential_kind: Option<String>,
    #[serde(default)]
    pub usage_rights: Option<String>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RoutingConfig {
    #[serde(default)]
    pub agents: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CalvinConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_calvin_url")]
    pub harmony_url: String,
}

fn default_calvin_url() -> String {
    "http://localhost:7171".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TwilightBarkConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub daemon_socket: Option<PathBuf>,
    #[serde(default = "default_twilight_agent_name")]
    pub agent_name: String,
    #[serde(default = "default_twilight_agent_role")]
    pub agent_role: String,
}

fn default_twilight_agent_name() -> String {
    "harkonnen-packchat".to_string()
}

fn default_twilight_agent_role() -> String {
    "packchat-bridge".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
    #[serde(rename = "self", default)]
    pub self_server: Option<McpSelfConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
    pub tool_aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpSelfConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub transport: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub auth_required: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SystemDiscovery {
    pub platform: String,
    pub arch: String,
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub git: bool,
    pub cargo: bool,
    pub node: bool,
    pub npm: bool,
    pub docker: bool,
    pub podman: bool,
    pub openclaw: bool,
}

impl SetupConfig {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading setup config: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("parsing setup config: {}", path.display()))
    }

    pub fn discover(root: &Path) -> Result<Self> {
        if let Ok(val) = std::env::var("HARKONNEN_SETUP") {
            if val.ends_with(".toml") {
                let path = Path::new(&val);
                if path.is_absolute() {
                    return Self::from_file(path);
                }
                return Self::from_file(&root.join(path));
            }
            let named = root.join("setups").join(format!("{val}.toml"));
            if named.exists() {
                return Self::from_file(&named);
            }
            anyhow::bail!(
                "HARKONNEN_SETUP={val} but {} does not exist",
                named.display()
            );
        }

        let default_path = root.join("harkonnen.toml");
        if default_path.exists() {
            return Self::from_file(&default_path);
        }

        Ok(Self::builtin_default())
    }

    fn builtin_default() -> Self {
        Self {
            setup: SetupMeta {
                name: "default".to_string(),
                template: None,
                role: None,
                organization: None,
                platform: std::env::consts::OS.to_string(),
                anythingllm: Some(false),
                openclaw: Some(false),
            },
            machine: None,
            providers: ProvidersConfig {
                default: "claude".to_string(),
                claude: Some(default_provider_config("claude")),
                gemini: None,
                codex: None,
                extras: HashMap::new(),
            },
            routing: None,
            mcp: None,
            calvin_archive: CalvinConfig::default(),
        }
    }

    pub fn resolve_provider_name(&self, name: &str) -> String {
        if name == "default" {
            self.providers.default.clone()
        } else {
            name.to_string()
        }
    }

    pub fn resolve_provider(&self, name: &str) -> Option<&ProviderConfig> {
        let resolved = self.resolve_provider_name(name);
        // Check extras (named tiers like claude-opus, claude-haiku) first,
        // then fall back to the three canonical named fields.
        if let Some(p) = self.providers.extras.get(&resolved) {
            return Some(p);
        }
        match resolved.as_str() {
            "claude" => self.providers.claude.as_ref(),
            "gemini" => self.providers.gemini.as_ref(),
            "codex" => self.providers.codex.as_ref(),
            _ => None,
        }
    }

    pub fn resolve_agent_provider_name(&self, agent_name: &str, profile_provider: &str) -> String {
        if let Some(route) = self
            .routing
            .as_ref()
            .and_then(|routing| routing.agents.get(agent_name))
        {
            self.resolve_provider_name(route)
        } else {
            self.resolve_provider_name(profile_provider)
        }
    }

    pub fn resolve_agent_provider(
        &self,
        agent_name: &str,
        profile_provider: &str,
    ) -> Option<&ProviderConfig> {
        if let Some(route) = self
            .routing
            .as_ref()
            .and_then(|routing| routing.agents.get(agent_name))
        {
            self.resolve_provider(route)
        } else {
            self.resolve_provider(profile_provider)
        }
    }
}

impl SystemDiscovery {
    pub fn discover() -> Self {
        Self {
            platform: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            hostname: detected_hostname(),
            username: detected_username(),
            git: command_available("git"),
            cargo: command_available("cargo"),
            node: command_available("node"),
            npm: command_available("npm"),
            docker: command_available("docker"),
            podman: command_available("podman"),
            openclaw: detect_openclaw(),
        }
    }

    pub fn default_role_name(&self) -> &'static str {
        match self.platform.as_str() {
            "windows" => "work",
            _ => "home",
        }
    }

    pub fn recommended_setup_name_for_role(&self, role: &str) -> &'static str {
        match role {
            "ci" | "build" | "runner" => "ci",
            "work" | "office" | "corp" | "enterprise" => {
                if self.platform == "windows" {
                    "work-windows"
                } else {
                    "home-linux"
                }
            }
            _ => match self.platform.as_str() {
                "windows" => "work-windows",
                "linux" => "home-linux",
                "macos" => "home-linux",
                _ => "home-linux",
            },
        }
    }

    pub fn recommended_template_path(&self, root: &Path, template_name: &str) -> PathBuf {
        root.join("setups").join(format!("{template_name}.toml"))
    }

    pub fn default_machine_name(&self) -> String {
        let raw = self
            .hostname
            .clone()
            .or_else(|| self.username.clone())
            .unwrap_or_else(|| format!("{}-machine", self.platform));
        slugify_machine_name(&raw)
    }

    pub fn default_write_path(&self, root: &Path, setup_id: &str) -> PathBuf {
        root.join("setups")
            .join("machines")
            .join(format!("{setup_id}.toml"))
    }

    pub fn required_tools(&self, template_name: &str) -> Vec<&'static str> {
        match template_name {
            "work-windows" => vec!["git", "cargo", "node", "npm"],
            "ci" => vec!["git", "cargo"],
            _ => vec!["git", "cargo", "node", "npm", "docker"],
        }
    }

    pub fn missing_required_tools(&self, template_name: &str) -> Vec<&'static str> {
        self.required_tools(template_name)
            .into_iter()
            .filter(|tool| match *tool {
                "git" => !self.git,
                "cargo" => !self.cargo,
                "node" => !self.node,
                "npm" => !self.npm,
                "docker" => !self.docker,
                _ => false,
            })
            .collect()
    }

    pub fn to_machine_fingerprint(&self) -> MachineFingerprint {
        MachineFingerprint {
            platform: self.platform.clone(),
            arch: self.arch.clone(),
            git: self.git,
            cargo: self.cargo,
            node: self.node,
            npm: self.npm,
            docker: self.docker,
            podman: self.podman,
            openclaw: self.openclaw,
        }
    }
}

pub fn command_available(cmd: &str) -> bool {
    let checker = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(checker)
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn detect_openclaw() -> bool {
    if command_available("openclaw") {
        return true;
    }

    let exe_name = if cfg!(windows) {
        "openclaw.exe"
    } else {
        "openclaw"
    };
    let mut candidates = Vec::new();

    if let Ok(bin_dir) = std::env::var("HARKONNEN_LOCAL_BIN") {
        candidates.push(PathBuf::from(bin_dir).join(exe_name));
    }
    if let Ok(home_dir) = std::env::var("HARKONNEN_LOCAL_HOME") {
        candidates.push(PathBuf::from(home_dir).join("bin").join(exe_name));
    }
    if let Ok(current_dir) = std::env::current_dir() {
        if let Some(parent) = current_dir.parent() {
            candidates.push(parent.join("harkonnen-local").join("bin").join(exe_name));
        }
    }

    candidates.into_iter().any(|path| path.is_file())
}

pub fn default_provider_config(name: &str) -> ProviderConfig {
    match name {
        "claude" => ProviderConfig {
            provider_type: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            enabled: true,
            credential_kind: Some("standard-api-key".to_string()),
            usage_rights: Some("standard".to_string()),
            surface: Some("claude-code".to_string()),
            base_url: None,
        },
        "gemini" => ProviderConfig {
            provider_type: "google".to_string(),
            model: "gemini-2.0-flash".to_string(),
            api_key_env: "GEMINI_API_KEY".to_string(),
            enabled: true,
            credential_kind: Some("standard-api-key".to_string()),
            usage_rights: Some("standard".to_string()),
            surface: Some("antigravity".to_string()),
            base_url: None,
        },
        "codex" => ProviderConfig {
            provider_type: "openai".to_string(),
            model: "gpt-4o".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            enabled: true,
            credential_kind: Some("standard-api-key".to_string()),
            usage_rights: Some("targeted".to_string()),
            surface: Some("vscode".to_string()),
            base_url: None,
        },
        _ => ProviderConfig {
            provider_type: "unknown".to_string(),
            model: "unknown".to_string(),
            api_key_env: "UNKNOWN_API_KEY".to_string(),
            enabled: false,
            credential_kind: None,
            usage_rights: None,
            surface: None,
            base_url: None,
        },
    }
}

pub fn available_template_names(root: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let setups_dir = root.join("setups");
    if !setups_dir.exists() {
        return Ok(names);
    }
    for entry in std::fs::read_dir(&setups_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

pub fn compose_setup_id(machine_name: &str, role: &str, organization: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(organization) = organization {
        let organization = slugify_machine_name(organization);
        if !organization.is_empty() {
            parts.push(organization);
        }
    }
    let machine_name = slugify_machine_name(machine_name);
    if !machine_name.is_empty() {
        parts.push(machine_name);
    }
    let role = slugify_machine_name(role);
    if !role.is_empty() {
        parts.push(role);
    }
    if parts.is_empty() {
        "unnamed-setup".to_string()
    } else {
        parts.join("-")
    }
}

pub fn slugify_machine_name(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !last_dash && !out.is_empty() {
                out.push(mapped);
            }
            last_dash = true;
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "unnamed-machine".to_string()
    } else {
        out
    }
}

fn detected_hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .filter(|value| !value.trim().is_empty())
}

fn detected_username() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
}
