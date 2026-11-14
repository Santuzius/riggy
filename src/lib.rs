//! What the avatar (`riggy`), the tray icon (`riggy-tray`) and the speech MCP server (`riggy-mcp`) share: where they reach each other, what the tray keeps between runs, the speech server and how speech is played, and installing models.

pub mod autostart;
pub mod control;
pub mod files;
pub mod install;
pub mod settings;
pub mod speaker;
pub mod speech_server;
pub mod tray_process;
