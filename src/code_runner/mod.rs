//! Code-block and inline-code execution for supported scripting languages.

mod system_terminal;

use std::io::{BufRead, BufReader, Read};
use std::ops::Range;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use futures::channel::mpsc::UnboundedSender;
use uuid::Uuid;

pub use system_terminal::open_in_system_terminal;

/// Snapshot of one code block's run state, synced to blocks for rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeBlockRunSnapshot {
    pub status: CodeRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_expanded: bool,
    pub output_content_expanded: bool,
    pub error_message: Option<String>,
}

/// Number of output lines shown before the body auto-collapses.
pub const CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES: usize = 3;

/// Default language label for inline code execution (shell one-liners).
pub const DEFAULT_INLINE_CODE_LANGUAGE: &str = "shell";

/// Maximum combined stdout/stderr characters retained for inline code runs.
pub const INLINE_CODE_RUN_MAX_OUTPUT_CHARS: usize = 8_192;

/// Counts logical lines across run-output text sections.
pub fn code_run_output_line_count(stdout: &str, stderr: &str, error_message: Option<&str>) -> usize {
    let mut line_count = 0usize;
    for section in [stdout, stderr] {
        if section.is_empty() {
            continue;
        }
        line_count += section.split('\n').count();
    }
    if let Some(error) = error_message.filter(|value| !value.is_empty()) {
        line_count += error.split('\n').count();
    }
    line_count
}

impl CodeBlockRunSnapshot {
    pub fn shows_output_panel(&self) -> bool {
        self.status != CodeRunStatus::Idle || self.output_expanded
    }
}

/// Lifecycle status for a code-block run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CodeRunStatus {
    #[default]
    Idle,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// Progress events streamed from the background runner thread.
#[derive(Debug)]
pub enum CodeRunProgress {
    StdoutChunk(String),
    StderrChunk(String),
    Finished(CodeRunOutcome),
}

/// Final outcome reported after the child process exits or is cancelled.
#[derive(Debug)]
pub struct CodeRunOutcome {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub cancelled: bool,
    pub error_message: Option<String>,
}

/// Resolved interpreter and file extension for a fenced language tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerSpec {
    pub program: &'static str,
    pub extension: &'static str,
}

/// Maps a code-block language label to a runnable interpreter.
pub fn resolve_runner(language: &str) -> Option<RunnerSpec> {
    match language.trim().to_ascii_lowercase().as_str() {
        "bash" | "shell" => Some(RunnerSpec {
            program: "bash",
            extension: "sh",
        }),
        "sh" => Some(RunnerSpec {
            program: "sh",
            extension: "sh",
        }),
        "python" | "py" | "python3" => Some(RunnerSpec {
            program: "python3",
            extension: "py",
        }),
        "javascript" | "js" | "node" => Some(RunnerSpec {
            program: "node",
            extension: "js",
        }),
        _ => None,
    }
}

/// Returns the shell interpreter used for inline code execution.
pub fn resolve_inline_code_runner() -> RunnerSpec {
    resolve_runner(DEFAULT_INLINE_CODE_LANGUAGE).expect("inline shell runner must exist")
}

/// Extracts inline code source from visible display text at `span_range`.
///
/// Leading and trailing whitespace are trimmed; internal spacing is preserved.
pub fn extract_inline_code_source(display_text: &str, span_range: &Range<usize>) -> String {
    let end = span_range.end.min(display_text.len());
    let start = span_range.start.min(end);
    display_text[start..end].trim().to_string()
}

/// Spawns a background thread that executes `source` and streams output through `tx`.
pub fn spawn_code_run(
    language: &str,
    source: &str,
    work_dir: &Path,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
    tx: UnboundedSender<CodeRunProgress>,
) -> thread::JoinHandle<()> {
    spawn_run(language, source, work_dir, cancel, child_slot, tx, None)
}

/// Spawns a background thread that executes inline shell `source`.
///
/// Output is capped at [`INLINE_CODE_RUN_MAX_OUTPUT_CHARS`].
pub fn spawn_inline_shell_run(
    source: &str,
    work_dir: &Path,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
    tx: UnboundedSender<CodeRunProgress>,
) -> thread::JoinHandle<()> {
    spawn_run(
        DEFAULT_INLINE_CODE_LANGUAGE,
        source,
        work_dir,
        cancel,
        child_slot,
        tx,
        Some(INLINE_CODE_RUN_MAX_OUTPUT_CHARS),
    )
}

fn spawn_run(
    language: &str,
    source: &str,
    work_dir: &Path,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
    tx: UnboundedSender<CodeRunProgress>,
    max_output_chars: Option<usize>,
) -> thread::JoinHandle<()> {
    let language = language.to_string();
    let source = source.to_string();
    let work_dir = work_dir.to_path_buf();

    thread::Builder::new()
        .name("velotype-code-run".into())
        .spawn(move || {
            let outcome = run_in_thread(
                &language,
                &source,
                &work_dir,
                cancel,
                child_slot,
                tx.clone(),
                max_output_chars,
            );
            let _ = tx.unbounded_send(CodeRunProgress::Finished(outcome));
        })
        .expect("failed to spawn code run thread")
}

fn run_in_thread(
    language: &str,
    source: &str,
    work_dir: &Path,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
    tx: UnboundedSender<CodeRunProgress>,
    max_output_chars: Option<usize>,
) -> CodeRunOutcome {
    let started = Instant::now();
    let Some(spec) = resolve_runner(language) else {
        return CodeRunOutcome {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            cancelled: false,
            error_message: Some(format!("unsupported language: {language}")),
        };
    };

    let temp_path = work_dir.join(format!(
        ".velotype-run-{}.{}",
        Uuid::new_v4(),
        spec.extension
    ));

    if let Err(err) = std::fs::write(&temp_path, source) {
        return CodeRunOutcome {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            cancelled: false,
            error_message: Some(format!("failed to write temp file: {err}")),
        };
    }

    let mut command = Command::new(spec.program);
    command
        .arg(&temp_path)
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = std::fs::remove_file(&temp_path);
            return CodeRunOutcome {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                cancelled: false,
                error_message: Some(format!("failed to start {}: {err}", spec.program)),
            };
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *child_slot.lock().expect("child slot lock") = Some(child);

    let collected_stdout = Arc::new(std::sync::Mutex::new(String::new()));
    let collected_stderr = Arc::new(std::sync::Mutex::new(String::new()));
    let total_output_chars = max_output_chars.map(|_| Arc::new(AtomicUsize::new(0)));
    let mut readers = Vec::new();

    if let Some(stdout) = stdout {
        let tx = tx.clone();
        let cancel = cancel.clone();
        let collected = collected_stdout.clone();
        let total_output_chars = total_output_chars.clone();
        readers.push(thread::spawn(move || {
            read_stream(
                stdout,
                false,
                &tx,
                &cancel,
                collected,
                max_output_chars,
                total_output_chars,
            );
        }));
    }
    if let Some(stderr) = stderr {
        let tx = tx.clone();
        let cancel = cancel.clone();
        let collected = collected_stderr.clone();
        let total_output_chars = total_output_chars.clone();
        readers.push(thread::spawn(move || {
            read_stream(
                stderr,
                true,
                &tx,
                &cancel,
                collected,
                max_output_chars,
                total_output_chars,
            );
        }));
    }

    let exit_code = loop {
        if cancel.load(Ordering::SeqCst) {
            kill_child(&child_slot);
            break None;
        }
        let mut child_guard = child_slot.lock().expect("child slot lock");
        if let Some(child) = child_guard.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    child_guard.take();
                    break status.code();
                }
                Ok(None) => {}
                Err(_) => {
                    child_guard.take();
                    break None;
                }
            }
        } else {
            break None;
        }
        drop(child_guard);
        thread::sleep(std::time::Duration::from_millis(50));
    };

    for reader in readers {
        let _ = reader.join();
    }

    let _ = std::fs::remove_file(&temp_path);

    let cancelled = cancel.load(Ordering::SeqCst);
    CodeRunOutcome {
        stdout: collected_stdout.lock().expect("stdout lock").clone(),
        stderr: collected_stderr.lock().expect("stderr lock").clone(),
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        cancelled,
        error_message: None,
    }
}

fn read_stream<R: Read + Send + 'static>(
    reader: R,
    is_stderr: bool,
    tx: &UnboundedSender<CodeRunProgress>,
    cancel: &Arc<AtomicBool>,
    collected: Arc<std::sync::Mutex<String>>,
    max_output_chars: Option<usize>,
    total_output_chars: Option<Arc<AtomicUsize>>,
) {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let (Some(max), Some(total)) = (max_output_chars, total_output_chars.as_ref())
                {
                    let used = total.load(Ordering::Relaxed);
                    if used >= max {
                        break;
                    }
                    let remaining = max - used;
                    if line.len() > remaining {
                        line.truncate(remaining);
                    }
                    total.fetch_add(line.len(), Ordering::Relaxed);
                }
                if line.is_empty() {
                    break;
                }
                collected
                    .lock()
                    .expect("stream collect lock")
                    .push_str(&line);
                let progress = if is_stderr {
                    CodeRunProgress::StderrChunk(line.clone())
                } else {
                    CodeRunProgress::StdoutChunk(line.clone())
                };
                let _ = tx.unbounded_send(progress);
                if max_output_chars.is_some_and(|max| {
                    total_output_chars
                        .as_ref()
                        .is_some_and(|total| total.load(Ordering::Relaxed) >= max)
                }) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Kills the active child process if one is registered.
pub fn kill_child(child_slot: &Arc<std::sync::Mutex<Option<Child>>>) {
    if let Some(mut child) = child_slot.lock().expect("child slot lock").take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use futures::channel::mpsc;

    use super::*;

    fn collect_run_outcome(
        mut rx: mpsc::UnboundedReceiver<CodeRunProgress>,
    ) -> (String, CodeRunOutcome) {
        let mut stdout = String::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match rx.try_recv() {
                Ok(CodeRunProgress::StdoutChunk(chunk)) => stdout.push_str(&chunk),
                Ok(CodeRunProgress::StderrChunk(_)) => {}
                Ok(CodeRunProgress::Finished(outcome)) => return (stdout, outcome),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        panic!("run did not finish within timeout");
    }

    #[test]
    fn resolve_inline_code_runner_returns_bash_shell_spec() {
        let spec = resolve_inline_code_runner();
        assert_eq!(spec.program, "bash");
        assert_eq!(spec.extension, "sh");
    }

    #[test]
    fn extract_inline_code_source_trims_outer_whitespace() {
        let text = "run echo hello now";
        assert_eq!(
            extract_inline_code_source(text, &(4..15)),
            "echo hello"
        );
    }

    #[test]
    fn extract_inline_code_source_preserves_internal_spacing() {
        let text = "a b c";
        assert_eq!(extract_inline_code_source(text, &(2..5)), "b c");
    }

    #[test]
    fn extract_inline_code_source_clamps_out_of_range_bounds() {
        let text = "hello";
        assert_eq!(extract_inline_code_source(text, &(0..100)), "hello");
        assert_eq!(extract_inline_code_source(text, &(10..20)), "");
    }

    #[test]
    fn spawn_inline_shell_run_executes_echo_command() {
        let cancel = Arc::new(AtomicBool::new(false));
        let child_slot = Arc::new(std::sync::Mutex::new(None));
        let (tx, rx) = mpsc::unbounded();

        let join = spawn_inline_shell_run(
            "echo hello",
            Path::new("."),
            cancel,
            child_slot,
            tx,
        );

        let (stdout, outcome) = collect_run_outcome(rx);
        join.join().expect("inline shell run thread");

        assert!(stdout.contains("hello"), "stdout was: {stdout:?}");
        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.error_message.is_none());
    }

    #[test]
    fn spawn_inline_shell_run_truncates_large_output() {
        let cancel = Arc::new(AtomicBool::new(false));
        let child_slot = Arc::new(std::sync::Mutex::new(None));
        let (tx, rx) = mpsc::unbounded();

        let join = spawn_inline_shell_run(
            "python3 -c \"print('x' * 10000)\"",
            Path::new("."),
            cancel,
            child_slot,
            tx,
        );

        let (stdout, outcome) = collect_run_outcome(rx);
        join.join().expect("inline shell run thread");

        assert!(stdout.len() <= INLINE_CODE_RUN_MAX_OUTPUT_CHARS);
        assert!(outcome.stdout.len() <= INLINE_CODE_RUN_MAX_OUTPUT_CHARS);
    }
}
