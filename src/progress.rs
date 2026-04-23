//! Progress-reporting types. The `Installer` owns a `ProgressState` and an
//! optional `ProgressSink`; see `crate::Installer` for the full API.

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
