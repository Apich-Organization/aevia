//! Zed extension for Aevia: starts `aevia-lsp` (or `aevia lsp`) from the worktree's PATH.

use zed::serde_json::json;
use zed::serde_json::Value;
use zed_extension_api as zed;
use zed_extension_api::LanguageServerId;
use zed_extension_api::Result;

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

    fn get_dap_binary(
        &mut self,
        _adapter_name: String,
        config: zed::DebugTaskDefinition,
        user_path: Option<String>,
        worktree: &zed::Worktree,
    ) -> Result<zed::DebugAdapterBinary> {
        if config.tcp_connection.is_some() {
            return Err("Aevia debug uses standard input and output.".into());
        }
        let command = user_path
            .or_else(|| worktree.which("aevia"))
            .ok_or("Install the Aevia CLI on the worktree's PATH to start the debugger.")?;
        Ok(zed::DebugAdapterBinary {
            command: Some(command),
            arguments: vec!["debug".into(), "--stdio".into()],
            envs: worktree.shell_env(),
            cwd: Some(worktree.root_path()),
            connection: None,
            request_args: zed::StartDebuggingRequestArguments {
                request: zed::StartDebuggingRequestArgumentsRequest::Launch,
                configuration: config.config,
            },
        })
    }

    fn dap_request_kind(
        &mut self,
        _adapter_name: String,
        config: Value,
    ) -> Result<zed::StartDebuggingRequestArgumentsRequest> {
        match config.get("request").and_then(Value::as_str) {
            | Some("launch") => Ok(zed::StartDebuggingRequestArgumentsRequest::Launch),
            | _ => Err("Set request to launch for Aevia source debugging.".into()),
        }
    }

    fn dap_config_to_scenario(
        &mut self,
        config: zed::DebugConfig,
    ) -> Result<zed::DebugScenario> {
        let zed::DebugRequest::Launch(launch) = config.request else {
            return Err("Aevia debugging starts from a source file.".into());
        };
        Ok(zed::DebugScenario {
            label: config.label,
            adapter: config.adapter,
            build: None,
            config: json!({
                "request": "launch",
                "program": launch.program,
                "cwd": launch.cwd,
                "args": launch.args,
                "stopOnEntry": config.stop_on_entry.unwrap_or(false),
            })
            .to_string(),
            tcp_connection: None,
        })
    }
}

mod registration {
    zed_extension_api::register_extension!(super::AeviaExtension);
}
