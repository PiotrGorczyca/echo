# MCP Server Setup Guide

This guide shows you how to add and configure any MCP (Model Context Protocol) server in EchoType, giving your AI assistant access to external tools and services.

## What are MCP Servers?

MCP servers are external programs that provide tools and capabilities to your AI assistant. Unlike hardcoded integrations, you can add **any** MCP server you want:

- **ClickUp** - Task and project management (`@taazkareem/clickup-mcp-server`)
- **GitHub** - Code repository management (`@modelcontextprotocol/server-github`)
- **Filesystem** - Local file operations (`@modelcontextprotocol/server-filesystem`)
- **Custom servers** - Build your own or use community packages

## Adding Your First MCP Server

### Method 1: Install & Auto-Configure (Recommended)

1. **Open Settings** → **Advanced Features** → **MCP Server Management**
2. **Click "Add Server"**
3. **In the Package Installation section:**
   - Type the package name (e.g., `@taazkareem/clickup-mcp-server`)
   - Click **"Install"**
   - The form will auto-populate with sensible defaults
4. **Configure environment variables** (like API tokens)
5. **Click "Add Server"**

### Method 2: Manual Configuration

1. **Install the package yourself:**
   ```bash
   bun add -g @taazkareem/clickup-mcp-server
   ```

2. **In EchoType Settings:**
   - **Server Name**: `my-clickup` (unique identifier)
   - **Display Name**: `My ClickUp`
   - **Description**: `Task management for my workspace`
   - **Command**: `bunx` (or `npx`, `node`)
   - **Package/Script**: `@taazkareem/clickup-mcp-server`
   - **Environment Variables**: 
     - `CLICKUP_API_TOKEN`: `your_actual_token_here`

## ClickUp Setup Example

### 1. Get Your ClickUp API Token
1. Go to ClickUp → **Settings** → **Apps**
2. Find **API Token** section
3. **Generate** a new token
4. **Copy** the token (keep it secure!)

### 2. Add ClickUp MCP Server
**Option A: One-Click Install**
1. In EchoType, click **Add Server**
2. Type `@taazkareem/clickup-mcp-server` and click **Install**
3. Enter your API token in the `CLICKUP_API_TOKEN` field
4. Click **Add Server**

**Option B: Manual**
```bash
bun add -g @taazkareem/clickup-mcp-server
```
Then configure manually in EchoType settings.

### 3. Test Your Integration
- **Enable** the server
- **Save** your settings
- Try voice commands like:
  - "Show me my ClickUp tasks"
  - "Create a task called 'Review documents'"

## Finding MCP Servers

### NPM Registry
Search for packages with keywords:
- `mcp-server`
- `model-context-protocol`
- `mcp`

### Community Resources
- **GitHub**: Search for "mcp-server" repositories
- **NPM**: Browse MCP-related packages
- **Community forums**: Discord, Reddit for recommendations

### Popular Packages
```bash
# Task Management
@taazkareem/clickup-mcp-server
@trello/mcp-server

# Development
@modelcontextprotocol/server-github
@gitlab/mcp-server

# File Operations
@modelcontextprotocol/server-filesystem
@ftp/mcp-server

# AI/Content
@replicate/mcp-server
@openai/mcp-server
```

## Troubleshooting

### Package Installation Fails
1. **Check internet connection**
2. **Ensure bun is installed**: `bun --version`
3. **Try alternative package managers**: npx, npm
4. **Check package name** for typos

### Server Won't Connect
1. **Verify package is installed**: `bun list -g`
2. **Check environment variables** are correct
3. **Review API tokens** and permissions
4. **Check server logs** in console

### No Tools Available
1. **Restart EchoType** after adding server
2. **Check server is enabled** and connected
3. **Verify API credentials** work
4. **Review server documentation** for setup requirements

## Advanced Configuration

### Custom Servers
You can run **any** executable as an MCP server:
- **Python scripts**: `python /path/to/server.py`
- **Node.js scripts**: `node /path/to/server.js`
- **Compiled binaries**: `/path/to/custom-server`

### Environment Variables
Common patterns:
- **API Keys**: `API_TOKEN`, `GITHUB_TOKEN`, `CLICKUP_API_TOKEN`
- **Configuration**: `CONFIG_FILE`, `BASE_URL`, `WORKSPACE_ID`
- **Debug settings**: `DEBUG=true`, `LOG_LEVEL=info`

### Transport Types
- **Stdio** (default): Communication via stdin/stdout
- **HTTP**: REST API communication
- **WebSocket**: Real-time communication

## Security Best Practices

1. **Secure API tokens** - Never commit to version control
2. **Use environment variables** - Don't hardcode credentials
3. **Regular token rotation** - Update tokens periodically
4. **Minimal permissions** - Only grant necessary access
5. **Review server code** - Understand what servers do

## Next Steps

Once you have MCP servers configured:
1. **Test voice commands** to ensure integration works
2. **Explore available tools** in each server
3. **Customize voice commands** for your workflow
4. **Add more servers** as needed

Need help? Check the EchoType documentation or community forums! 