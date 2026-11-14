//! Zed extension for Aevia: starts `aevia-lsp` (or `aevia lsp`) from the worktree's PATH.

use zed_extension_api::LanguageServerId;
use zed_extension_api::Result;
use zed_extension_api::{
    self as zed,
};

struct AeviaExtension;

impl zed::Extension for AeviaExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        if let Some(command) = worktree.which("aevia-lsp") {
            return Ok(zed::Command {
                command,
                args: vec!["--stdio".to_string()],
                env: worktree.shell_env(),
            });
        }
        if let Some(command) = worktree.which("aevia") {
            return Ok(zed::Command {
                command,
                args: vec!["lsp".to_string(), "--stdio".to_string()],
                env: worktree.shell_env(),
            });
        }
        Err(
            "neither `aevia-lsp` nor `aevia` was found on PATH; install the Aevia toolchain"
                .to_string(),
        )
    }
}

zed::register_extension!(AeviaExtension);
