//! The command line every win-containers service shares:
//!
//! ```text
//! <exe>                          run in the console
//! <exe> --install                register as a Windows service
//!       [--name <service>] [--description <text>] [--settings <file>]
//! <exe> --start [--name <s>]     start the installed service
//! <exe> --uninstall [--name <s>] stop and remove the service
//! ```
//!
//! One place also decides which settings file everything reads: the explicit
//! flag becomes the env var the rest of the code honors.

use std::path::PathBuf;

pub enum Mode {
    /// Plain console run — what `cargo run` and smoke tests use.
    Console,
    /// Register with the Service Control Manager and exit.
    Install,
    /// Stop (when running) and delete the service, then exit.
    Uninstall,
    /// Start the installed service, then exit.
    Start,
    /// The mode the installed binPath invokes: speak the SCM protocol.
    RunAsService,
}

pub struct CliArgs {
    pub mode: Mode,
    /// Service name for install/uninstall/start/run-as-service.
    pub name: String,
    pub description: Option<String>,
    pub settings: Option<String>,
}

pub fn parse_args(bin_name: &str, default_service_name: &str) -> Result<CliArgs, String> {
    let mut mode = Mode::Console;
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut settings: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--install" => mode = Mode::Install,
            "--uninstall" => mode = Mode::Uninstall,
            "--start" => mode = Mode::Start,
            "--run-as-service" => mode = Mode::RunAsService,
            "--name" => name = Some(required_value(&mut args, "--name", bin_name)?),
            "--description" => {
                description = Some(required_value(&mut args, "--description", bin_name)?)
            }
            "--settings" => settings = Some(required_value(&mut args, "--settings", bin_name)?),
            other => return Err(usage(bin_name, &format!("unknown argument '{other}'"))),
        }
    }

    Ok(CliArgs {
        mode,
        name: name.unwrap_or_else(|| default_service_name.to_string()),
        description,
        settings,
    })
}

fn required_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    bin_name: &str,
) -> Result<String, String> {
    args.next()
        .filter(|v| !v.starts_with("--"))
        .ok_or_else(|| usage(bin_name, &format!("{flag} needs a value")))
}

pub fn usage(bin_name: &str, problem: &str) -> String {
    format!(
        "{problem}\n\n\
         Usage:\n  \
         {bin_name}.exe                         run in the console\n  \
         {bin_name}.exe --install               register as a Windows service\n      \
         [--name <service>] [--description <text>] [--settings <file>]\n      \
         (auto-start at boot, restart-on-failure; settings default to settings.yaml next to the exe)\n  \
         {bin_name}.exe --start [--name <service>]      start the installed service\n  \
         {bin_name}.exe --uninstall [--name <service>]  stop and remove the service"
    )
}

/// Where the settings live: the env var, then `settings.yaml` next to the exe
/// (the natural layout for an installed service, which has no useful HOME),
/// then the home-dir dotfile.
pub fn resolve_settings_file(env_var: &str, default_file: &str) -> String {
    if let Ok(path) = std::env::var(env_var)
        && !path.trim().is_empty()
    {
        return path;
    }

    if let Some(path) = resolve_settings_file_near_exe() {
        return path.to_string_lossy().into_owned();
    }

    default_file.to_string()
}

pub fn resolve_settings_file_near_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("settings.yaml");
    candidate.is_file().then_some(candidate)
}
