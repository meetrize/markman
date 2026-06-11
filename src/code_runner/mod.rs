//! Code-block execution for supported scripting languages.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use futures::channel::mpsc::UnboundedSender;
use uuid::Uuid;

/// Snapshot of one code block's run state, synced to blocks for rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeBlockRunSnapshot {
    pub status: CodeRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_expanded: bool,
    pub error_message: Option<String>,
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

/// Spawns a background thread that executes `source` and streams output through `tx`.
pub fn spawn_code_run(
    language: &str,
    source: &str,
    work_dir: &Path,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
    tx: UnboundedSender<CodeRunProgress>,
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
    let mut readers = Vec::new();

    if let Some(stdout) = stdout {
        let tx = tx.clone();
        let cancel = cancel.clone();
        let collected = collected_stdout.clone();
        readers.push(thread::spawn(move || {
            read_stream(stdout, false, &tx, &cancel, collected);
        }));
    }
    if let Some(stderr) = stderr {
        let tx = tx.clone();
        let cancel = cancel.clone();
        let collected = collected_stderr.clone();
        readers.push(thread::spawn(move || {
            read_stream(stderr, true, &tx, &cancel, collected);
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
