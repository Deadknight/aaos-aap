use std::fs::OpenOptions;
use std::io;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ScreenrecordConfig {
    pub size: String,
    pub bit_rate: u32,
    pub seconds: u32,
    pub display_id: Option<String>,
}

pub struct ScreenrecordSource {
    child: Child,
    stdout: ChildStdout,
}

impl ScreenrecordSource {
    fn build_command(config: ScreenrecordConfig) -> Command {
        let mut cmd = Command::new("screenrecord");
        cmd.arg("--output-format=h264")
            .arg("--size")
            .arg(config.size)
            .arg("--bit-rate")
            .arg(config.bit_rate.to_string())
            .arg("--time-limit")
            .arg(config.seconds.to_string());

        if let Some(display_id) = config.display_id {
            cmd.arg("--display-id").arg(display_id);
        }

        // '-' makes screenrecord write the raw H264/NAL stream to stdout on this AAOS image.
        cmd.arg("-");
        cmd
    }

    fn stderr_log() -> Stdio {
        // Do not let screenrecord stderr contaminate adb exec-out binary stdout.
        // Keep a small log on-device so failures are still debuggable.
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("/data/local/tmp/arda-screenrecord.stderr.log")
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    }

    pub fn spawn(config: ScreenrecordConfig) -> io::Result<Self> {
        let mut cmd = Self::build_command(config);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Self::stderr_log());

        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture screenrecord stdout"))?;

        Ok(Self { child, stdout })
    }



    pub fn run_direct_to_stdout(config: ScreenrecordConfig) -> io::Result<ExitStatus> {
        // Live preview mode: inherit stdout so screenrecord writes raw H264 directly
        // to adb exec-out. This avoids buffering/pipe edge cases in capture-test --out -.
        let mut cmd = Self::build_command(config);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Self::stderr_log());
        cmd.status()
    }

    pub fn stdout_mut(&mut self) -> &mut ChildStdout {
        &mut self.stdout
    }

    pub fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status));
            }
            if start.elapsed() >= timeout {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for ScreenrecordSource {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
