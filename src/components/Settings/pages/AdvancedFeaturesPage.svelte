<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { hasUnsavedChanges = $bindable(false) } = $props();

  // Types
  interface McpServer {
    name: string;
    display_name: string;
    description: string;
    enabled: boolean;
    auto_connect: boolean;
    config: {
      command: string;
      args: string[];
      env: Record<string, string>;
      transport: "stdio" | "websocket" | "http";
    };
    voice_commands: VoiceCommand[];
    status?: "connected" | "disconnected" | "connecting" | "error";
    error_message?: string;
  }

  interface VoiceCommand {
    trigger_phrases: string[];
    tool_name: string;
    parameter_mapping: Record<string, string>;
    description: string;
    examples: string[];
  }

  interface McpTool {
    name: string;
    description: string;
    server: string;
    parameters: Record<string, any>;
  }

  // State
  let mcpServers: McpServer[] = $state([]);
  let availableTools: Record<string, McpTool[]> = $state({});
  let selectedServer: McpServer | null = $state(null);
  let isAddingServer: boolean = $state(false);
  let isLoadingTools: boolean = $state(false);
  let error: string = $state("");
  let success: string = $state("");
  let installingServers: string[] = $state([]);
  let showAddServer: boolean = $state(false);
  let packageToInstall: string = $state("");
  let installingPackage: boolean = $state(false);
  let packageOrScript: string = $state("");

  // New server form
  let newServer: McpServer = $state({
    name: "",
    display_name: "",
    description: "",
    enabled: true,
    auto_connect: false,
    config: {
      command: "node",
      args: [],
      env: {},
      transport: "stdio"
    },
    voice_commands: []
  });

  // Voice command form
  let editingVoiceCommand: VoiceCommand | null = $state(null);
  let newVoiceCommand: VoiceCommand = $state({
    trigger_phrases: [],
    tool_name: "",
    parameter_mapping: {},
    description: "",
    examples: []
  });

  // JSON Editor state
  let mcpConfigJson: string = $state("");
  let jsonError: string = $state("");
  let jsonValid: boolean = $state(false);
  let installing: boolean = $state(false);
  let installationLogs: Array<{type: string, message: string}> = $state([]);
  
  const jsonPlaceholder = `{
  "mcpServers": {
    "clickup": {
      "command": "bunx",
      "args": ["@nazruden/clickup-mcp-server"],
      "env": {
        "CLICKUP_PERSONAL_TOKEN": "your_personal_token_here"
      }
    }
  }
}`;

  // Template configurations for popular MCP servers
  const templates = [
    {
      name: "ClickUp (@nazruden)",
      description: "Task and project management",
      config: {
        clickup: {
          command: "bunx",
          args: ["@nazruden/clickup-mcp-server"],
          env: {
            CLICKUP_PERSONAL_TOKEN: "your-clickup-personal-token"
          }
        }
      }
    },
    {
      name: "ClickUp (@taazkareem)", 
      description: "Alternative ClickUp server",
      config: {
        clickup: {
          command: "bunx", 
          args: ["@taazkareem/clickup-mcp-server"],
          env: {
            CLICKUP_API_TOKEN: "your-clickup-api-token"
          }
        }
      }
    },
    {
      name: "GitHub",
      description: "GitHub repository management",
      config: {
        github: {
          command: "bunx",
          args: ["@modelcontextprotocol/server-github"],
          env: {
            GITHUB_PERSONAL_ACCESS_TOKEN: "your-github-token"
          }
        }
      }
    },
    {
      name: "Filesystem",
      description: "Local file operations",
      config: {
        filesystem: {
          command: "bunx",
          args: ["@modelcontextprotocol/server-filesystem"],
          env: {}
        }
      }
    },
    {
      name: "Custom Server",
      description: "Custom configuration",
      config: {
        "custom-server": {
          command: "bunx",
          args: ["your-mcp-package"],
          env: {
            API_KEY: "your-api-key"
          }
        }
      }
    }
  ];

  onMount(async () => {
    await loadMcpServers();
    await loadAvailableTools();
    loadMcpServersIntoJson();
  });

  async function loadMcpServers() {
    try {
      mcpServers = await invoke<McpServer[]>("get_mcp_servers");
    } catch (err) {
      error = `Failed to load MCP servers: ${err}`;
    }
  }

  async function loadAvailableTools() {
    try {
      isLoadingTools = true;
      availableTools = await invoke<Record<string, McpTool[]>>("get_available_mcp_tools");
    } catch (err) {
      console.error("Failed to load available tools:", err);
    } finally {
      isLoadingTools = false;
    }
  }

  async function saveMcpServers() {
    try {
      await invoke("save_mcp_servers", { servers: mcpServers });
      success = "MCP servers saved successfully!";
      hasUnsavedChanges = false;
      setTimeout(() => success = "", 3000);
    } catch (err) {
      error = `Failed to save MCP servers: ${err}`;
    }
  }

  async function connectServer(serverName: string) {
    try {
      await invoke("connect_mcp_server", { serverName });
      await loadMcpServers(); // Refresh status
      await loadAvailableTools(); // Refresh tools
    } catch (err) {
      error = `Failed to connect to server ${serverName}: ${err}`;
    }
  }

  async function disconnectServer(serverName: string) {
    try {
      await invoke("disconnect_mcp_server", { serverName });
      await loadMcpServers(); // Refresh status
    } catch (err) {
      error = `Failed to disconnect from server ${serverName}: ${err}`;
    }
  }

  function addServer() {
    isAddingServer = true;
    newServer = {
      name: "",
      display_name: "",
      description: "",
      enabled: true,
      auto_connect: false,
      config: {
        command: "node",
        args: [],
        env: {},
        transport: "stdio"
      },
      voice_commands: []
    };
  }

  function saveNewServer() {
    if (!newServer.name || !newServer.display_name) {
      error = "Name and display name are required";
      return;
    }

    mcpServers = [...mcpServers, { ...newServer }];
    isAddingServer = false;
    hasUnsavedChanges = true;
    success = "Server added! Remember to save your changes.";
    setTimeout(() => success = "", 3000);
  }

  function cancelAddServer() {
    newServer = {
      name: "",
      display_name: "",
      description: "",
      enabled: true,
      auto_connect: false,
      config: {
        command: "node",
        args: [],
        env: {},
        transport: "stdio"
      },
      voice_commands: []
    };
    showAddServer = false;
    packageToInstall = "";
    packageOrScript = "";
  }

  function removeServer(serverName: string) {
    mcpServers = mcpServers.filter(s => s.name !== serverName);
    hasUnsavedChanges = true;
  }

  function editServer(server: McpServer) {
    selectedServer = { ...server };
  }

  function saveEditedServer() {
    if (!selectedServer) return;

    const index = mcpServers.findIndex(s => s.name === selectedServer.name);
    if (index !== -1) {
      mcpServers[index] = { ...selectedServer };
      hasUnsavedChanges = true;
    }
    selectedServer = null;
  }

  function loadTemplate(templateName: string) {
    const templates = {
      clickup: {
        name: "clickup",
        display_name: "ClickUp (@nazruden)",
        description: "Task and project management - Uses CLICKUP_PERSONAL_TOKEN",
        config: {
          command: "bunx",
          args: ["@nazruden/clickup-mcp-server"],
          env: {
            "CLICKUP_PERSONAL_TOKEN": "your_personal_token_here"
          },
          transport: "stdio" as const
        }
      },
      github: {
        name: "github",
        display_name: "GitHub",
        description: "Code repository management",
        config: {
          command: "bunx",
          args: ["@modelcontextprotocol/server-github"],
          env: {
            "GITHUB_TOKEN": "your_token_here"
          },
          transport: "stdio" as const
        }
      },
      filesystem: {
        name: "filesystem",
        display_name: "File System",
        description: "Local file operations",
        config: {
          command: "bunx",
          args: ["@modelcontextprotocol/server-filesystem"],
          env: {},
          transport: "stdio" as const
        }
      },
      notion: {
        name: "notion",
        display_name: "Notion",
        description: "Notion workspace integration",
        config: {
          command: "bunx",
          args: ["@notionhq/notion-mcp-server"],
          env: {
            "NOTION_API_TOKEN": "your_token_here"
          },
          transport: "stdio" as const
        }
      }
    };

    const template = templates[templateName as keyof typeof templates];
    if (template) {
      newServer = {
        ...template,
        enabled: true,
        auto_connect: false,
        voice_commands: []
      };
    }
  }

  function deleteServer(serverName: string) {
    if (confirm(`Are you sure you want to delete server "${serverName}"?`)) {
      mcpServers = mcpServers.filter(s => s.name !== serverName);
      hasUnsavedChanges = true;
    }
  }

  function addVoiceCommand(server: McpServer) {
    editingVoiceCommand = {
      trigger_phrases: [],
      tool_name: "",
      parameter_mapping: {},
      description: "",
      examples: []
    };
  }

  function saveVoiceCommand(server: McpServer) {
    if (!editingVoiceCommand) return;

    const serverIndex = mcpServers.findIndex(s => s.name === server.name);
    if (serverIndex !== -1) {
      mcpServers[serverIndex].voice_commands.push({ ...editingVoiceCommand });
      hasUnsavedChanges = true;
    }
    editingVoiceCommand = null;
  }

  function deleteVoiceCommand(server: McpServer, commandIndex: number) {
    const serverIndex = mcpServers.findIndex(s => s.name === server.name);
    if (serverIndex !== -1) {
      mcpServers[serverIndex].voice_commands.splice(commandIndex, 1);
      hasUnsavedChanges = true;
    }
  }

  function addTriggerPhrase(phrases: string[], newPhrase: string) {
    if (newPhrase.trim()) {
      phrases.push(newPhrase.trim());
      hasUnsavedChanges = true;
    }
  }

  function removeTriggerPhrase(phrases: string[], index: number) {
    phrases.splice(index, 1);
    hasUnsavedChanges = true;
  }

  function addExample(examples: string[], newExample: string) {
    if (newExample.trim()) {
      examples.push(newExample.trim());
      hasUnsavedChanges = true;
    }
  }

  function removeExample(examples: string[], index: number) {
    examples.splice(index, 1);
    hasUnsavedChanges = true;
  }

  function addParameterMapping(mapping: Record<string, string>, key: string, value: string) {
    if (key.trim() && value.trim()) {
      mapping[key.trim()] = value.trim();
      hasUnsavedChanges = true;
    }
  }

  function removeParameterMapping(mapping: Record<string, string>, key: string) {
    delete mapping[key];
    hasUnsavedChanges = true;
  }

  function markAsChanged() {
    hasUnsavedChanges = true;
  }

  function getServerStatus(server: McpServer): string {
    switch (server.status) {
      case "connected": return "🟢 Connected";
      case "connecting": return "🟡 Connecting...";
      case "error": return "🔴 Error";
      default: return "⚪ Disconnected";
    }
  }

  function getServerStatusClass(server: McpServer): string {
    switch (server.status) {
      case "connected": return "status-connected";
      case "connecting": return "status-connecting";
      case "error": return "status-error";
      default: return "status-disconnected";
    }
  }

  async function installMcpServer(serverName: string) {
    try {
      installingServers = [...installingServers, serverName];
      await invoke("install_builtin_server", { serverName });
      await loadMcpServers();
      success = `Successfully installed ${serverName} server!`;
      setTimeout(() => success = "", 3000);
    } catch (err) {
      error = `Failed to install ${serverName} server: ${err}`;
    } finally {
      installingServers = installingServers.filter(s => s !== serverName);
    }
  }

  async function installPackage() {
    if (!packageToInstall) {
      error = "Please enter a package name to install.";
      return;
    }

    try {
      installingPackage = true;
      await invoke("install_mcp_package", { packageName: packageToInstall });
      success = `Successfully installed ${packageToInstall}!`;
      setTimeout(() => success = "", 3000);
      
      // Auto-configure the server form based on the installed package
      autoConfigureFromPackage(packageToInstall);
      
      await loadMcpServers(); // Refresh list to show installed server
    } catch (err) {
      error = `Failed to install ${packageToInstall}: ${err}`;
    } finally {
      installingPackage = false;
    }
  }

  function autoConfigureFromPackage(packageName: string) {
    // Auto-fill the form based on common package patterns
    newServer.config.command = "bunx";
    packageOrScript = packageName;
    newServer.config.args = [packageName];

    // Suggest names based on package
    if (packageName.includes("clickup")) {
      newServer.name = newServer.name || "clickup";
      newServer.display_name = newServer.display_name || "ClickUp";
      newServer.description = newServer.description || "Task and project management with ClickUp";
      newServer.config.env = { "CLICKUP_API_TOKEN": "your_token_here" };
    } else if (packageName.includes("github")) {
      newServer.name = newServer.name || "github";
      newServer.display_name = newServer.display_name || "GitHub";
      newServer.description = newServer.description || "GitHub repository management";
      newServer.config.env = { "GITHUB_TOKEN": "your_token_here" };
    } else if (packageName.includes("filesystem")) {
      newServer.name = newServer.name || "filesystem";
      newServer.display_name = newServer.display_name || "Filesystem";
      newServer.description = newServer.description || "Local file operations";
    } else {
      // Generic package
      const packagePart = packageName.split('/').pop() || packageName;
      const cleanName = packagePart.replace(/[-_]mcp[-_]server/, '').replace(/[@\-_]/g, '');
      newServer.name = newServer.name || cleanName;
      newServer.display_name = newServer.display_name || cleanName.charAt(0).toUpperCase() + cleanName.slice(1);
      newServer.description = newServer.description || `MCP server for ${cleanName}`;
    }

    packageToInstall = "";
  }

  // Load existing MCP servers into JSON format
  function loadMcpServersIntoJson() {
    if (mcpServers.length === 0) {
      mcpConfigJson = jsonPlaceholder;
    } else {
      const config = {
        mcpServers: {}
      };
      
      mcpServers.forEach(server => {
        config.mcpServers[server.name] = {
          command: server.config.command,
          args: server.config.args,
          env: server.config.env
        };
      });
      
      mcpConfigJson = JSON.stringify(config, null, 2);
    }
    validateJson();
  }

  function validateJson() {
    try {
      const parsed = JSON.parse(mcpConfigJson);
      if (parsed.mcpServers && typeof parsed.mcpServers === 'object') {
        jsonError = "";
        jsonValid = true;
        
        // Convert JSON back to mcpServers array with proper structure
        mcpServers = Object.entries(parsed.mcpServers).map(([name, config]: [string, any]) => ({
          name: name, // This was missing!
          display_name: name.charAt(0).toUpperCase() + name.slice(1),
          description: `MCP server for ${name}`,
          enabled: true,
          auto_connect: false,
          config: {
            name: name, // Add name to config as well
            command: config.command || "bunx",
            args: config.args || [],
            env: config.env || {},
            transport: "stdio",
            enabled: true // Add enabled to config
          },
          voice_commands: []
        }));
        
        hasUnsavedChanges = true;
        
        // Auto-save when JSON is valid
        saveMcpServers();
      } else {
        jsonError = "Configuration must have 'mcpServers' object";
        jsonValid = false;
      }
    } catch (error: any) {
      jsonError = `Invalid JSON: ${error.message}`;
      jsonValid = false;
    }
  }

  function formatJson() {
    try {
      const parsed = JSON.parse(mcpConfigJson);
      mcpConfigJson = JSON.stringify(parsed, null, 2);
      validateJson();
         } catch (error: any) {
       jsonError = `Cannot format invalid JSON: ${error.message}`;
     }
  }

  function addExampleServer() {
    try {
      const parsed = JSON.parse(mcpConfigJson);
      if (!parsed.mcpServers) parsed.mcpServers = {};
      
      const exampleName = `example_${Date.now()}`;
      parsed.mcpServers[exampleName] = {
        command: "bunx",
        args: ["@your-package/mcp-server"],
        env: {
          "API_TOKEN": "your-token-here"
        }
      };
      
      mcpConfigJson = JSON.stringify(parsed, null, 2);
      validateJson();
    } catch (error) {
      // If JSON is invalid, replace with template
      mcpConfigJson = jsonPlaceholder;
      validateJson();
    }
  }

  function loadJsonTemplate(templateName: string) {
    const templates: Record<string, any> = {
      clickup: {
        mcpServers: {
          clickup: {
            command: "bunx",
            args: ["@taazkareem/clickup-mcp-server"],
            env: {
              "CLICKUP_API_TOKEN": "your-api-key",
              "CLICKUP_TEAM_ID": "your-team-id"
            }
          }
        }
      },
      github: {
        mcpServers: {
          github: {
            command: "bunx", 
            args: ["@modelcontextprotocol/server-github"],
            env: {
              "GITHUB_TOKEN": "your-github-token"
            }
          }
        }
      },
      filesystem: {
        mcpServers: {
          filesystem: {
            command: "bunx",
            args: ["@modelcontextprotocol/server-filesystem"],
            env: {}
          }
        }
      },
      custom: {
        mcpServers: {
          "my-server": {
            command: "node",
            args: ["/path/to/your/server.js"],
            env: {
              "CONFIG_FILE": "/path/to/config.json"
            }
          }
        }
      }
    };

    const template = templates[templateName];
    if (template) {
      mcpConfigJson = JSON.stringify(template, null, 2);
      validateJson();
    }
  }

  async function installFromJson() {
    if (!jsonValid) {
      error = "Please fix JSON errors before installing packages";
      return;
    }

    try {
      installing = true;
      installationLogs = [];
      
      const parsed = JSON.parse(mcpConfigJson);
      const servers = parsed.mcpServers || {};
      
      for (const [serverName, config] of Object.entries(servers)) {
        const serverConfig = config as any;
        
        // Extract package names from args
        for (const arg of serverConfig.args || []) {
          if (arg.startsWith('@') || arg.includes('/')) {
            // This looks like a package name
            addLog('info', `Installing ${arg}...`);
            
            try {
              await invoke("install_mcp_package", { packageName: arg });
              addLog('success', `✅ Installed ${arg}`);
            } catch (err) {
              addLog('error', `❌ Failed to install ${arg}: ${err}`);
            }
          }
        }
      }
      
      addLog('success', '🎉 Installation process completed!');
      setTimeout(() => {
        installing = false;
        installationLogs = [];
      }, 3000);
      
    } catch (err) {
      addLog('error', `Installation failed: ${err}`);
      installing = false;
    }
  }

  function addLog(type: string, message: string) {
    installationLogs = [...installationLogs, { type, message }];
  }
</script>

<div class="advanced-features-page">
  <div class="page-content">
    <!-- Status Messages -->
    {#if error}
      <div class="error-message">{error}</div>
    {/if}
    {#if success}
      <div class="success-message">{success}</div>
    {/if}

    <!-- MCP Server Management Section -->
    <div class="settings-section">
      <h3>🔌 MCP Server Management</h3>
      <p>Configure MCP servers using JSON, similar to Cursor and Windsurf.</p>
      
      <!-- JSON Configuration Editor -->
      <div class="json-editor-section">
        <div class="editor-header">
          <h4>MCP Configuration</h4>
          <div class="editor-actions">
            <button class="btn btn-secondary btn-small" onclick={addExampleServer}>
              ➕ Add Example
            </button>
            <button class="btn btn-accent btn-small" onclick={formatJson}>
              🎨 Format
            </button>
            <button class="btn btn-primary btn-small" onclick={installFromJson}>
              📦 Install Packages
            </button>
          </div>
        </div>
        
        <div class="json-editor-container">
          <textarea 
            bind:value={mcpConfigJson}
            class="json-editor"
            placeholder={jsonPlaceholder}
            spellcheck="false"
            onchange={validateJson}
          ></textarea>
        </div>
        
        {#if jsonError}
          <div class="json-error">
            <span class="error-icon">⚠️</span>
            {jsonError}
          </div>
        {/if}
        
        {#if jsonValid}
          <div class="json-success">
            <span class="success-icon">✅</span>
            Configuration is valid
          </div>
        {/if}
      </div>

      <!-- Quick Templates -->
      <div class="templates-section">
        <h4>📋 Quick Templates</h4>
        <div class="template-grid">
                     <button class="template-card" onclick={() => loadJsonTemplate('clickup')}>
             <div class="template-icon">📋</div>
             <div class="template-info">
               <h5>ClickUp</h5>
               <p>Task management</p>
             </div>
           </button>
           
           <button class="template-card" onclick={() => loadJsonTemplate('github')}>
             <div class="template-icon">🐙</div>
             <div class="template-info">
               <h5>GitHub</h5>
               <p>Repository management</p>
             </div>
           </button>
           
           <button class="template-card" onclick={() => loadJsonTemplate('filesystem')}>
             <div class="template-icon">📁</div>
             <div class="template-info">
               <h5>Filesystem</h5>
               <p>File operations</p>
             </div>
           </button>
           
           <button class="template-card" onclick={() => loadJsonTemplate('custom')}>
             <div class="template-icon">⚙️</div>
             <div class="template-info">
               <h5>Custom</h5>
               <p>Your own server</p>
             </div>
           </button>
        </div>
      </div>

      <!-- Installation Status -->
      {#if installing}
        <div class="installation-status">
          <div class="status-header">
            <span class="loading-icon">⏳</span>
            <h4>Installing Packages...</h4>
          </div>
          {#each installationLogs as log}
            <div class="log-line" class:error={log.type === 'error'} class:success={log.type === 'success'}>
              {log.message}
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Available Tools -->
    <section class="settings-section card">
      <div class="section-header">
        <h3>Available Tools</h3>
        <button class="btn btn-secondary" onclick={loadAvailableTools} disabled={isLoadingTools}>
          {isLoadingTools ? "Loading..." : "🔄 Refresh"}
        </button>
      </div>

      <div class="tools-grid">
        {#each Object.entries(availableTools) as [serverName, tools]}
          <div class="tools-server">
            <h4>{serverName}</h4>
            <div class="tools-list">
              {#each tools as tool}
                <div class="tool-card">
                  <strong>{tool.name}</strong>
                  <p>{tool.description}</p>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </section>

    <!-- Actions -->
    <section class="actions-section">
      <div class="button-group">
        <button onclick={saveMcpServers} class="btn btn-primary">
          💾 Save MCP Settings
        </button>
        <button onclick={loadMcpServers} class="btn btn-secondary">
          🔄 Reload Settings
        </button>
      </div>
    </section>
  </div>
</div>

<!-- Add Server Modal -->
{#if showAddServer}
  <div class="modal-backdrop" onclick={cancelAddServer}>
    <div class="modal-content large" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h3>Add MCP Server</h3>
        <button class="close-btn" onclick={cancelAddServer}>×</button>
      </div>
      <div class="modal-body">
        <!-- Package Installation Helper -->
        <div class="package-install-section">
          <h4>📦 Package Installation</h4>
          <p>Install an MCP server package from NPM:</p>
          <div class="package-install-row">
            <input 
              type="text" 
              bind:value={packageToInstall}
              placeholder="@taazkareem/clickup-mcp-server"
              class="package-input"
            />
            <button 
              class="btn btn-accent"
              onclick={installPackage}
              disabled={installingPackage}
            >
              {installingPackage ? 'Installing...' : 'Install'}
            </button>
          </div>
          <div class="package-examples">
            <p>Popular packages:</p>
            <div class="package-tags">
              <button class="package-tag" onclick={() => packageToInstall = '@taazkareem/clickup-mcp-server'}>
                ClickUp
              </button>
              <button class="package-tag" onclick={() => packageToInstall = '@modelcontextprotocol/server-github'}>
                GitHub
              </button>
              <button class="package-tag" onclick={() => packageToInstall = '@modelcontextprotocol/server-filesystem'}>
                Filesystem
              </button>
            </div>
          </div>
        </div>

        <div class="divider">or configure manually</div>

        <!-- Manual Configuration -->
        <div class="form-group">
          <label>Server Name (unique identifier)</label>
          <input type="text" bind:value={newServer.name} placeholder="e.g., my-clickup" />
        </div>
        <div class="form-group">
          <label>Display Name</label>
          <input type="text" bind:value={newServer.display_name} placeholder="e.g., My ClickUp Integration" />
        </div>
        <div class="form-group">
          <label>Description</label>
          <textarea bind:value={newServer.description} placeholder="Brief description of what this server does"></textarea>
        </div>
        <div class="form-group">
          <label>Command</label>
          <select bind:value={newServer.config.command}>
            <option value="bunx">bunx (recommended for NPM packages)</option>
            <option value="npx">npx</option>
            <option value="node">node</option>
            <option value="python">python</option>
            <option value="bun">bun</option>
          </select>
        </div>
        <div class="form-group">
          <label>Package/Script</label>
          <input 
            type="text" 
            bind:value={packageOrScript}
            placeholder="@taazkareem/clickup-mcp-server or /path/to/script.js"
            onchange={() => {
              newServer.config.args = packageOrScript ? [packageOrScript] : [];
            }}
          />
        </div>
        <div class="form-group">
          <label>Environment Variables</label>
          <div class="env-vars-editor">
            {#each Object.entries(newServer.config.env) as [key, value], index}
              <div class="env-var-row">
                <input 
                  type="text" 
                  placeholder="Variable name (e.g., API_TOKEN)"
                  value={key}
                  onchange={(e) => {
                    const target = e.target as HTMLInputElement;
                    const oldKey = key;
                    const newKey = target.value;
                    if (newKey !== oldKey) {
                      delete newServer.config.env[oldKey];
                      if (newKey) {
                        newServer.config.env[newKey] = value;
                      }
                    }
                  }}
                />
                <input 
                  type="text" 
                  placeholder="Value"
                  bind:value={newServer.config.env[key]}
                />
                <button 
                  type="button" 
                  class="btn btn-danger small"
                  onclick={() => {
                    delete newServer.config.env[key];
                    newServer.config.env = { ...newServer.config.env };
                  }}
                >
                  Remove
                </button>
              </div>
            {/each}
            <button 
              type="button" 
              class="btn btn-secondary small"
              onclick={() => {
                newServer.config.env['NEW_VAR'] = '';
                newServer.config.env = { ...newServer.config.env };
              }}
            >
              Add Environment Variable
            </button>
          </div>
        </div>
        <div class="form-group">
          <label>Common MCP Server Templates</label>
          <div class="templates-section">
            <button 
              type="button" 
              class="btn btn-outline small"
              onclick={() => loadTemplate('clickup')}
            >
              ClickUp
            </button>
            <button 
              type="button" 
              class="btn btn-outline small"
              onclick={() => loadTemplate('github')}
            >
              GitHub
            </button>
            <button 
              type="button" 
              class="btn btn-outline small"
              onclick={() => loadTemplate('filesystem')}
            >
              File System
            </button>
            <button 
              type="button" 
              class="btn btn-outline small"
              onclick={() => loadTemplate('notion')}
            >
              Notion
            </button>
          </div>
        </div>
        <div class="checkbox-group">
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={newServer.enabled} />
            <span class="checkmark"></span>
            Enable server
          </label>
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={newServer.auto_connect} />
            <span class="checkmark"></span>
            Auto-connect on startup
          </label>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" onclick={cancelAddServer}>Cancel</button>
        <button class="btn btn-primary" onclick={saveNewServer}>Add Server</button>
      </div>
    </div>
  </div>
{/if}

<!-- Edit Server Modal -->
{#if selectedServer}
  <div class="modal-backdrop" onclick={() => selectedServer = null}>
    <div class="modal-content large" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h3>Configure {selectedServer.display_name}</h3>
        <button class="close-btn" onclick={() => selectedServer = null}>×</button>
      </div>
      <div class="modal-body">
        <div class="form-group">
          <label>Display Name</label>
          <input type="text" bind:value={selectedServer.display_name} />
        </div>
        <div class="form-group">
          <label>Description</label>
          <textarea bind:value={selectedServer.description}></textarea>
        </div>
        <div class="form-group">
          <label>Command</label>
          <input type="text" bind:value={selectedServer.config.command} />
        </div>
        <div class="form-group">
          <label>Arguments (one per line)</label>
          <textarea 
            value={selectedServer?.config.args.join('\n') || ''} 
            onchange={(e) => {
              const target = e.target as HTMLTextAreaElement;
              if (selectedServer) {
                selectedServer.config.args = target.value.split('\n').filter((arg: string) => arg.trim());
              }
            }}
          ></textarea>
        </div>
        <div class="form-group">
          <label>Environment Variables</label>
          <div class="env-vars-editor">
            {#each Object.entries(selectedServer.config.env) as [key, value], index}
              <div class="env-var-row">
                <input 
                  type="text" 
                  placeholder="Variable name (e.g., API_TOKEN)"
                  value={key}
                  onchange={(e) => {
                    const target = e.target as HTMLInputElement;
                    const oldKey = key;
                    const newKey = target.value;
                    if (newKey !== oldKey && selectedServer) {
                      delete selectedServer.config.env[oldKey];
                      if (newKey) {
                        selectedServer.config.env[newKey] = value;
                      }
                    }
                  }}
                />
                <input 
                  type="text" 
                  placeholder="Value"
                  bind:value={selectedServer.config.env[key]}
                />
                <button 
                  type="button" 
                  class="btn btn-danger small"
                  onclick={() => {
                    if (selectedServer) {
                      delete selectedServer.config.env[key];
                      selectedServer.config.env = { ...selectedServer.config.env };
                    }
                  }}
                >
                  Remove
                </button>
              </div>
            {/each}
            <button 
              type="button" 
              class="btn btn-secondary small"
              onclick={() => {
                if (selectedServer) {
                  selectedServer.config.env['NEW_VAR'] = '';
                  selectedServer.config.env = { ...selectedServer.config.env };
                }
              }}
            >
              Add Environment Variable
            </button>
          </div>
        </div>
        <div class="checkbox-group">
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={selectedServer.enabled} />
            <span class="checkmark"></span>
            Enable server
          </label>
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={selectedServer.auto_connect} />
            <span class="checkmark"></span>
            Auto-connect on startup
          </label>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" onclick={() => selectedServer = null}>Cancel</button>
        <button class="btn btn-primary" onclick={saveEditedServer}>Save Changes</button>
      </div>
    </div>
  </div>
{/if}

<!-- Voice Command Modal -->
{#if editingVoiceCommand}
  <div class="modal-backdrop" onclick={() => editingVoiceCommand = null}>
    <div class="modal-content large" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h3>Configure Voice Command</h3>
        <button class="close-btn" onclick={() => editingVoiceCommand = null}>×</button>
      </div>
      <div class="modal-body">
        <div class="form-group">
          <label>Tool Name</label>
          <input type="text" bind:value={editingVoiceCommand.tool_name} placeholder="e.g., create_task" />
        </div>
        <div class="form-group">
          <label>Description</label>
          <textarea bind:value={editingVoiceCommand.description} placeholder="What does this command do?"></textarea>
        </div>
        
        <!-- Trigger Phrases -->
        <div class="form-group">
          <label>Trigger Phrases</label>
          <div class="tag-input">
                         {#each editingVoiceCommand?.trigger_phrases || [] as phrase, index}
               <span class="tag">
                 "{phrase}"
                 <button onclick={() => editingVoiceCommand && removeTriggerPhrase(editingVoiceCommand.trigger_phrases, index)}>×</button>
               </span>
             {/each}
             <input 
               type="text" 
               placeholder="Add trigger phrase..." 
               onkeydown={(e) => {
                 if (e.key === 'Enter' && editingVoiceCommand) {
                   const target = e.target as HTMLInputElement;
                   addTriggerPhrase(editingVoiceCommand.trigger_phrases, target.value);
                   target.value = '';
                 }
               }}
             />
          </div>
        </div>

        <!-- Examples -->
        <div class="form-group">
          <label>Examples</label>
          <div class="tag-input">
            {#each editingVoiceCommand.examples as example, index}
              <span class="tag">
                {example}
                <button onclick={() => removeExample(editingVoiceCommand.examples, index)}>×</button>
              </span>
            {/each}
            <input 
              type="text" 
              placeholder="Add example..." 
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  addExample(editingVoiceCommand.examples, e.target.value);
                  e.target.value = '';
                }
              }}
            />
          </div>
        </div>

        <!-- Parameter Mapping -->
        <div class="form-group">
          <label>Parameter Mapping</label>
          <div class="parameter-mapping">
            {#each Object.entries(editingVoiceCommand.parameter_mapping) as [key, value]}
              <div class="parameter-row">
                <span class="parameter-key">{key}</span>
                <span class="parameter-arrow">→</span>
                <span class="parameter-value">{value}</span>
                <button onclick={() => removeParameterMapping(editingVoiceCommand.parameter_mapping, key)}>×</button>
              </div>
            {/each}
            <div class="parameter-row">
              <input type="text" placeholder="Parameter name..." id="param-key" />
              <span class="parameter-arrow">→</span>
              <input type="text" placeholder="Extraction pattern..." id="param-value" />
              <button onclick={() => {
                const keyInput = document.getElementById('param-key');
                const valueInput = document.getElementById('param-value');
                addParameterMapping(editingVoiceCommand.parameter_mapping, keyInput.value, valueInput.value);
                keyInput.value = '';
                valueInput.value = '';
              }}>+</button>
            </div>
          </div>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" onclick={() => editingVoiceCommand = null}>Cancel</button>
                 <button class="btn btn-primary" onclick={() => {
           // Find the server we're editing for - use a temporary reference
           if (selectedServer && editingVoiceCommand) {
             saveVoiceCommand(selectedServer);
           }
         }}>Save Command</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .advanced-features-page {
    padding: 24px;
    height: 100%;
  }

  .page-content {
    display: flex;
    flex-direction: column;
    gap: 24px;
    max-width: 100%;
  }

  .settings-section {
    padding: 20px;
  }

  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
  }

  .section-header h3 {
    margin: 0;
    font-size: 1.2rem;
    color: var(--text-primary);
  }

  .error-message {
    padding: 12px;
    background: rgba(244, 67, 54, 0.1);
    color: var(--error);
    border-radius: 6px;
    border: 1px solid var(--error);
    margin-bottom: 16px;
  }

  .success-message {
    padding: 12px;
    background: rgba(76, 175, 80, 0.1);
    color: var(--success);
    border-radius: 6px;
    border: 1px solid var(--success);
    margin-bottom: 16px;
  }

  .server-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .server-card {
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: 16px;
    background: var(--bg-tertiary);
  }

  .server-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 16px;
  }

  .server-info h4 {
    margin: 0 0 4px 0;
    color: var(--text-primary);
  }

  .server-description {
    margin: 0 0 8px 0;
    color: var(--text-secondary);
    font-size: 0.9rem;
  }

  .server-status {
    font-size: 0.85rem;
    font-weight: 500;
  }

  .status-connected { color: var(--success); }
  .status-connecting { color: var(--warning); }
  .status-error { color: var(--error); }
  .status-disconnected { color: var(--text-secondary); }

  .server-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .voice-commands-section {
    border-top: 1px solid var(--border-primary);
    padding-top: 16px;
  }

  .voice-commands-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }

  .voice-commands-header h5 {
    margin: 0;
    color: var(--text-primary);
    font-size: 1rem;
  }

  .voice-command-card {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: 12px;
    border: 1px solid var(--border-secondary);
    border-radius: 6px;
    margin-bottom: 8px;
    background: var(--bg-primary);
  }

  .command-info strong {
    color: var(--accent-primary);
  }

  .command-info p {
    margin: 4px 0;
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .trigger-phrases {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 6px;
  }

  .phrase-tag {
    background: var(--accent-primary);
    color: white;
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 0.8rem;
  }

  .tools-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 16px;
  }

  .tools-server {
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: 16px;
    background: var(--bg-tertiary);
  }

  .tools-server h4 {
    margin: 0 0 12px 0;
    color: var(--text-primary);
  }

  .tools-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .tool-card {
    padding: 8px 12px;
    border: 1px solid var(--border-secondary);
    border-radius: 4px;
    background: var(--bg-primary);
  }

  .tool-card strong {
    color: var(--accent-secondary);
  }

  .tool-card p {
    margin: 4px 0 0 0;
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .actions-section {
    padding-top: 20px;
  }

  .button-group {
    display: flex;
    gap: 12px;
    justify-content: center;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary);
  }

  /* Modal Styles */
  .modal-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    width: 100vw;
    height: 100vh;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    justify-content: center;
    align-items: center;
    z-index: 1000;
  }

  .modal-content {
    background: var(--bg-primary);
    border-radius: 8px;
    width: 90%;
    max-width: 500px;
    max-height: 90vh;
    overflow-y: auto;
    border: 1px solid var(--border-primary);
  }

  .modal-content.large {
    max-width: 700px;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-primary);
  }

  .modal-header h3 {
    margin: 0;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 1.5rem;
    cursor: pointer;
    padding: 4px;
  }

  .close-btn:hover {
    color: var(--error);
  }

  .modal-body {
    padding: 20px;
  }

  .form-group {
    margin-bottom: 16px;
  }

  .form-group label {
    display: block;
    margin-bottom: 6px;
    color: var(--text-primary);
    font-weight: 500;
  }

  .form-group input,
  .form-group textarea {
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  .form-group textarea {
    resize: vertical;
    min-height: 60px;
  }

  .checkbox-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .tag-input {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    padding: 8px;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    background: var(--bg-tertiary);
  }

  .tag {
    background: var(--accent-primary);
    color: white;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 0.85rem;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .tag button {
    background: none;
    border: none;
    color: white;
    cursor: pointer;
    font-size: 0.9rem;
  }

  .parameter-mapping {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .parameter-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .parameter-key,
  .parameter-value {
    padding: 4px 8px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 0.85rem;
  }

  .parameter-arrow {
    color: var(--text-secondary);
  }

  .parameter-row input {
    flex: 1;
  }

  .parameter-row button {
    background: var(--accent-primary);
    color: white;
    border: none;
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding: 16px 20px;
    border-top: 1px solid var(--border-primary);
  }

  .btn-small {
    padding: 4px 8px;
    font-size: 0.8rem;
  }

  .env-vars-editor {
    border: 1px solid var(--border-secondary);
    border-radius: 6px;
    padding: 12px;
    background: var(--bg-secondary);
  }

  .env-var-row {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
    align-items: center;
  }

  .env-var-row input[type="text"] {
    flex: 1;
    min-width: 120px;
  }

  .env-var-row input[type="text"]:first-child {
    max-width: 200px;
  }

  .templates-section {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }

  .btn.small {
    padding: 6px 12px;
    font-size: 0.85rem;
  }

  .btn.btn-outline {
    background: transparent;
    border: 1px solid var(--border-primary);
    color: var(--text-primary);
  }

  .btn.btn-outline:hover {
    background: var(--border-primary);
  }

  /* JSON Editor Styles */
  .json-editor-section {
    margin-bottom: 30px;
  }

  .editor-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 15px;
  }

  .editor-actions {
    display: flex;
    gap: 8px;
  }

  .json-editor-container {
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    overflow: hidden;
  }

  .json-editor {
    width: 100%;
    min-height: 300px;
    padding: 15px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: none;
    outline: none;
    font-family: 'JetBrains Mono', 'Monaco', 'Consolas', monospace;
    font-size: 14px;
    line-height: 1.4;
    resize: vertical;
    tab-size: 2;
  }

  .json-editor:focus {
    background: var(--bg-secondary);
  }

  .json-error {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 10px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 4px;
    color: #ef4444;
    font-size: 0.9rem;
  }

  .json-success {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 10px;
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid rgba(34, 197, 94, 0.3);
    border-radius: 4px;
    color: #22c55e;
    font-size: 0.9rem;
  }

  .template-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 12px;
    margin-top: 15px;
  }

  .template-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 15px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-secondary);
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .template-card:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
    transform: translateY(-2px);
  }

  .template-icon {
    font-size: 24px;
  }

  .template-info {
    text-align: center;
  }

  .template-info h5 {
    margin: 0;
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .template-info p {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  .installation-status {
    margin-top: 20px;
    padding: 15px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
  }

  .status-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }

  .status-header h4 {
    margin: 0;
    font-size: 1rem;
  }

  .loading-icon {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .log-line {
    padding: 5px 0;
    font-family: 'JetBrains Mono', monospace;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .log-line.error {
    color: #ef4444;
  }

  .log-line.success {
    color: #22c55e;
  }

  /* Mobile responsiveness */
  @media (max-width: 768px) {
    .advanced-features-page {
      padding: 16px;
    }

    .server-actions {
      flex-direction: column;
      align-items: stretch;
    }

    .button-group {
      flex-direction: column;
    }

    .modal-content {
      width: 95%;
      margin: 20px;
    }
  }
</style> 