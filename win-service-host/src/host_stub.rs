#![cfg(not(windows))]

//! Non-Windows stubs so the services type-check and run their tests off a
//! Windows box. The service modes themselves only exist on Windows.

use crate::{InstallSpec, StartFn};

pub fn install(_spec: InstallSpec<'_>) -> Result<(), String> {
    Err("Windows-service modes only exist on Windows".to_string())
}

pub fn uninstall(_name: &str) -> Result<(), String> {
    Err("Windows-service modes only exist on Windows".to_string())
}

pub fn start(_name: &str) -> Result<(), String> {
    Err("Windows-service modes only exist on Windows".to_string())
}

pub fn run_as_service(_name: String, _start: StartFn) -> Result<(), String> {
    Err("Windows-service modes only exist on Windows".to_string())
}
