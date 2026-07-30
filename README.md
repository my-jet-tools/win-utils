# win-utils

Shared Rust crates for the Windows side of the win-containers family. Public on
purpose: every consumer fetches these as a plain `git` dependency, and a private
repository would mean each consuming CI needs a credential just to compile a
library that holds no secrets.

## Crates

| crate | what it is |
|---|---|
| [`win-service-host`](./win-service-host) | Hosts a Rust binary as a native Windows service: `--install` / `--uninstall` / `--start` / `--run-as-service`, restart-on-failure, the CLI and usage text around them, and the `StartFn` / `ShutdownFn` lifecycle the service runs inside. Compiles everywhere — non-Windows gets stubs, so consumers still type-check and test off a Windows box. |

## Why it is its own repository

`win-service-host` used to live inside `win-containers-engine`, and
`win-containers-statistic-collector` depended on it by a git tag **on the engine's
repository** — a service depending on a sibling service's repo for a library. Two
consequences, both of which this repo removes:

- releasing a fix meant tagging the engine's repo and then bumping the dependency
  in the collector, in that order;
- the engine's repository is private, so the collector's CI could not fetch the
  crate at all: `actions/checkout` writes its `GITHUB_TOKEN` into the checked-out
  repository's local git config, which cargo's own clone of another repo never
  sees, and that token is scoped to one repository anyway. The collector's release
  build failed at `cargo build` for exactly that reason.

A third consumer exists: `release-mcp-windows-agent` carries its own copy of the
same service-hosting code. Collapsing it onto this crate is the remaining step.

## Using it

```toml
win-service-host = { tag = "win-service-host-0.1.2", git = "https://github.com/my-jet-tools/win-utils.git" }
```

Tags are per crate: `<crate>-<version>`, so crates here version independently.

Each binary supplies its own service description — `InstallSpec::description` is a
required `&str`, not an `Option`, because a shared host cannot invent a sensible
line for a service it knows nothing about, and a blank Description column in
services.msc tells whoever is looking at the machine nothing. `--description`
overrides whatever default the binary passes.
