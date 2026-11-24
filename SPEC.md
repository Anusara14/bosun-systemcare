# Bosun SystemCare: Product Spec (v0.1)

This is a clean-room spec. It was written from Advanced SystemCare 19.5's visible behaviour, its UI strings (`Language\English.lng`, 3,890 lines) and screenshots.
No IObit code, artwork, rule databases or text is reused. Feature names are paraphrased where needed.

## 1. Processes

| Binary | Runs | Job | Memory budget (private bytes) |
|---|---|---|---|
| `bosun-tray.exe` | Always (autostart, unelevated) | Tray icon, floating widget, Performance Monitor popup, launches the main window | < 10 MB at rest |
| `bosun.exe` | On demand (admin manifest), exits when closed | Main window, all scans and fixes, settings | < 30 MB while open |

The tray's floor is Direct2D's software renderer (WARP keeps ~6 MB of compiled shaders for the process lifetime); a GPU target would cost ~50 MB of driver. Measured: 9.9 MB median at rest, ~19 MB with the Performance Monitor open.

They talk over a named pipe (`\\.\pipe\bosun`). Messages: `Open(page)`, `TurboToggle`, `ScanNow`.
Shared code lives in the `bosun-core` crate: system operations, the rules engine and the change journal.

## 2. Screens and features

Legend: ✅ v1 · 🔁 replaced with an honest version · ⛔ skipped (reason given)

### 2.1 Care (home)
- Big animated **SCAN** ring button, plus an **AI Mode / Manual Mode** switch.
  - 🔁 "AI Mode" becomes **Recommended** (a preset of the safe modules). **Manual** lets the user tick modules.
- Scan modules (the Settings tree mirrors these):
  | Module | v1 | What it does |
  |---|---|---|
  | Junk File Clean | ✅ | Recycle Bin, Windows/user temp, Windows Update cache (`SoftwareDistribution\Download`), Delivery Optimization files, memory dumps, Windows Error Reports, log files, thumbnail cache, DirectX shader cache, font cache, chkdsk fragments, old prefetch, leftover installer temp files. Every item shows its size. |
  | Privacy Sweep | ✅ | Per browser (Edge, Chrome, Firefox, Opera, Brave, Vivaldi, Chromium-based): cache, cookies*, history*, download history, session. Windows: Recent Docs, Run MRU, jump lists, clipboard, DNS cache. *Not ticked by default.* |
  | Registry Clean | 🔁 | Only *provably* orphaned entries: uninstall keys whose install path is missing, App Paths pointing at missing exes, MUI cache, invalid file-extension handlers, startup entries pointing at missing files. Every fix is exported to a `.reg` file first. |
  | Shortcut Fixer | ✅ | `.lnk` files on Desktop and Start Menu whose target is missing. |
  | System Optimization | 🔁 | Only real, reversible tweaks (e.g. auto-restart Explorer, end hung apps faster, disable NTFS last-access). Each tweak shows its exact registry value. Placebo tweaks are dropped. |
  | Disk Optimization | ✅ | Runs `defrag /O` (trim on SSD, defrag on HDD) for the chosen drives. |
  | Security check | 🔁 | Reads Defender, Firewall and UAC status plus pending Windows updates. Reports and deep-links; we don't run our own anti-spyware engine. |
  | Internet Boost | ⛔ | TCP registry tweaks with no effect on Windows 10/11. |
  | Spyware Removal | ⛔ | Needs a malware engine and signatures; Defender does it. |
- Results page: a category list with item counts and sizes, expandable details, a "Fix" button, and a summary.

### 2.2 Speed Up
- Header banner: *"N startup items can be disabled or delayed."*
- **Turbo Boost** card: an ON/OFF dropdown with modes. Its config dialog has tabs: System Services · Non-Windows Services · Background Apps, plus a checkbox to switch to the Bosun power plan and one to turn on Turbo Boost at startup.
  - Implementation: change the power scheme (`PowerSetActiveScheme`), stop the chosen services (SCM) and close the chosen processes. Turning it off restores the previous state.
- **Startup Optimizer** card opens a window with 5 tabs:
  1. *Quick Optimization*: last boot time, items you can disable, and an Optimize button.
  2. *Startup Items*: Run/RunOnce keys (HKCU/HKLM, including Wow6432), Startup folders, and Store app startup tasks.
  3. *Services*: non-Microsoft services by default ("Show System Services" adds the rest).
  4. *Scheduled Tasks*: non-Microsoft tasks (via the `ITaskService` COM interface).
  5. *Browser Startups*: extensions for Edge, Chrome and Firefox (parsed from `Preferences` and `extensions.json`), with Remove.
  - Columns: Name · Boot time · Impact · Recommendation · Status (Enabled/Disabled/Delayed pill + dropdown).
  - 🔁 IObit's "Disable rate %" is their crowd data. Ours is **Impact** (measured from the boot-performance event log, Event ID 100/101), and the recommendation comes from an open `rules/startup.toml` (publisher signature + known-safe list).
  - Boot time: read from `Microsoft-Windows-Diagnostics-Performance/Operational` (needs admin).
- **Driver updates** card: 🔁 lists driver updates that Windows Update offers (`IUpdateSearcher`, driver type). No third-party driver installer.
- **App/Extension Cleaner** card: the uninstall list from the registry, sortable by size and install date, plus browser extensions.
- **Real-Time TuneUp** card: switches for Auto clean (scheduled), Auto RAM clean (🔁 opt-in, with an honest note that Windows already manages RAM), and Performance Monitor.
- **More tools** strip: Disk Cleaner, Large File Finder, Empty Folder Finder, Shortcut Fixer (v1.1).

### 2.3 Protect
- Banner showing overall protection status.
- **System Protection**: Defender and Firewall status (read via WMI `root/SecurityCenter2`), deep-linking to Windows Security.
- **Browser Protection**: Homepage Advisor ✅ watches the default browser's homepage and search engine and warns when they change. ⛔ ad removal / surfing protection: that needs a filter-list engine; deferred.
- ⛔ Safe Folder, Fingerprint Protection, Anti-Tracking, VPN: upsells.

### 2.4 Software Updater
- List: icon · name · current version → new version · Update button. Plus an **Update All** button (free in our version).
- Source: the **winget** COM API (`Microsoft.Management.Deployment`), falling back to `winget upgrade --include-unknown` output.

### 2.5 Action Center
- ⛔ In IObit this is cross-promotion. Replaced by an **About / Logs** page.

### 2.6 Rescue Center
- Every change goes to an append-only journal: `%ProgramData%\Bosun\journal\<timestamp>.json` with the previous state (registry value, service start type, file moved to the Recycle Bin, task state).
- Restore any journal entry. Optionally create a System Restore point before big fixes (`SRSetRestorePoint`).

### 2.7 Performance Monitor (from the tray)
- Popup of about 520×390. A header row of 5 tabs: RAM %, CPU %, Disk %, Metrics, Network. The first three are ring gauges.
- RAM tab: list of processes with high RAM use (icon, name, RAM, grouped by exe), plus "Recently closed" and "Background" sections, a checkbox per row, and **End Task**.
- CPU and Disk tabs: the same list, sorted by CPU % and disk I/O.
- Network tab: upload/download per adapter (`GetIfTable2`).
- Footer tools: Clean RAM (🔁 trims working sets via `EmptyWorkingSet`, labelled honestly), Scan, Turbo toggle, Screenshot (⛔ v1).
- Data: `NtQuerySystemInformation(SystemProcessInformation)` for one-shot process snapshots, `GetSystemTimes`, `GlobalMemoryStatusEx`, PDH for disk. 1 s refresh while visible, 0 while hidden.

### 2.8 Floating widget
- Pill of about 270×40 px, always on top, layered window (`UpdateLayeredWindow`), draggable, snaps to screen edges, remembers its position.
- Content: [Bosun tray mark] RAM: 54% CPU: 11% [alert badge] [→ open Monitor].
- Right-click menu: Hide widget · Open Bosun · Turbo Boost on/off · Exit.
- Refresh every 2 s. Uses no GPU while it isn't changing.

### 2.9 Settings (dialog)
Tree: General · Clean & Optimize (one page per module) · Automatic Service (Auto clean schedule) · Auto RAM Clean · Ignore List · Logs & Backup · Notifications.
General: Minimize to tray on close · Start tray at Windows startup · Enable Performance Monitor at startup · Language · Theme follows system accent.
⛔ "Skip UAC warning" (a security downgrade). ⛔ Shell context-menu extension (v2; would use static registry verbs, not a COM shell extension).

### 2.10 Tray icon menu
Open Bosun · Scan now · Turbo Boost · Performance Monitor · Show widget · Settings · Exit.

## 3. Visual design

### 3.1 Tokens (Windows 11 dark, charcoal)
| Token | Value |
|---|---|
| `bg.window` | `#202020` |
| `bg.sidebar` | `#1C1C1C` |
| `bg.card` | `#2B2B2B` |
| `bg.card.hover` | `#323232` |
| `bg.card.pressed` | `#272727` |
| `bg.nav.selected` | `#2D2D2D` + 3 px accent pill on the left |
| `stroke.card` | `#FFFFFF` at 6% |
| `stroke.divider` | `#FFFFFF` at 8% |
| `text.primary` | `#FFFFFF` |
| `text.secondary` | `#C5C5C5` |
| `text.tertiary` | `#9D9D9D` |
| `accent` | `#60CDFF` (button fill) |
| `accent.text-on` | `#000000` |
| `accent.ring` | `#1E9CFF` (scan ring, gauges; the brand ring blue) |
| `status.ok` / `warn` / `bad` | `#6CCB5F` / `#FCE100` / `#FF99A4` |
| `attention` | `#F7A600` (the "N items" count highlight and the info circle) |
| Font | Segoe UI Variable (Display ≥ 20 px, Text below) |
| Type scale | Title 20/28 semibold · Subtitle 16/22 semibold · Body 14/20 · Caption 12/16 |
| Radius | Window 8 · card 8 · button 4 · pill/toggle 999 · nav item 6 |
| Spacing | 4-px grid; card padding 20; gap between cards 12; page padding 24 |
| Motion | 150 ms ease-out for hover and press; scan ring spin 1.6 s linear; pages crossfade 120 ms |

### 3.2 Layout (from the reference screenshots, adjusted to Windows 11)
- **Main window** 960×660 by default, resizable down to 900×620. Custom title bar 40 px with logo + "Bosun SystemCare", plus bell, menu, minimize, maximize and close buttons. The window uses Mica on Windows 11 and a solid background elsewhere.
- **Sidebar** 148 px wide. Nav items are 132×84: a 24 px icon over a 12 px label. The selected item gets a card background. Items: Care, Speed Up, Protect, Software Updater, About. The bottom area shows the version, with no "Activate" button.
- **Care**: SCAN ring 240 px in the centre of the content area. The segmented Recommended | Manual switch sits 32 px below it.
- **Speed Up / Protect**: a status banner (warning icon + title 20 px + one subtitle line), then a 2-column card grid with 12 px gaps. Each card has an icon, title, description and a right-aligned primary button.
- **Software Updater**: a banner with Update All on the right, and a list of rows 72 px tall.
- **Startup Optimizer window**: 980×680 with a tab bar of 5 tabs, a table with sticky header, row height 60, and a footer with Rescue Center (left) and Refresh (right).
- **Performance Monitor**: 520×390 with a header of 5 tab tiles (ring gauges at 56 px), a table, and a footer toolbar of icon buttons with End Task on the right.
- **Widget**: 270×40 with radius 20, `bg` `#1C1C1C` at 92% opacity, a 1 px stroke at 8% white, and 13 px text.

### 3.3 Icons
- Interface icons come from **Fluent UI System Icons** (MIT), Regular style, 20/24 px.
- The app icon and tray icon come from `brand/export/ico/`.

## 4. Safety rules (non-negotiable)
1. Nothing destructive happens without a journal entry written first.
2. Files are moved to the Recycle Bin (`IFileOperation` with `FOFX_RECYCLEONDELETE`), except for caches, temp files and Windows Update downloads.
3. Every scan defaults to a dry run. Tests run on a Windows VM before any fix ships.
4. Never touch: anything signed by Microsoft in Services or Tasks (unless "Show System Services" is on and the user confirms), `HKLM\SYSTEM\CurrentControlSet\Control`, or drivers.
5. No telemetry. No network access except winget, Windows Update and the update check.

## 5. Non-goals
Malware scanning, a VPN, a driver installer, a "registry defrag", product promotions, license nags.
