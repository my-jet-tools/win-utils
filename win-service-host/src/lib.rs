//! Shared Windows-service hosting for the win-containers services — the same
//! machinery release-mcp-windows-agent carries, extracted so the engine and
//! the statistic collector do not each re-implement it.
//!
//! `install` registers the exe with the Service Control Manager: auto-start at
//! boot, restart-on-failure (10s after every failure, for as long as it
//! fails), description, and a `binPath` that re-invokes the same exe with
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

/// The account the installed service runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceAccount {
    /// `LocalSystem`: everything on the machine is open to it. The default, as it always was.
    #[default]
    LocalSystem,
    /// `NT AUTHORITY\LocalService`: the rights of a plain user, and nobody on the network.
    LocalService,
    /// `NT AUTHORITY\NetworkService`: the rights of a plain user, the machine on the network.
    NetworkService,
    /// `NT SERVICE\<service name>`: an account of this one service, made by Windows with the
    /// service and gone with it. The rights of a plain user, and files can be granted to it
    /// alone - which is what makes it the account for a service facing the network.
    Virtual,
}

impl ServiceAccount {
    /// The value of `--account`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local-system" => Some(Self::LocalSystem),
            "local-service" => Some(Self::LocalService),
            "network-service" => Some(Self::NetworkService),
            "virtual" => Some(Self::Virtual),
            _ => None,
        }
    }

    /// The account name the SCM takes for the service `service_name`; `None` is LocalSystem.
    pub fn account_name(&self, service_name: &str) -> Option<String> {
        match self {
            Self::LocalSystem => None,
            Self::LocalService => Some(r"NT AUTHORITY\LocalService".to_string()),
            Self::NetworkService => Some(r"NT AUTHORITY\NetworkService".to_string()),
            Self::Virtual => Some(format!(r"NT SERVICE\{service_name}")),
        }
    }
}

/// Everything `install` needs to know about the service being registered.
pub struct InstallSpec<'a> {
    /// The account the service runs as. Each binary decides its default, and lets `--account`
    /// override it.
    pub account: ServiceAccount,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_of_a_service() {
        assert_eq!(
            ServiceAccount::parse("virtual"),
            Some(ServiceAccount::Virtual)
        );
        assert_eq!(ServiceAccount::parse("root"), None);
        assert_eq!(ServiceAccount::default(), ServiceAccount::LocalSystem);

        assert_eq!(ServiceAccount::LocalSystem.account_name("proxy"), None);
        assert_eq!(
            ServiceAccount::Virtual.account_name("proxy").as_deref(),
            Some(r"NT SERVICE\proxy")
        );
        assert_eq!(
            ServiceAccount::LocalService
                .account_name("proxy")
                .as_deref(),
            Some(r"NT AUTHORITY\LocalService")
        );
    }
}
