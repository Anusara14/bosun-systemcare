# Bosun SystemCare

A small, honest PC care app for Windows 10/11, written in Rust. It cleans junk and privacy traces, fixes provably broken entries, manages startup items, keeps software up to date, and keeps a journal of every change so the Rescue Center can undo it.

The design is a clean-room take on the classic "system care" layout ([SPEC.md](SPEC.md)). Features that are placebo or upsell in other tools are either replaced with something real or left out, and the spec says which.

## What's in 0.2.0

- **Care:** one SCAN over nine modules: Privacy Sweep, Junk Files, Shortcut Fixer, Registry Clean, Security check (Defender, Firewall), System Optimization, Disk Optimization, Windows Update. Recommended or Manual mode. Every Fix is journaled first, with an optional restore point.
- **Speed Up:** Turbo Boost (power plan, services, background apps; can start with Windows), and a Startup Optimizer for startup items, services, scheduled tasks and browser extensions, with Boot time and Impact columns. Also App Cleaner, Driver Updates through Windows Update, and More Optimization Tools.
- **Protect:** Windows Security status and the Homepage Advisor.
- **Software Updater:** updates through winget, with an Ignore List.
- **Rescue Center:** browse and restore journaled changes.
- **Tray:** a floating widget, the Performance Monitor (with Recently closed), Auto RAM Clean, Auto Clean once a day, and notifications. The bell in the main window lists past notifications.
- **Look:** dark charcoal by default, plus a light skin.
- **Memory:** the tray uses about 10 MB at rest. The main window closes completely when you close it.

## Install

Download `BosunSystemCare-Setup-0.2.0.exe` and run it. It installs for the current user, with no administrator rights:

- Files go in `%LOCALAPPDATA%\Programs\Bosun SystemCare`.
- It adds a Start menu shortcut and an "Apps & features" entry.
- The tray starts at sign-in. You can turn that off in Settings.

To uninstall, use Apps & features, or run `uninstall.exe` in that folder. It asks whether to keep your settings and the Rescue Center journal; keeping them is the default.

The installer is not code-signed yet, so Windows SmartScreen will warn the first time ("More info" → "Run anyway"). Check the download against the `.sha256` file published next to it.

## Build

You need Rust (stable, edition 2024) and the MSVC build tools.

```powershell
cargo build --release          # bosun.exe + bosun-tray.exe in target\release
cargo test --workspace
powershell -File scripts\release.ps1   # dist\BosunSystemCare-Setup-<version>.exe
```

The workspace is split into four crates:

- `bosun-core`: system operations and the change journal.
- `bosun-ui`: Direct2D drawing.
- `bosun-tray`: the tray.
- `bosun-app`: the main window.

`bosun-setup` is the installer, which embeds the two release executables.

## Safety

- Nothing destructive happens without a journal entry first. Files go to the Recycle Bin, except caches and temp files.
- Microsoft-signed services and tasks, `HKLM\SYSTEM\CurrentControlSet\Control`, and drivers are never touched.
- No telemetry. The only network use is winget and Windows Update.

## Known limits

- **Boot time and Impact:** these are read from Windows' own startup measurements, which needs one elevated read. On the test machine that read didn't produce a result yet, so the columns may show "Not measured".
- **Testing:** fixes were exercised with dry runs and on the development PC, not yet on a clean VM.
- **Out of scope:** malware scanning, ad blocking, and a third-party driver installer (see SPEC §5).

## Release notes

### 0.2.0 (2026-11-25)
First installable release.

- **New:** an installer and uninstaller.
- **Tray:** full menu, light skin, notifications (with a list on the bell), Auto Clean, Turbo Boost at startup, Recently closed, and memory at rest cut from about 14 MB to about 10 MB.
- **Care:** System Optimization, Disk Optimization and Windows Update modules, plus an optional restore point before Fix.
- **Speed Up:** an Ignore List for startup items and software updates, and Boot time and Impact columns.

### 0.1.0
Development builds: the main window, Care scan, Speed Up, Protect, Software Updater, Rescue Center, and the tray with its widget and Performance Monitor.
