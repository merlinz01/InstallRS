//! Progress-reporting types and the `Installer` methods that drive them.

use anyhow::Result;

use crate::Installer;

/// Sink for progress, status, and log events emitted by installer operations.
///
/// Attach one to an [`crate::Installer`] with
/// [`crate::Installer::set_progress_sink`] (the wizard GUI does this
/// automatically inside the install page).
pub trait ProgressSink: Send + Sync {
    fn set_status(&self, status: &str);
    fn set_progress(&self, fraction: f64);
    fn log(&self, message: &str);
}

/// Default text-based progress sink used by non-GUI installs. Writes
/// status and log lines to stderr; renders an in-place progress bar when
/// stderr is a TTY, otherwise emits no progress lines.
///
/// Attached automatically by [`Installer::install_main`] /
/// [`Installer::uninstall_main`] when no other sink is set, so headless
/// installers get readable output for free.
pub struct StderrProgressSink {
    is_tty: bool,
    state: std::sync::Mutex<StderrSinkState>,
}

#[derive(Default)]
struct StderrSinkState {
    last_status: String,
    last_fraction: f64,
    progress_line_active: bool,
}

impl StderrProgressSink {
    pub fn new() -> Self {
        use std::io::IsTerminal;
        Self {
            is_tty: std::io::stderr().is_terminal(),
            state: std::sync::Mutex::new(StderrSinkState::default()),
        }
    }

    fn redraw(&self, state: &mut StderrSinkState) {
        use std::io::Write;
        if !self.is_tty {
            return;
        }
        let pct = (state.last_fraction * 100.0).round() as u32;
        let bar_width = 24usize;
        let filled = ((state.last_fraction * bar_width as f64).round() as usize).min(bar_width);
        let bar: String = "#".repeat(filled) + &"-".repeat(bar_width - filled);
        let mut out = std::io::stderr().lock();
        let _ = write!(out, "\r[{bar}] {pct:>3}% {}\x1b[K", state.last_status);
        let _ = out.flush();
        state.progress_line_active = true;
    }

    fn clear_progress_line(&self, state: &mut StderrSinkState) {
        use std::io::Write;
        if !self.is_tty || !state.progress_line_active {
            return;
        }
        let mut out = std::io::stderr().lock();
        let _ = write!(out, "\r\x1b[2K");
        let _ = out.flush();
        state.progress_line_active = false;
    }
}

impl Default for StderrProgressSink {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StderrProgressSink {
    fn drop(&mut self) {
        use std::io::Write;
        let mut state = self.state.lock().unwrap();
        if self.is_tty && state.progress_line_active {
            let mut out = std::io::stderr().lock();
            let _ = writeln!(out);
            let _ = out.flush();
            state.progress_line_active = false;
        }
    }
}

impl ProgressSink for StderrProgressSink {
    fn set_status(&self, status: &str) {
        let mut state = self.state.lock().unwrap();
        state.last_status = status.to_string();
        if self.is_tty {
            self.redraw(&mut state);
        } else {
            self.clear_progress_line(&mut state);
            eprintln!("[*] {status}");
        }
    }
    fn set_progress(&self, fraction: f64) {
        let mut state = self.state.lock().unwrap();
        state.last_fraction = fraction.clamp(0.0, 1.0);
        if self.is_tty {
            self.redraw(&mut state);
        }
    }
    fn log(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        self.clear_progress_line(&mut state);
        eprintln!("    {message}");
        if self.is_tty {
            self.redraw(&mut state);
        }
    }
}

pub(crate) struct ProgressState {
    /// Weighted step cursor. Each operation advances this by its weight (default 1).
    /// `f64` so in-flight updates (`set_step_progress`) can land anywhere in
    /// the current step's range.
    pub(crate) steps_done: f64,
    /// Start of the current step's weight range — the cursor when the step opened.
    pub(crate) step_range_start: f64,
    /// End of the current step's weight range — cursor + weight.
    pub(crate) step_range_end: f64,
}

impl Installer {
    /// Total step weight across all currently-selected components. Every
    /// builder op (and `step`/`begin_step`) advances a cursor that runs from
    /// 0 up to this total; the progress sink receives `cursor / total` on
    /// each update.
    pub fn total_steps(&self) -> u64 {
        self.components
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.progress_weight as u64)
            .sum()
    }

    /// Reset the step cursor to zero. Usually not needed — `install_main`
    /// leaves it alone and the wizard's install page runs exactly once.
    pub fn reset_progress(&mut self) {
        let mut state = self.progress.lock().unwrap();
        state.steps_done = 0.0;
        state.step_range_start = 0.0;
        state.step_range_end = 0.0;
    }

    /// Open a weighted step without running a builder op. Use this around
    /// your own long-running work (downloads, service registration, etc.) so
    /// the progress bar advances; pair with [`Installer::set_step_progress`]
    /// for sub-step updates, then [`Installer::end_step`] to close it out.
    ///
    /// ```rust,ignore
    /// i.begin_step("Downloading", 5);
    /// for (done, total) in download_chunks(&url)? {
    ///     i.set_step_progress(done as f64 / total as f64);
    /// }
    /// i.end_step();
    /// ```
    pub fn begin_step(&self, status: &str, weight: u32) {
        self.emit_status(&Some(status.to_string()));
        let mut state = self.progress.lock().unwrap();
        state.step_range_start = state.steps_done;
        state.step_range_end = state.steps_done + weight as f64;
        drop(state);
        self.emit_progress();
    }

    /// Update progress within the currently-open step: `fraction` is
    /// interpreted as a position from 0.0 (step start) to 1.0 (step end).
    /// Outside an open step this is a no-op. Clamped to `[0, 1]`.
    pub fn set_step_progress(&self, fraction: f64) {
        let f = fraction.clamp(0.0, 1.0);
        let mut state = self.progress.lock().unwrap();
        let span = state.step_range_end - state.step_range_start;
        state.steps_done = state.step_range_start + f * span;
        drop(state);
        self.emit_progress();
    }

    /// Close the currently-open step, jumping the cursor to the end of its
    /// range. If no step is open this just snaps the cursor to the previous
    /// `step_range_end`.
    pub fn end_step(&self) {
        let mut state = self.progress.lock().unwrap();
        state.steps_done = state.step_range_end;
        state.step_range_start = state.step_range_end;
        drop(state);
        self.emit_progress();
    }

    /// One-shot equivalent of `begin_step` + `end_step` for user code whose
    /// progress can't be subdivided: advances the cursor by `weight` units
    /// and emits the status message.
    pub fn step(&self, status: &str, weight: u32) {
        self.begin_step(status, weight);
        self.end_step();
    }

    pub(crate) fn emit_progress(&self) {
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        let state = self.progress.lock().unwrap();
        let total = self.total_steps();
        let fraction = if total == 0 {
            0.0
        } else {
            (state.steps_done / total as f64).clamp(0.0, 1.0)
        };
        drop(state);
        sink.set_progress(fraction);
    }

    /// Open a step with the given weight, run `f`, then close the step.
    /// Used by builder ops' `.install()` to wrap the actual work in a
    /// progress step. If `f` errors, the step is still closed.
    pub(crate) fn run_weighted_step<F>(&self, weight: u32, f: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        {
            let mut state = self.progress.lock().unwrap();
            state.step_range_start = state.steps_done;
            state.step_range_end = state.steps_done + weight as f64;
        }
        self.emit_progress();
        let result = f();
        {
            let mut state = self.progress.lock().unwrap();
            state.steps_done = state.step_range_end;
            state.step_range_start = state.step_range_end;
        }
        self.emit_progress();
        result
    }
}
