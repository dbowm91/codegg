# Additional Interfaces Report: Desktop App & Browser Access for codegg

## Executive Summary

codegg currently provides a terminal-based TUI (ratatui) and a basic server mode with WebSocket support. The original Codegg (TypeScript) has evolved to include a Desktop app (initially Tauri, recently migrated to Electron), while browser-based access remains largely untapped in the Rust implementation. This report analyzes the current state, competitor offerings, and provides architecture recommendations.

---

## 1. Current Implementation Analysis

### 1.1 TUI Architecture (`src/tui/mod.rs`)

- **Framework**: Built with `ratatui` (Rust TUI library) and `crossterm` for terminal control
- **Event Loop**: Uses `GlobalEventBus` for pub/sub events, `tokio::select!` for handling terminal input, bus events, and async commands
- **State Management**: Centralized `App` struct with sub-states: `UiState`, `SessionState`, `PromptState`, `MessagesState`, `DialogState`, `AgentState`
- **TuiCommand Pattern**: Async operations sent via `mpsc::channel` to avoid blocking event loop
- **Rendering**: Direct terminal rendering via `Terminal<CrosstermBackend<Stdout>>`

### 1.2 Server Mode (`src/server/`)

**Feature-gated behind `server` feature flag**

| Component | Technology | Purpose |
|-----------|-------------|---------|
| HTTP Server | Axum 0.8 | REST API and WebSocket upgrade |
| WebSocket (RPC) | `tokio-tungstenite` | JSON-RPC 2.0 for session/tool/provider operations |
| WebSocket (TUI) | Axum WebSocket | TUI message passing (`TuiMessage` enum) |
| SSE Events | Axum SSE | Server-Sent Events for real-time updates |
| Rate Limiting | In-memory (DashMap) | 100 req/min default, Redis planned but not implemented |
| Auth | Token-based | `CODEGG_SERVER_TOKEN` env var |
| CORS | tower-http | Configurable via `cors_origins` |

**Key Server Endpoints**:
- `/ws` - WebSocket RPC interface
- `/tui` - WebSocket TUI interface (receives `TuiMessage`, sends events)
- `/api/event` - SSE event stream
- `/api/sessions/*` - Session CRUD
- `/api/tools` - Tool listing
- `/api/mcp` - MCP server management

### 1.3 Current Gaps

| Feature | Status |
|---------|--------|
| Desktop app shell (Electron/Tauri) | ❌ Not implemented |
| Browser-based WebUI | ❌ Not implemented |
| xterm.js integration | ❌ Not implemented |
| Multi-session side-by-side UI | ❌ TUI only supports one session at a time |
| Scheduled tasks UI | ❌ Not implemented |
| Mobile access | ❌ No responsive WebUI |

---

## 2. Codegg.ai Desktop App Research

### 2.1 Original Codegg Desktop (TypeScript Version)

Based on research, the original Codegg (anomalyco/codegg) has:
- **Desktop app in Beta** (macOS, Windows, Linux)
- **Initially built with Tauri v2** (Rust + WebView)
- **Recently migrated to Electron** (as of April 2026) due to WebKit performance issues on Mac/Linux

**Desktop App Features** (from codegg.ai):
- LSP-enabled behavior (auto-loading LSPs)
- Multi-session work (multiple agents in parallel)
- Share links to sessions
- Broad provider/model support (75+ LLM providers)
- Native system integration (notifications, auto-update, file dialogs)
- Settings UI for configuration

**Architecture** (from PR #5044):
```
Codegg Desktop (Tauri/Electron)
    ├── Frontend: Solid.js/React-like UI in `packages/desktop/src/`
    ├── Backend: Rust in `packages/desktop/src-tauri/`
    └── Sidecar: Codegg CLI binary bundled and spawned as child process
```

### 2.2 Desktop vs Terminal Comparison

| Feature | Desktop App | CLI/TUI |
|---------|-------------|---------|
| Graphical Interface | ✅ Native window, clickable UI | ❌ Terminal only |
| Multiple Sessions | ✅ Sidebar with tabs | ⚠️ Single session (can run multiple terminals) |
| Session Sharing | ✅ GUI with share links | ✅ CLI command |
| File Navigation | ✅ GUI file tree, drag-drop | ⚠️ Terminal file picker |
| Resource Usage | Higher (WebView/Chromium) | Lower (native terminal) |
| Automation/Scripting | Limited | ✅ Full CLI access |
| Remote Access | ❌ Not directly | ✅ SSH to server |
| Scheduled Tasks | ✅ GUI scheduler | ⚠️ Cron/systemd |

---

## 3. Claude Code Desktop/Browser Research

### 3.1 Claude Code Desktop Features

**Key Differentiators** (from claude.com/blog):
- **Redesigned sidebar** for managing multiple sessions
- **Drag-and-drop layout** for arranging workspace
- **Integrated terminal and file editor** within the app
- **Visual diff review** with inline commenting
- **Scheduled tasks** (local and cloud routines)
- **Dispatch** - start sessions from mobile via claude.ai
- **Computer use** on macOS and Windows
- **Multiple view modes** - Verbose, Normal, Summary
- **Side chats** (⌘+;) for branching conversations

**Desktop vs CLI Feature Matrix**:

| Feature | Desktop | CLI |
|---------|---------|-----|
| Multiple sessions | Sidebar tabs | Separate terminals |
| Scheduled tasks | GUI scheduler | `/loop`, cron |
| Computer use | ✅ macOS/Windows | Limited |
| Dispatch (mobile) | ✅ Via cloud | ❌ |
| Git operations | GUI buttons | CLI commands |
| Permission modes | GUI toggles | Flags/config |

### 3.2 Browser Interface

Claude Code's browser strategy:
- **Web routines** at claude.ai/code/routines
- **Channels** - push events from GitHub Actions, CI
- **Remote Control** - control desktop from web
- **Cloud routines** run on Anthropic infrastructure (even when computer is off)

---

## 4. Browser-based Access Research

### 4.1 xterm.js Ecosystem

**xterm.js** is the standard for browser-based terminals:
- Used by: VS Code, Hyper, Tabby, Theia
- **GPU-accelerated** rendering (WebGL addon)
- **Rich Unicode support** (CJK, emojis, IMEs)
- **Self-contained** - zero dependencies core
- **Addons**: attach (WebSocket), fit, web-links, search

**Integration Pattern**:
```javascript
// Frontend (browser)
const term = new Terminal();
const socket = new WebSocket('ws://localhost:7681');
term.loadAddon(new AttachAddon.AttachAddon(socket));

// Backend (Rust)
// WebSocket server sends terminal I/O via binary frames
```

### 4.2 ttyd - Quick Browser Terminal

**ttyd** (C-based):
- Exposes any CLI program via WebSocket + xterm.js
- **One-line start**: `ttyd -W bash`
- **Lightweight**: ~5MB binary
- **Features**: TLS, authentication, ZMODEM file transfer
- **Used by**: Control-Terminal, AgentBoard, many self-hosted tools

**Architecture**:
```
Browser → HTTPS (Caddy/Traefik) → ttyd (localhost:7681) → tmux/bash
```

### 4.3 Modern Rust + WebView Approaches

| Approach | Technology | Bundle Size | Memory | Use Case |
|----------|-------------|-------------|--------|----------|
| **Tauri v2** | Rust + OS WebView | 2-15MB | 50-150MB | Desktop app with web UI |
| **Electron** | Chromium + Node.js | 80-200MB | 200-500MB | Cross-platform desktop |
| **Pure WebUI** | xterm.js + WebSocket | N/A (browser) | N/A | Browser access |
| **ttyd** | C + xterm.js | ~5MB | ~20MB | Quick browser terminal |

**Tauri v2 vs Electron (2026 Benchmarks)**:
- Startup: Tauri 0.5s vs Electron 2-4s
- Memory: Tauri 75MB avg vs Electron 300MB avg
- Bundle: Tauri 5MB avg vs Electron 120MB avg
- **Tauri v2 added mobile support** (iOS + Android) in 2026

---

## 5. Gap Analysis

### 5.1 What Exists

| Component | Technology | Status |
|-----------|-------------|--------|
| TUI (terminal) | ratatui + crossterm | ✅ Fully functional |
| Server mode | Axum + WebSocket | ✅ Basic (feature-gated) |
| WebSocket TUI | Axum WS + `TuiMessage` | ✅ Protocol defined, needs frontend |
| REST API | Axum HTTP | ✅ Sessions, tools, MCP, files |
| SSE Events | Axum SSE | ✅ For real-time updates |
| IDE Extensions | VS Code extension | ✅ Basic diff viewing |

### 5.2 What's Missing

| Feature | Priority | Complexity |
|---------|----------|------------|
| **Desktop App Shell** | High | Medium - Tauri v2 integration |
| **Browser-based WebUI** | High | Medium - xterm.js + React/Vue |
| **Multi-session UI** | High | Medium - Sidebar + tab management |
| **Scheduled Tasks UI** | Medium | Low - GUI over cron-like functionality |
| **Mobile-responsive UI** | Medium | Medium - Tauri mobile or PWA |
| **File tree + editor in browser** | Medium | High - Monaco/CodeMirror integration |
| **Share session UI** | Low | Low - Already in API |

### 5.3 What's Needed

1. **Desktop Shell** (Tauri v2 preferred):
   - Rust backend to spawn/manage codegg server
   - WebView frontend with xterm.js for terminal rendering
   - Native OS integration (notifications, auto-update, file dialogs)

2. **WebUI Frontend**:
   - xterm.js for terminal emulation
   - React/TypeScript or Vue for UI components
   - WebSocket client to connect to `/tui` endpoint
   - Session management UI (sidebar with multiple sessions)

3. **Shared TUI Logic**:
   - Extract terminal rendering logic for reuse between TUI and WebUI
   - Or use xterm.js in browser and ratatui in terminal (separate implementations)

---

## 6. Architecture Recommendations

### 6.1 Desktop App Architecture (Tauri v2)

```
┌─────────────────────────────────────────────────────┐
│           Codegg Desktop (Tauri v2)              │
├─────────────────────────────────────────────────────┤
│ Frontend (WebView)                                 │
│  ├── Solid.js/React UI        (TypeScript)        │
│  ├── xterm.js                   (Terminal)        │
│  └── WebSocket Client          (TUI protocol)     │
├─────────────────────────────────────────────────────┤
│ Backend (Rust)                                    │
│  ├── Tauri Commands            (System integration)│
│  ├── codegg server       (Sidecar process)   │
│  └── Native plugins           (Notifications, etc)│
└─────────────────────────────────────────────────────┘
         │                                    │
         │ WebSocket (TuiMessage)           │ HTTP/WS
         ▼                                    ▼
┌─────────────────────────────────────────────────────┐
│              codegg Server                   │
│  ├── /tui (WebSocket)  - TUI messages          │
│  ├── /ws (WebSocket)   - JSON-RPC              │
│  ├── /api/* (REST)      - Sessions, tools, etc │
│  └── /api/event (SSE)  - Real-time events      │
└─────────────────────────────────────────────────────┘
```

**Why Tauri v2 over Electron for codegg**:
1. **Rust-native** - Leverage existing Rust codebase
2. **Smaller bundle** - 5MB vs 120MB (24x smaller)
3. **Lower memory** - 75MB vs 300MB avg
4. **Mobile support** - Tauri v2 supports iOS/Android
5. **Security** - Rust backend + OS WebView (no bundled Chromium)

### 6.2 Browser-based WebUI

**Option A: Pure Browser UI (No Desktop Shell)**

```
Browser (any device)
    ├── xterm.js terminal (via WebSocket to /tui)
    ├── React/TypeScript UI
    └── Service Worker (PWA for offline)

Connection:
    Browser → HTTPS → Nginx/Caddy → codegg server (:3000)
                     └── /tui WebSocket (TuiMessage protocol)
                     └── /api/* REST API
                     └── /api/event SSE
```

**Option B: ttyd Integration (Quick Solution)**

```
Browser → ttyd (:7681) → codegg TUI (spawned in tmux)
```

- **Pros**: Zero development effort, works immediately
- **Cons**: No custom UI, no session management UI

**Option C: Custom WebUI (Recommended)**

```
Browser
    ├── TerminalView (xterm.js + WebSocket to /tui)
    ├── Sidebar (session list, agent list)
    ├── MainView (chat messages, tool calls)
    └── Dialogs (permission, model select, etc.)

Frontend Stack:
    - Framework: React 19 + TypeScript (or Vue 3)
    - Terminal: xterm.js 6 + WebGL addon
    - State: Zustand (or Redux Toolkit)
    - Build: Vite 7
    - Styling: Tailwind CSS 4
```

### 6.3 Sharing TUI Logic Between Terminal and Browser

**Challenge**: ratatui (terminal) and xterm.js (browser) are fundamentally different.

**Approach 1: Protocol-based (Recommended)**
- Keep ratatui for terminal TUI
- Use xterm.js for browser TUI
- Share the **protocol** (`TuiMessage`) between both
- Server's `/tui` WebSocket already defines this protocol

**Approach 2: Web-based TUI Everywhere**
- Run a headless browser engine (web-view) even in "terminal mode"
- More complex, negates benefits of native terminal TUI

**Approach 3: Abstract UI Components**
- Define UI components abstractly (messages, sidebar, dialogs)
- Render to ratatui in terminal
- Render to React components in browser
- Significant refactoring required

---

## 7. Implementation Roadmap

### Phase 1: Browser Access (Quick Win)
1. **Build WebUI frontend** (React + xterm.js)
   - Connect to existing `/tui` WebSocket endpoint
   - Implement basic terminal view
   - Reuse `TuiMessage` protocol from `src/server/ws.rs`
2. **Add session management UI**
   - List sessions (via `/api/sessions`)
   - Create/delete sessions
   - Switch between sessions
3. **Deploy with ttyd as fallback**
   - Document `ttyd -W codegg` for quick browser access

### Phase 2: Desktop Shell (Tauri v2)
1. **Set up Tauri v2 project**
   - `cargo install tauri-cli` and initialize in `desktop/` directory
   - Configure `tauri.conf.json` for macOS/Windows/Linux
2. **Integrate codegg as sidecar**
   - Bundle codegg binary
   - Spawn server on app start
   - Connect WebUI to local server
3. **Add native features**
   - Auto-update (Tauri plugin)
   - Notifications (tauri-plugin-notification)
   - File dialogs (tauri-plugin-dialog)
   - Deep linking (tauri-plugin-deep-link)

### Phase 3: Advanced Features
1. **Multi-session side-by-side**
   - Tabbed layout in WebUI
   - Drag-and-drop resize (like Claude Code Desktop)
2. **Scheduled tasks UI**
   - GUI for creating local/cloud routines
   - Integrate with system cron or internal scheduler
3. **Mobile PWA**
   - Service worker for offline capability
   - Responsive layout for small screens
   - Touch-friendly controls

---

## 8. Technology Choices Summary

| Decision | Recommendation | Alternatives | Reasoning |
|----------|----------------|--------------|-----------|
| **Desktop Framework** | Tauri v2 | Electron, Neutralino | 24x smaller, Rust-native, mobile support |
| **WebUI Framework** | React 19 + TypeScript | Vue 3, Svelte | Ecosystem, xterm.js integration examples |
| **Terminal Emulation** | xterm.js 6 | Custom canvas, ttyd | Industry standard, VS Code uses it |
| **State Management** | Zustand | Redux, Pinia | Lightweight, TypeScript-first |
| **Build Tool** | Vite 7 | Webpack, Next.js | Fast, modern, Tauri-compatible |
| **Styling** | Tailwind CSS 4 | CSS Modules, styled-components | Rapid UI development |
| **WebSocket Client** | Native WebSocket | Socket.io, tungstenite | Already used in server |

---

## 9. Risk Assessment

| Risk | Mitigation |
|------|-------------|
| **Tauri v2 immaturity** | Electron as fallback option |
| **WebView inconsistencies** | Test on WebKit (macOS/Linux), WebView2 (Windows) |
| **Protocol changes** | Version `TuiMessage` protocol, backwards compat |
| **Performance** | WebGL for xterm.js, virtual scrolling for large lists |
| **Security** | CORS, rate limiting, auth already in server |

---

## 10. Conclusion

codegg has a **solid foundation** with its server mode and WebSocket TUI protocol. The next logical steps are:

1. **Build a WebUI frontend** (React + xterm.js) that connects to the existing `/tui` WebSocket - this provides browser access with minimal backend changes
2. **Wrap it in a Tauri v2 desktop shell** for native OS integration - leverages Rust ecosystem and keeps bundle small
3. **Follow Claude Code's lead** on multi-session UI, scheduled tasks, and mobile access

The server's existing `TuiMessage` protocol and `GlobalEventBus` architecture already provide the necessary plumbing for both browser and desktop interfaces. The main work is building the frontend and desktop shell around the existing backend.

---

## Appendix: Relevant Files in codegg

| File | Purpose |
|------|---------|
| `src/server/ws.rs` | WebSocket TUI protocol (`TuiMessage`, `handle_tui`) |
| `src/server/http.rs` | Axum server setup, CORS, rate limiting |
| `src/server/routes/mod.rs` | API route definitions |
| `src/server/state.rs` | Server state (pool, MCP, event bus) |
| `src/server/routes/event.rs` | SSE event bus (`GlobalEventBus`) |
| `src/tui/mod.rs` | TUI event loop, `TuiCommand` pattern |
| `src/tui/app/` | App state and UI components |
| `Cargo.toml` | Feature flags: `server`, `plugins`, `debug-logging` |

---

*Report generated: 2026-04-28*
*Author: codegg investigation*
