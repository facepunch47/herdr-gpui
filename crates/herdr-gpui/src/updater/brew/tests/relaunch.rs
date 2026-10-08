use super::super::schedule_relaunch;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

struct Reap(Option<Child>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn write_executable(path: &Path, body: &str) -> anyhow::Result<()> {
    let status = Command::new("/bin/sh")
        .args(["-c", "printf '%s' \"$1\" > \"$2\"", "write-opener", body])
        .arg(path)
        .status()?;
    anyhow::ensure!(status.success(), "writing opener failed: {status}");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn wait_for(path: &Path) -> anyhow::Result<String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.is_file() {
            return Ok(fs::read_to_string(path)?);
        }
        thread::sleep(Duration::from_millis(20));
    }
    anyhow::bail!("timed out waiting for {}", path.display())
}

#[test]
fn relaunch_waits_for_the_running_app_then_opens_the_same_bundle() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let bundle = root.path().join("Applications/Herdr App.app");
    fs::create_dir_all(&bundle)?;
    let log = root.path().join("open-args");
    let opener = root.path().join("open");
    write_executable(
        &opener,
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display()),
    )?;
    let ready = root.path().join("ready");
    let settled = root.path().join("settled");
    let watched = Command::new("/bin/sleep").arg("30").spawn()?;
    let pid = watched.id();
    let mut watched = Reap(Some(watched));
    schedule_relaunch(pid, &bundle, &opener, 600, Some(&ready), Some(&settled))?;
    wait_for(&ready)?;
    thread::sleep(Duration::from_millis(150));
    anyhow::ensure!(
        !settled.exists() && !log.exists(),
        "opened while the old instance was still running"
    );
    if let Some(child) = watched.0.as_mut() {
        child.kill()?;
        child.wait()?;
    }
    anyhow::ensure!(
        wait_for(&settled)? == "open\n",
        "waiter did not choose open"
    );
    let args = wait_for(&log)?;
    anyhow::ensure!(
        args == format!("--\n{}\n", bundle.display()),
        "opener args were {args:?}"
    );
    anyhow::ensure!(!args.split_whitespace().any(|arg| arg == "-n"));
    Ok(())
}

#[test]
fn relaunch_does_not_open_a_second_copy_when_the_app_never_exits() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let bundle = root.path().join(super::super::BUNDLE);
    fs::create_dir_all(&bundle)?;
    let log = root.path().join("open-args");
    let opener = root.path().join("open");
    write_executable(
        &opener,
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display()),
    )?;
    let settled = root.path().join("settled");
    schedule_relaunch(
        std::process::id(),
        &bundle,
        &opener,
        0,
        None,
        Some(&settled),
    )?;
    anyhow::ensure!(wait_for(&settled)? == "gave-up\n");
    thread::sleep(Duration::from_millis(100));
    anyhow::ensure!(!log.exists(), "gave up by launching anyway");
    Ok(())
}
