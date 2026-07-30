//! Shared Windows-service hosting for the win-containers services — the same
//! machinery release-mcp-windows-agent carries, extracted so the engine and
//! the statistic collector do not each re-implement it.
//!
//! `install` registers the exe with the Service Control Manager: auto-start at
//! boot, restart-on-failure (3 attempts, 10s apart, counter reset daily),
//! description, and a `binPath` that re-invokes the same exe with
//! `--run-as-service`. That flag matters: a service exe must speak the SCM
//! protocol (ServiceMain + control handler) or Windows kills it with error
//! 1053 — the whole reason wrappers like WinSW existed.
//!
//! Everything compiles on non-Windows too (stubs), which is enough to
//! type-check and test the services off a Windows box.

pub mod cli;

use std::future::Future;
use std::pin::Pin;

pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What `run_as_service` calls after the SCM says "stop": an orderly shutdown
/// of whatever `StartFn` brought up.
pub type ShutdownFn = Box<dyn FnOnce() -> BoxFut<()> + Send>;

/// Brings the application up inside the service's tokio runtime and hands
/// back the shutdown routine. Runs between `StartPending` and `Running`.
pub type StartFn = Box<dyn FnOnce() -> BoxFut<Result<ShutdownFn, String>> + Send>;

#[cfg(windows)]
mod host_windows;
#[cfg(windows)]
pub use host_windows::{install, run_as_service, start, uninstall};

#[cfg(not(windows))]
mod host_stub;
#[cfg(not(windows))]
pub use host_stub::{install, run_as_service, start, uninstall};

/// Everything `install` needs to know about the service being registered.
pub struct InstallSpec<'a> {
    /// Service name in the SCM. Also what --uninstall/--start address.
    pub name: &'a str,
    /// What services.msc shows in its Description column.
    ///
    /// REQUIRED rather than `Option`, and that is the point: a shared host cannot invent a
    /// sensible line for a service it knows nothing about, so the decision belongs to each
    /// binary — and making it a type obligation means no service can be registered with a
    /// blank description by forgetting to pass one. That is exactly how it went blank on
    /// WIN-RUN-01: the description was set only when `--description` was given, so the one
    /// line explaining what a process is went missing whenever nobody typed it.
    ///
    /// Callers pass their own default and let `--description` override it.
    pub description: &'a str,
    /// Explicit --settings path; baked into the binPath as an absolute path,
    /// because a service has no useful HOME to find a dotfile in.
    pub settings: Option<&'a str>,
    /// Fallback when no explicit settings path was given: `settings.yaml`
    /// next to the exe, when it exists at install time.
    pub settings_near_exe: Option<std::path::PathBuf>,
}
