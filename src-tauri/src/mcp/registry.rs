use super::protocol::*;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tokio::sync::RwLock;
use std::sync::Arc;

/// MCP Server Registry for managing server configurations
#[derive(Debug, Clone)]
pub struct McpServerRegistry {
    servers: Arc<RwLock<HashMap<String, McpServerConfig>>>,
    config_file: PathBuf,
}

/// Built-in server configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltInServerConfig {
    pub name: String,
    pub description: String,
    pub category: String,
    pub config: McpServerConfig,
    pub dependencies: Vec<String>,
    pub install_command: Option<String>,
}

impl McpServerRegistry {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            config_file: config_path,
        }
    }

    /// Load server configurations from file
    pub async fn load_from_file(&self) -> Result<()> {
        if !self.config_file.exists() {
            // Create default configuration with built-in servers
            self.create_default_config().await?;
            return Ok(());
        }

        let content = fs::read_to_string(&self.config_file)?;
        let configs: HashMap<String, McpServerConfig> = serde_json::from_str(&content)?;

        let mut servers = self.servers.write().await;
        *servers = configs;

        Ok(())
    }

    /// Save server configurations to file
    pub async fn save_to_file(&self) -> Result<()> {
        let servers = self.servers.read().await;
        let content = serde_json::to_string_pretty(&*servers)?;
        fs::write(&self.config_file, content)?;
        Ok(())
    }

    /// Add a new server configuration
    pub async fn add_server(&self, config: McpServerConfig) -> Result<()> {
        let mut servers = self.servers.write().await;
        servers.insert(config.name.clone(), config);
        Ok(())
    }

    /// Remove a server configuration
    pub async fn remove_server(&self, name: &str) -> Result<()> {
        let mut servers = self.servers.write().await;
        servers.remove(name);
        Ok(())
    }

    /// Update a server configuration
    pub async fn update_server(&self, name: &str, config: McpServerConfig) -> Result<()> {
        let mut servers = self.servers.write().await;
        if servers.contains_key(name) {
            servers.insert(name.to_string(), config);
            Ok(())
        } else {
            Err(anyhow!("Server not found: {}", name))
        }
    }

    /// Get a server configuration
    pub async fn get_server(&self, name: &str) -> Option<McpServerConfig> {
        let servers = self.servers.read().await;
        servers.get(name).cloned()
    }

    /// Get all server configurations
    pub async fn get_all_servers(&self) -> HashMap<String, McpServerConfig> {
        let servers = self.servers.read().await;
        servers.clone()
    }

    /// Get enabled server configurations
    pub async fn get_enabled_servers(&self) -> HashMap<String, McpServerConfig> {
        let servers = self.servers.read().await;
        servers.iter()
            .filter(|(_, config)| config.enabled)
            .map(|(name, config)| (name.clone(), config.clone()))
            .collect()
    }

    /// Enable/disable a server
    pub async fn set_server_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let mut servers = self.servers.write().await;
        if let Some(config) = servers.get_mut(name) {
            config.enabled = enabled;
            Ok(())
        } else {
            Err(anyhow!("Server not found: {}", name))
        }
    }

    /// Get built-in server configurations
    pub fn get_builtin_servers() -> Vec<BuiltInServerConfig> {
        // Start with an empty list - users add their own servers
        vec![]
    }

    /// Create default configuration with built-in servers
    async fn create_default_config(&self) -> Result<()> {
        let builtin_servers = Self::get_builtin_servers();
        let mut servers = HashMap::new();

        for builtin in builtin_servers {
            servers.insert(builtin.name.clone(), builtin.config);
        }

        let mut registry_servers = self.servers.write().await;
        *registry_servers = servers;

        // Save to file
        self.save_to_file().await?;

        Ok(())
    }

    /// Install a built-in server
    pub async fn install_builtin_server(&self, name: &str) -> Result<String> {
        let builtin_servers = Self::get_builtin_servers();
        let builtin = builtin_servers.iter()
            .find(|s| s.name == name)
            .ok_or_else(|| anyhow!("Built-in server not found: {}", name))?;

        if let Some(install_command) = &builtin.install_command {
            // Execute installation command
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(install_command)
                .output()
                .await?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("Installation failed: {}", error));
            }

            // Add to registry
            self.add_server(builtin.config.clone()).await?;
            self.save_to_file().await?;

            Ok(format!("Successfully installed {}", name))
        } else {
            // Already built-in, just enable it
            self.set_server_enabled(name, true).await?;
            self.save_to_file().await?;
            Ok(format!("Enabled built-in server: {}", name))
        }
    }

    /// Check if a server is installed
    pub async fn is_server_installed(&self, name: &str) -> bool {
        let servers = self.servers.read().await;
        servers.contains_key(name)
    }

    /// Validate server configuration
    pub fn validate_config(&self, config: &McpServerConfig) -> Result<()> {
        if config.name.is_empty() {
            return Err(anyhow!("Server name cannot be empty"));
        }

        if config.command.is_empty() {
            return Err(anyhow!("Server command cannot be empty"));
        }

        match &config.transport {
            TransportType::WebSocket { url } | TransportType::Http { url } => {
                if url.is_empty() {
                    return Err(anyhow!("URL cannot be empty for WebSocket/HTTP transport"));
                }
                // Validate URL format
                url::Url::parse(url)
                    .map_err(|_| anyhow!("Invalid URL format: {}", url))?;
            }
            TransportType::Stdio => {
                // No additional validation needed for stdio
            }
        }

        Ok(())
    }

    /// Get server categories
    pub fn get_server_categories() -> Vec<String> {
        vec![
            "productivity".to_string(),
            "system".to_string(),
            "search".to_string(),
            "development".to_string(),
            "data".to_string(),
            "ai".to_string(),
            "echotype".to_string(),
        ]
    }

    /// Get servers by category
    pub fn get_builtin_servers_by_category(category: &str) -> Vec<BuiltInServerConfig> {
        Self::get_builtin_servers()
            .into_iter()
            .filter(|s| s.category == category)
            .collect()
    }

    /// Add a server configuration
    pub async fn add_server_config(&self, name: String, config: McpServerConfig) {
        let mut servers = self.servers.write().await;
        servers.insert(name, config);
    }

    /// Remove a server configuration
    pub async fn remove_server_config(&self, name: &str) {
        let mut servers = self.servers.write().await;
        servers.remove(name);
    }
} 