//! Shared IPC substrate (loopback HTTP, MCP stdio).

mod loopback_server;
mod mcp_stdio;

pub use loopback_server::{
    json_response, serve_connections, serve_json, serve_json_with_limit, JsonHttpRequest,
    JsonHttpResponse, LoopbackBody,
};
pub use mcp_stdio::{run_stdio_server, McpServerInfo};
