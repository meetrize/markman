//! Launch inline shell commands in the platform default terminal.

use std::path::Path;

use anyhow::{Context as _, Result};

/// Builds the shell script body passed to the system terminal (`cd` + user command).
pub(crate) fn build_terminal_run_script(command: &str, work_dir: &Path) -> String {
    format!(
        "cd {} && {}",
        shell_single_quote(&work_dir.display().to_string()),
        command
    )
}

/// Escapes a string for embedding inside an AppleScript double-quoted literal.
pub(crate) fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Opens the user's command in the system terminal, starting in `work_dir`.
pub fn open_in_system_terminal(command: &str, work_dir: &Path) -> Result<()> {
    let script = build_terminal_run_script(command, work_dir);
    open_in_system_terminal_with_script(&script)
}

fn open_in_system_terminal_with_script(script: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let escaped = applescript_escape(script);
        let source = format!(
            "tell application \"Terminal\" to activate\ntell application \"Terminal\" to do script \"{escaped}\""
        );
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&source)
            .status()
            .context("failed to run osascript")?;
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("osascript exited with status {status}");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = script;
        anyhow::bail!("system terminal launch is not supported on this platform");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_terminal_run_script_quotes_working_directory_with_spaces() {
        let script = build_terminal_run_script("echo hi", Path::new("/tmp/foo bar"));
        assert_eq!(script, "cd '/tmp/foo bar' && echo hi");
    }

    #[test]
    fn build_terminal_run_script_quotes_single_quotes_in_working_directory() {
        let script = build_terminal_run_script("pwd", Path::new("/tmp/a'b"));
        assert_eq!(script, "cd '/tmp/a'\\''b' && pwd");
    }

    #[test]
    fn applescript_escape_doubles_backslashes_and_quotes() {
        assert_eq!(
            applescript_escape(r#"say "hi""#),
            r#"say \"hi\""#
        );
        assert_eq!(applescript_escape(r"path\to\file"), r"path\\to\\file");
    }

    #[test]
    fn build_terminal_run_script_preserves_user_command_verbatim() {
        let script = build_terminal_run_script(r#"echo "hello" && ls"#, Path::new("/tmp"));
        assert_eq!(script, r#"cd '/tmp' && echo "hello" && ls"#);
    }
}
