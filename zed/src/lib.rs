//! Zed extension: starts the `flagcheck` language server.
//!
//! The binary is taken from `lsp.flagcheck.binary.path` in Zed settings, or
//! from PATH. Settings in `lsp.flagcheck.initialization_options` are passed
//! to the server unchanged. The shell environment is forwarded so the
//! server can read the API key variable; the key never passes through
//! Zed settings.

use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

const SERVER: &str = "flagcheck";

struct Flagcheck;

impl zed::Extension for Flagcheck {
    fn new() -> Self {
        Flagcheck
    }

    fn language_server_command(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(SERVER, worktree)
            .ok()
            .and_then(|s| s.binary);
        let path = binary
            .as_ref()
            .and_then(|b| b.path.clone())
            .or_else(|| worktree.which(SERVER))
            .ok_or_else(|| format!("{SERVER} not found on PATH"))?;
        let mut env = worktree.shell_env();
        if let Some(extra) = binary.as_ref().and_then(|b| b.env.clone()) {
            env.extend(extra);
        }
        Ok(zed::Command {
            command: path,
            args: binary.and_then(|b| b.arguments).unwrap_or_default(),
            env,
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(SERVER, worktree)
            .ok()
            .and_then(|s| s.initialization_options))
    }
}

zed::register_extension!(Flagcheck);
