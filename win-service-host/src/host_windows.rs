#![cfg(windows)]

use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use crate::{InstallSpec, ShutdownFn, StartFn};

/// The exit code of a service whose body failed - in practice, whose `StartFn` returned an
/// error. Not `NO_ERROR`: a service which stops with it has stopped cleanly as far as the SCM
/// knows, so nothing records the failure and no failure action runs.
const BODY_FAILED: ServiceExitCode = ServiceExitCode::ServiceSpecific(1);

/// A windows-service error with what the Win32 call said. The crate's own text for a failed call
/// is "IO error in winapi call" and nothing more, so "no such service" (1060), "it exists
/// already" (1073) and "access denied" (5) read the same; the io::Error inside carries the
/// system message and the code - "... (os error 1060)".
fn describe(error: windows_service::Error) -> String {
    match error {
        windows_service::Error::Winapi(io) => io.to_string(),
        other => other.to_string(),
    }
}

pub fn install(spec: InstallSpec<'_>) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("can not determine the path of this exe: {e}"))?;

    let mut launch_arguments: Vec<OsString> =
        vec!["--run-as-service".into(), "--name".into(), spec.name.into()];

    // Freeze the settings location now: what the operator sees at install
    // time is what the service will read at boot time.
    let settings_path = match spec.settings {
        Some(path) => Some(
            std::fs::canonicalize(path)
                .map_err(|e| format!("settings file '{path}' is not readable: {e}"))?,
        ),
        None => spec.settings_near_exe.clone(),
    };
    if let Some(path) = settings_path.as_ref() {
        launch_arguments.push("--settings".into());
        launch_arguments.push(path.into());
    }

    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| {
        format!(
            "can not open the Service Control Manager (run elevated?): {}",
            describe(e)
        )
    })?;

    let info = ServiceInfo {
        name: OsString::from(spec.name),
        display_name: OsString::from(spec.name),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments,
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    let service = manager
        .create_service(
            &info,
            ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("can not create service '{}': {}", spec.name, describe(e)))?;

    service.set_description(spec.description).map_err(|e| {
        format!(
            "service created, but setting the description failed: {}",
            describe(e)
        )
    })?;

    // A restart 10s after every failure, with no limit: the SCM repeats the
    // last action of the list for every failure past its end, so the three
    // entries are not three attempts. The reset period only sets back the
    // count the event log reports ("It has done this N time(s)").
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(10),
                };
                3
            ]),
        })
        .map_err(|e| {
            format!(
                "service created, but setting restart-on-failure failed: {}",
                describe(e)
            )
        })?;

    // Also restart when the service stops with an error on its own - a body
    // which failed, see `BODY_FAILED` - not only when it dies without
    // reporting SERVICE_STOPPED.
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|e| {
            format!(
                "service created, but enabling non-crash recovery failed: {}",
                describe(e)
            )
        })?;

    println!(
        "Service '{}' installed (auto-start, restart-on-failure).",
        spec.name
    );
    match settings_path {
        Some(path) => println!("Settings: {}", path.display()),
        None => println!(
            "WARNING: no settings file was resolved at install time; the service will look for \
             settings.yaml next to the exe at start"
        ),
    }
    println!("Start it with:  sc start {}   (or --start)", spec.name);
    Ok(())
}

/// Stops (when running) and deletes the service.
pub fn uninstall(name: &str) -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| {
            format!(
                "can not open the Service Control Manager (run elevated?): {}",
                describe(e)
            )
        })?;

    let service = manager
        .open_service(
            name,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("can not open service '{name}': {}", describe(e)))?;

    let status = service
        .query_status()
        .map_err(|e| format!("can not query service '{name}': {}", describe(e)))?;

    if status.current_state != ServiceState::Stopped {
        println!("Stopping '{name}' ...");
        service
            .stop()
            .map_err(|e| format!("can not stop service '{name}': {}", describe(e)))?;

        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(500));
            let status = service
                .query_status()
                .map_err(|e| format!("can not query service '{name}': {}", describe(e)))?;
            if status.current_state == ServiceState::Stopped {
                break;
            }
        }
    }

    service
        .delete()
        .map_err(|e| format!("can not delete service '{name}': {}", describe(e)))?;

    println!("Service '{name}' uninstalled.");
    Ok(())
}

pub fn start(name: &str) -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| {
            format!(
                "can not open the Service Control Manager (run elevated?): {}",
                describe(e)
            )
        })?;
    let service = manager
        .open_service(name, ServiceAccess::START)
        .map_err(|e| format!("can not open service '{name}': {}", describe(e)))?;
    service
        .start::<&std::ffi::OsStr>(&[])
        .map_err(|e| format!("can not start service '{name}': {}", describe(e)))?;
    println!("Service '{name}' started.");
    Ok(())
}

// ------------------------------------------------------------- service mode

/// Set before the dispatcher starts; the SCM calls `ffi_service_main` on its
/// own thread, so parameters travel through a static.
static SERVICE_RUN: OnceLock<Mutex<Option<(String, StartFn)>>> = OnceLock::new();

windows_service::define_windows_service!(ffi_service_main, service_main);

/// Blocks for the whole service lifetime (the SCM dispatcher owns the process
/// from here). `start` is called inside the service's tokio runtime between
/// `StartPending` and `Running`; the shutdown routine it returns runs when the
/// SCM says stop.
pub fn run_as_service(name: String, start: StartFn) -> Result<(), String> {
    let _ = SERVICE_RUN.set(Mutex::new(Some((name.clone(), start))));
    service_dispatcher::start(name, ffi_service_main)
        .map_err(|e| format!("service dispatcher failed: {}", describe(e)))
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        my_logger::LOGGER.write_error(
            "win_service",
            format!("service run failed: {error}"),
            my_logger::LogEventCtx::new(),
        );
    }
}

fn run_service() -> Result<(), String> {
    let (name, start) = SERVICE_RUN
        .get()
        .and_then(|slot| slot.lock().ok()?.take())
        .ok_or_else(|| "service body was not prepared before the dispatcher".to_string())?;

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();

    let status_handle = service_control_handler::register(&name, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
            let _ = shutdown_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(|e| {
        format!(
            "can not register the service control handler: {}",
            describe(e)
        )
    })?;

    let set_status = |state: ServiceState, wait_hint: Duration, exit_code: ServiceExitCode| {
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code,
            checkpoint: 0,
            wait_hint,
            process_id: None,
        });
    };
    let set_state = |state: ServiceState, wait_hint: Duration| {
        set_status(state, wait_hint, ServiceExitCode::NO_ERROR)
    };

    set_state(ServiceState::StartPending, Duration::from_secs(30));

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            set_status(ServiceState::Stopped, Duration::ZERO, BODY_FAILED);
            return Err(format!("can not build the tokio runtime: {e}"));
        }
    };

    let result = runtime.block_on(async {
        let shutdown: ShutdownFn = start().await?;

        set_state(ServiceState::Running, Duration::ZERO);

        // Parked until the SCM says stop; the blocking recv lives on the
        // blocking pool so the runtime keeps serving.
        let _ = tokio::task::spawn_blocking(move || shutdown_rx.recv()).await;

        set_state(ServiceState::StopPending, Duration::from_secs(30));

        shutdown().await;

        Ok::<(), String>(())
    });

    if let Err(error) = result.as_ref() {
        my_logger::LOGGER.write_error(
            "win_service",
            format!("service body failed: {error}"),
            my_logger::LogEventCtx::new(),
        );
    }

    // Give the logger a beat to flush before the process is torn down.
    runtime.shutdown_timeout(Duration::from_secs(5));

    // A failed body stops the service with an error, not as a clean stop: the SCM records it
    // (event 7024, "terminated with the following service-specific error", which Windows words
    // as the system message of code 1, "Incorrect function."), and as `install` turns on the
    // failure actions for non-crash failures, the service is restarted 10s later as after a
    // crash - which gets it past a start that failed on something passing, a port not yet free
    // at boot, and retries one which fails for good every 10s.
    let exit_code = match result {
        Ok(()) => ServiceExitCode::NO_ERROR,
        Err(_) => BODY_FAILED,
    };
    set_status(ServiceState::Stopped, Duration::ZERO, exit_code);
    Ok(())
}
