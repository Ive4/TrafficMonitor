# Rust fork publication

The Rust rewrite is published to [Ive4/TrafficMonitor](https://github.com/Ive4/TrafficMonitor), on branch `refactor/rust-migration-public`. The upstream project is [zhongyang219/TrafficMonitor](https://github.com/zhongyang219/TrafficMonitor); its source, resources, authorship and license are retained.

This branch contains the complete Rust source, compatibility fixtures, build scripts and verification summaries. Generated binaries, ZIP packages, local profiles, raw diagnostic logs and startup-repair backups are excluded from Git. Build a package using `scripts/build-rust-release.ps1`; the latest recorded verification is in `task-scheduler-acceptance.json`.

The latest local validation passed 267 default workspace tests per architecture, plus two explicit Task Scheduler regressions on each of x64 and x86. Release packages passed extraction/hash and real sampling checks. ARM64EC was compile-checked only. Real Windows 10/ARM devices, mixed-DPI setups, standard-user task creation and actual logon execution remain outside the verified scope.

Acceptance JSON files describe historical local validation snapshots. Their original base commit and uncommitted-at-validation flags refer to the time of those checks. Machine-specific paths and account identifiers have been generalized for publication; complete local evidence remains outside Git. Remote CI is a separate validation step and is not implied by these local results.
