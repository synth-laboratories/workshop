//! Shared IPC substrate (loopback HTTP, MCP stdio).

mod loopback_server;
mod mcp_stdio;

#[cfg(unix)]
pub use loopback_server::serve_unix_connections;
pub use loopback_server::{
    constant_time_eq, json_response, serve_connections, serve_connections_allowing, serve_json,
    serve_json_with_limit, JsonHttpRequest, JsonHttpResponse, LoopbackBody,
};
pub use mcp_stdio::{run_stdio_server, McpServerInfo};
