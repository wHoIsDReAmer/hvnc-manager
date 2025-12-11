# HVNC Manager (W.I.P)

A remote desktop management system built with Rust, featuring a relay-based architecture for NAT traversal and secure connections.

## Architecture

```
┌──────────────┐         ┌──────────────┐         ┌──────────────┐
│   Manager    │◄───────►│ Relay Server │◄───────►│    Client    │
│              │  QUIC   │              │  QUIC   │   (hVNC)     │
└──────────────┘         └──────────────┘         └──────────────┘
```

- **Manager**: Desktop application with Slint UI for viewing and controlling remote clients
- **Relay Server**: Central hub that routes connections between managers and clients
- **Client**: Windows hVNC (Hidden VNC) agent that captures a hidden desktop session and handles input events without disrupting the user's primary display

## Features

- QUIC-based encrypted connections
- Token-based authentication
- Real-time desktop streaming (RGBA frames)
- Mouse and keyboard input forwarding

## Project Structure

```
crates/
├── shared/        # Common protocol definitions and codec
├── relay-server/  # QUIC relay server
├── manager/       # Desktop manager with Slint UI
└── client-win/    # Windows client (WIP)
```

## Quick Start

```bash
# Start the relay server
cargo run -p relay-server

# Start the mock client (for testing)
cargo run --example mock_client

# Start the manager
cargo run -p manager
```

## Configuration

Set `RELAY_AUTH_TOKEN` environment variable for production:
```bash
export RELAY_AUTH_TOKEN="your-secret-token"
cargo run -p relay-server
```

## License

MIT
