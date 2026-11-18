use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};

use super::AppSpec;

/// Owns one Unix process group, including last-resort cleanup.
/// Group membership is not a security boundary: descendants can leave it.
pub(super) struct AppProcess {
    child: Child,
}

impl AppProcess {
    pub fn spawn(spec: &AppSpec) -> io::Result<Self> {
        let mut command = Command::new(&spec.executable);
        command
            .args(&spec.args)
            .envs(&spec.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .process_group(0);
        if let Some(directory) = &spec.working_directory {
            command.current_dir(directory);
        }
        command.spawn().map(|child| Self { child })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn terminate(&self) -> io::Result<()> {
        self.signal(libc::SIGTERM)
    }
    pub fn kill(&self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }

    fn signal(&self, signal: i32) -> io::Result<()> {
        let pid =
            i32::try_from(self.id()).map_err(|_| io::Error::other("process ID exceeds pid_t"))?;
        if pid <= 0 {
            return Err(io::Error::other("invalid process group ID"));
        }
        // SAFETY: integer-only call; negative PID selects our child's process
        // group, created separately from Core's by process_group(0).
        if unsafe { libc::kill(-pid, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

impl Drop for AppProcess {
    fn drop(&mut self) {
        let _ = self.kill();
        let _ = self.child.wait();
    }
}
