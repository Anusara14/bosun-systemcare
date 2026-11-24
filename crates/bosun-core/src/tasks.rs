//! Scheduled tasks (spec §2.2, Startup Optimizer → Scheduled Tasks):
//! non-Microsoft tasks through the Task Scheduler COM API. Enabling and
//! disabling is journaled first; tasks owned by other accounts need admin.

use std::io::{self, Write};
use std::path::Path;

use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::BSTR;

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub name: String,
    /// Full path, e.g. `\GoogleSystem\GoogleUpdater\…`.
    pub path: String,
    pub enabled: bool,
}

fn service() -> Option<ITaskService> {
    // SAFETY: COM object creation and connection to the local scheduler.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let s: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).ok()?;
        let e = VARIANT::default();
        s.Connect(&e, &e, &e, &e).ok()?;
        Some(s)
    }
}

fn walk(folder: &ITaskFolder, out: &mut Vec<Task>) {
    // SAFETY: COM calls on objects returned by the scheduler; indices are 1-based.
    unsafe {
        let path = folder.Path().map(|p| p.to_string()).unwrap_or_default();
        if path.eq_ignore_ascii_case(r"\Microsoft") {
            return; // Windows' own tasks
        }
        if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
            for i in 1..=tasks.Count().unwrap_or(0) {
                let Ok(t) = tasks.get_Item(&VARIANT::from(i)) else {
                    continue;
                };
                out.push(Task {
                    name: t.Name().map(|n| n.to_string()).unwrap_or_default(),
                    path: t.Path().map(|p| p.to_string()).unwrap_or_default(),
                    enabled: t.Enabled().is_ok_and(|e| e.as_bool()),
                });
            }
        }
        if let Ok(subs) = folder.GetFolders(0) {
            for i in 1..=subs.Count().unwrap_or(0) {
                if let Ok(f) = subs.get_Item(&VARIANT::from(i)) {
                    walk(&f, out);
                }
            }
        }
    }
}

/// Every task outside `\Microsoft`, sorted by name.
pub fn list() -> Vec<Task> {
    let mut out = Vec::new();
    // SAFETY: root folder of a connected scheduler.
    if let Some(root) = service().and_then(|s| unsafe { s.GetFolder(&BSTR::from(r"\")) }.ok()) {
        walk(&root, &mut out);
    }
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

fn registered(path: &str) -> io::Result<IRegisteredTask> {
    let s = service().ok_or_else(|| io::Error::other("Task Scheduler unavailable"))?;
    let (dir, name) = path.rsplit_once('\\').unwrap_or(("", path));
    let dir = if dir.is_empty() { r"\" } else { dir };
    // SAFETY: looks the task up by its folder and name.
    unsafe {
        let f = s.GetFolder(&BSTR::from(dir)).map_err(io::Error::other)?;
        f.GetTask(&BSTR::from(name)).map_err(io::Error::other)
    }
}

/// Enables or disables a task, journaling its old state first.
pub fn set_enabled(task: &Task, on: bool, dir: &Path) -> io::Result<()> {
    let (_, mut f) = crate::clean::journal_file(dir, "Scheduled Tasks")?;
    writeln!(f, "T\t{}\t{}\t{}", task.path, task.enabled, on)?;
    f.sync_all()?;
    let t = registered(&task.path)?;
    // SAFETY: flips the task's Enabled flag.
    unsafe { t.SetEnabled(if on { VARIANT_TRUE } else { VARIANT_FALSE }) }.map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_without_windows_tasks() {
        // May be empty on a clean machine; never includes \Microsoft\….
        assert!(list().iter().all(|t| !t.path.starts_with(r"\Microsoft\")));
    }

    /// Read-only: `cargo test -p bosun-core real_tasks -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_tasks() {
        let all = list();
        for t in &all {
            println!("on={} {} | {}", t.enabled, t.name, t.path);
        }
        println!("{} tasks", all.len());
    }
}
