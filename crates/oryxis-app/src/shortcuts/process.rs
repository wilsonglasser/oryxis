//! Launching another Oryxis.
//!
//! Duplicate-in-new-window spawns a child with `--connect <id>` and
//! hands it the master password on stdin; the renderer switch relaunches
//! the app in place. Neither is a shortcut: they live here because the
//! menu entries that trigger them do, and getting them out of the key
//! router is exactly the point of this split.


use crate::app::Oryxis;

impl Oryxis {
    /// Spawns a fresh top-level Oryxis process. When `source_tab`
    /// names a tab bound to a saved connection, passes
    /// `--connect <uuid>` so the new window auto-opens it. When the
    /// caller already has a master password unlocked, also passes
    /// `--inherit-vault` and pipes the password through stdin so the
    /// secret never appears in argv (which `ps aux` would expose).
    pub(crate) fn spawn_oryxis_child(&self, source_tab: Option<usize>) {
        // Map the tab back to a saved connection so the child opens the
        // same host. Quick-connect tabs have no saved connection, so
        // they resolve to None and the child opens a plain window (a
        // fresh process can't carry an in-memory relaunch message
        // across the boundary).
        let connect_uuid = source_tab.and_then(|idx| {
            self.tabs.get(idx).and_then(|tab| {
                let base_label = tab
                    .label
                    .trim_end_matches(" (disconnected)")
                    .trim_start_matches(crate::app::SSM_TAB_PREFIX)
                    .to_string();
                self.connections
                    .iter()
                    .find(|c| c.label == base_label)
                    .map(|c| c.id)
            })
        });
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("current_exe unavailable: {}", e);
                return;
            }
        };
        let mut cmd = std::process::Command::new(exe);
        if let Some(uuid) = connect_uuid {
            cmd.arg("--connect").arg(uuid.to_string());
        }
        let inherit = self.master_password.is_some();
        if inherit {
            cmd.arg("--inherit-vault");
            cmd.stdin(std::process::Stdio::piped());
        }
        match cmd.spawn() {
            Ok(mut child) => {
                if inherit
                    && let Some(mut stdin) = child.stdin.take()
                    && let Some(pw) = self.master_password.as_ref()
                {
                    use std::io::Write as _;
                    let _ = writeln!(stdin, "{}", pw);
                    // Closing the pipe signals EOF to the child.
                    drop(stdin);
                }
            }
            Err(e) => tracing::error!("Failed to spawn new window: {}", e),
        }
    }

    /// Relaunch the app in place: spawn a fresh process that inherits
    /// the unlocked vault, then exit the current one. Used to apply a
    /// setting that is only read at process start (the graphics
    /// renderer). The child carries `--relaunch` so it waits for this
    /// process's single-instance mutex to release and comes back as
    /// primary. Live SSH sessions and tabs do not survive a process
    /// restart, the caller warns the user before invoking this.
    ///
    /// Never returns on success (`process::exit`). On a spawn failure it
    /// returns so the caller stays running rather than stranding the user
    /// with no window.
    pub(crate) fn relaunch_self(&self) {
        // The replacement process should come back with today's window
        // geometry, and this one exits without passing through the
        // normal close path.
        self.persist_window_geometry();
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("relaunch: current_exe unavailable: {e}");
                return;
            }
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--relaunch");
        // A diagnostic session started with --debug-log must not lose its
        // log across a renderer-change restart: that restart is exactly
        // the kind of event the log is being kept for.
        if crate::logging::is_forced() {
            cmd.arg("--debug-log");
        }
        let inherit = self.master_password.is_some();
        if inherit {
            cmd.arg("--inherit-vault");
            cmd.stdin(std::process::Stdio::piped());
        }
        match cmd.spawn() {
            Ok(mut child) => {
                if inherit
                    && let Some(mut stdin) = child.stdin.take()
                    && let Some(pw) = self.master_password.as_ref()
                {
                    use std::io::Write as _;
                    let _ = writeln!(stdin, "{}", pw);
                    drop(stdin);
                }
                // Hand off cleanly: the child is up, drop this process so
                // the mutex releases and the child promotes to primary.
                std::process::exit(0);
            }
            Err(e) => tracing::error!("relaunch: spawn failed: {e}"),
        }
    }
}
