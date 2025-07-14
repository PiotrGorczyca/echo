use super::protocol::*;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child as AsyncChild, Command as AsyncCommand};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// MCP Client for managing connections to MCP servers
#[derive(Debug, Clone)]
pub struct McpClient {
    servers: Arc<RwLock<HashMap<String, ServerConnection>>>,
    request_timeout: Duration,
}

/// Server Connection State
#[derive(Debug)]
pub struct ServerConnection {
    pub config: McpServerConfig,
    pub state: ConnectionState,
    pub capabilities: Option<ServerCapabilities>,
    pub tools: Vec<McpTool>,
    pub resources: Vec<McpResource>,
    pub prompts: Vec<McpPrompt>,
    pub transport: Box<dyn McpTransport + Send + Sync>,
}

/// Connection State
#[derive(Debug, Clone)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed(String),
}

/// MCP Transport Trait
#[async_trait::async_trait]
pub trait McpTransport: std::fmt::Debug {
    async fn send_request(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse>;
    async fn close(&mut self) -> Result<()>;
    fn is_connected(&self) -> bool;
}

/// Standard I/O Transport
#[derive(Debug)]
pub struct StdioTransport {
    child: Option<AsyncChild>,
    stdin: Option<tokio::process::ChildStdin>,
    stdout_receiver: Option<mpsc::Receiver<String>>,
    connected: bool,
}

/// WebSocket Transport
#[derive(Debug)]
pub struct WebSocketTransport {
    url: String,
    connected: bool,
}

/// HTTP Transport
#[derive(Debug)]
pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
    connected: bool,
}

impl McpClient {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            request_timeout: Duration::from_secs(30),
        }
    }

    /// Add a new MCP server configuration
    pub async fn add_server(&self, config: McpServerConfig) -> Result<()> {
        let transport = self.create_transport(&config).await?;
        
        let connection = ServerConnection {
            config: config.clone(),
            state: ConnectionState::Disconnected,
            capabilities: None,
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
            transport,
        };

        self.servers.write().await.insert(config.name.clone(), connection);
        Ok(())
    }

    /// Connect to a server
    pub async fn connect_server(&self, server_name: &str) -> Result<()> {
        let mut servers = self.servers.write().await;
        let connection = servers.get_mut(server_name)
            .ok_or_else(|| anyhow!("Server not found: {}", server_name))?;

        if !connection.config.enabled {
            return Err(anyhow!("Server is disabled: {}", server_name));
        }

        connection.state = ConnectionState::Connecting;

        match self.initialize_connection(connection).await {
            Ok(()) => {
                connection.state = ConnectionState::Connected;
                log::info!("Connected to MCP server: {}", server_name);
            }
            Err(e) => {
                connection.state = ConnectionState::Failed(e.to_string());
                return Err(e);
            }
        }

        Ok(())
    }

    /// Initialize connection with handshake
    async fn initialize_connection(&self, connection: &mut ServerConnection) -> Result<()> {
        let init_request = InitializeRequest {
            protocol_version: MCP_VERSION.to_string(),
            capabilities: ClientCapabilities {
                experimental: None,
                sampling: None,
                roots: Some(RootsCapability {
                    list_changed: true,
                }),
            },
            client_info: ClientInfo {
                name: "EchoType AI Agent".to_string(),
                version: "0.1.0".to_string(),
            },
        };

        let request = JsonRpcRequest::new(
            "initialize".to_string(),
            Some(serde_json::to_value(init_request)?),
        );

        let response = timeout(
            self.request_timeout,
            connection.transport.send_request(request),
        ).await??;

        if let Some(error) = response.error {
            return Err(anyhow!("Initialization failed: {}", error.message));
        }

        let init_response: InitializeResponse = serde_json::from_value(
            response.result.ok_or_else(|| anyhow!("No result in initialization response"))?
        )?;

        connection.capabilities = Some(init_response.capabilities);

        // Send initialized notification
        let initialized_request = JsonRpcRequest::new(
            "notifications/initialized".to_string(),
            None,
        );

        connection.transport.send_request(initialized_request).await?;

        // Load available tools, resources, and prompts
        self.load_server_capabilities(connection).await?;

        Ok(())
    }

    /// Load server capabilities (tools, resources, prompts)
    async fn load_server_capabilities(&self, connection: &mut ServerConnection) -> Result<()> {
        // Load tools
        if let Some(capabilities) = &connection.capabilities {
            if capabilities.tools.is_some() {
                let tools_request = JsonRpcRequest::new(
                    "tools/list".to_string(),
                    Some(serde_json::to_value(ListToolsRequest { cursor: None })?),
                );

                let response = connection.transport.send_request(tools_request).await?;
                if let Some(result) = response.result {
                    let tools_response: ListToolsResponse = serde_json::from_value(result)?;
                    connection.tools = tools_response.tools;
                }
            }

            // Load resources
            if capabilities.resources.is_some() {
                let resources_request = JsonRpcRequest::new(
                    "resources/list".to_string(),
                    Some(serde_json::to_value(ListResourcesRequest { cursor: None })?),
                );

                let response = connection.transport.send_request(resources_request).await?;
                if let Some(result) = response.result {
                    let resources_response: ListResourcesResponse = serde_json::from_value(result)?;
                    connection.resources = resources_response.resources;
                }
            }

            // Load prompts
            if capabilities.prompts.is_some() {
                let prompts_request = JsonRpcRequest::new(
                    "prompts/list".to_string(),
                    Some(serde_json::to_value(ListPromptsRequest { cursor: None })?),
                );

                let response = connection.transport.send_request(prompts_request).await?;
                if let Some(result) = response.result {
                    let prompts_response: ListPromptsResponse = serde_json::from_value(result)?;
                    connection.prompts = prompts_response.prompts;
                }
            }
        }

        Ok(())
    }

    /// Execute a tool on a specific server
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<CallToolResponse> {
        let servers = self.servers.read().await;
        let connection = servers.get(server_name)
            .ok_or_else(|| anyhow!("Server not found: {}", server_name))?;

        if !matches!(connection.state, ConnectionState::Connected) {
            return Err(anyhow!("Server not connected: {}", server_name));
        }

        // Check if tool exists
        if !connection.tools.iter().any(|t| t.name == tool_name) {
            return Err(anyhow!("Tool not found: {}", tool_name));
        }

        let call_request = CallToolRequest {
            name: tool_name.to_string(),
            arguments,
        };

        let request = JsonRpcRequest::new(
            "tools/call".to_string(),
            Some(serde_json::to_value(call_request)?),
        );

        // Need to drop the read lock before the async call
        drop(servers);

        let mut servers = self.servers.write().await;
        let connection = servers.get_mut(server_name)
            .ok_or_else(|| anyhow!("Server not found: {}", server_name))?;

        let response = timeout(
            self.request_timeout,
            connection.transport.send_request(request),
        ).await??;

        if let Some(error) = response.error {
            return Err(anyhow!("Tool call failed: {}", error.message));
        }

        let call_response: CallToolResponse = serde_json::from_value(
            response.result.ok_or_else(|| anyhow!("No result in tool call response"))?
        )?;

        Ok(call_response)
    }

    /// Get all available tools across all connected servers
    pub async fn get_all_tools(&self) -> HashMap<String, Vec<McpTool>> {
        let servers = self.servers.read().await;
        let mut all_tools = HashMap::new();

        for (server_name, connection) in servers.iter() {
            if matches!(connection.state, ConnectionState::Connected) {
                all_tools.insert(server_name.clone(), connection.tools.clone());
            }
        }

        all_tools
    }

    /// Get server connection status
    pub async fn get_server_status(&self, server_name: &str) -> Option<ConnectionState> {
        let servers = self.servers.read().await;
        servers.get(server_name).map(|conn| conn.state.clone())
    }

    /// Check if a server is connected
    pub async fn is_server_connected(&self, server_name: &str) -> bool {
        let connections = self.servers.read().await;
        connections.contains_key(server_name)
    }

    /// Disconnect from a server
    pub async fn disconnect_server(&self, server_name: &str) -> Result<()> {
        let mut connections = self.servers.write().await;
        if let Some(_connection) = connections.remove(server_name) {
            // Connection will be dropped automatically
            log::info!("Disconnected from MCP server: {}", server_name);
        }
        Ok(())
    }

    /// Get list of connected servers
    pub async fn get_connected_servers(&self) -> Vec<String> {
        let connections = self.servers.read().await;
        connections.keys().cloned().collect()
    }

    /// Create transport based on configuration
    async fn create_transport(&self, config: &McpServerConfig) -> Result<Box<dyn McpTransport + Send + Sync>> {
        match &config.transport {
            TransportType::Stdio => {
                Ok(Box::new(StdioTransport::new(config).await?))
            }
            TransportType::WebSocket { url } => {
                Ok(Box::new(WebSocketTransport::new(url.clone()).await?))
            }
            TransportType::Http { url } => {
                Ok(Box::new(HttpTransport::new(url.clone()).await?))
            }
        }
    }
}

impl StdioTransport {
    async fn new(config: &McpServerConfig) -> Result<Self> {
        let mut command = AsyncCommand::new(&config.command);
        command.args(&config.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        for (key, value) in &config.env {
            command.env(key, value);
        }

        let mut child = command.spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to get stdout"))?;

        let (tx, rx) = mpsc::channel(100);
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_receiver: Some(rx),
            connected: true,
        })
    }
}

#[async_trait::async_trait]
impl McpTransport for StdioTransport {
    async fn send_request(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("No stdin available"))?;
        let receiver = self.stdout_receiver.as_mut().ok_or_else(|| anyhow!("No stdout receiver"))?;

        let request_json = serde_json::to_string(&request)?;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        // Wait for response
        let response_line = timeout(Duration::from_secs(30), receiver.recv()).await?
            .ok_or_else(|| anyhow!("No response received"))?;

        let response: JsonRpcResponse = serde_json::from_str(&response_line)?;
        Ok(response)
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().await?;
        }
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

impl WebSocketTransport {
    async fn new(url: String) -> Result<Self> {
        // WebSocket implementation would go here
        // For now, just create a placeholder
        Ok(Self {
            url,
            connected: false,
        })
    }
}

#[async_trait::async_trait]
impl McpTransport for WebSocketTransport {
    async fn send_request(&mut self, _request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        // WebSocket implementation would go here
        Err(anyhow!("WebSocket transport not yet implemented"))
    }

    async fn close(&mut self) -> Result<()> {
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

impl HttpTransport {
    async fn new(url: String) -> Result<Self> {
        Ok(Self {
            url,
            client: reqwest::Client::new(),
            connected: true,
        })
    }
}

#[async_trait::async_trait]
impl McpTransport for HttpTransport {
    async fn send_request(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let response = self.client
            .post(&self.url)
            .json(&request)
            .send()
            .await?;

        let json_response: JsonRpcResponse = response.json().await?;
        Ok(json_response)
    }

    async fn close(&mut self) -> Result<()> {
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
} 