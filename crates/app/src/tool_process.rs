use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

/// Drain both pipes while polling the process. Tool text is private to the caller.
pub(crate) fn run(
    command: &mut Command,
    should_stop: impl Fn() -> bool,
    mut stdout_line: impl FnMut(&str) + Send,
) -> io::Result<(ExitStatus, String)> {
    if should_stop() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    ProcessTree::configure(command);
    let mut child = command.spawn()?;
    let tree = match ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let stdout = child.stdout.take().expect("stdout configured as pipe");
    let stderr = child.stderr.take().expect("stderr configured as pipe");
    thread::scope(|scope| {
        let stdout_reader = scope.spawn(move || read_lines(stdout, &mut stdout_line));
        let stderr_reader = scope.spawn(move || {
            const MAX_TAIL: usize = 64 * 1024;
            let mut tail = String::new();
            read_lines(stderr, |line| {
                if tail.len() + line.len() + 1 > MAX_TAIL {
                    tail.clear();
                }
                tail.push_str(line);
                tail.push('\n');
            })?;
            Ok::<_, io::Error>(tail)
        });
        let status = loop {
            if should_stop() {
                break Err(io::ErrorKind::Interrupted.into());
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(error) => break Err(error),
            }
        };
        let stop_result = tree.terminate();
        if stop_result.is_err() {
            let _ = child.kill();
        }
        child.wait()?;
        stop_result?;
        let status = status?;
        stdout_reader
            .join()
            .map_err(|_| io::Error::other("stdout reader failed"))??;
        let stderr = stderr_reader
            .join()
            .map_err(|_| io::Error::other("stderr reader failed"))??;
        Ok((status, stderr))
    })
}

fn read_lines(reader: impl Read, mut handle: impl FnMut(&str)) -> io::Result<()> {
    const MAX_LINE: usize = 16 * 1024;
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        // A line longer than the cap is discarded, including its remainder.
        let count = reader
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        if count <= MAX_LINE {
            handle(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']));
        } else if line.last() != Some(&b'\n') {
            reader.skip_until(b'\n')?;
        }
        line.clear();
    }
}

#[cfg(unix)]
struct ProcessTree(u32);

#[cfg(unix)]
impl ProcessTree {
    fn configure(command: &mut Command) {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    fn attach(child: &Child) -> io::Result<Self> {
        Ok(Self(child.id()))
    }

    fn terminate(&self) -> io::Result<()> {
        // The spawned child leads a fresh process group.
        if unsafe { libc::killpg(self.0 as libc::pid_t, libc::SIGKILL) } == 0 {
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

#[cfg(windows)]
struct ProcessTree(std::os::windows::io::OwnedHandle);

#[cfg(windows)]
impl ProcessTree {
    fn configure(_command: &mut Command) {}

    fn attach(child: &Child) -> io::Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // The job is owned here and closes automatically on every return path.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(job))
    }

    fn terminate(&self) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    #[test]
    fn oversized_tool_output_is_discarded_and_stream_drains() {
        let input = format!("{}\nnormal\r\n", "x".repeat(40_000));
        let mut lines = Vec::new();
        read_lines(input.as_bytes(), |line| lines.push(line.to_string())).unwrap();
        assert_eq!(lines, ["normal"]);
    }

    #[test]
    fn stop_ends_the_process_tree_and_unblocks_output_readers() {
        let ready = AtomicBool::new(false);
        let started = Instant::now();
        let result = run(
            &mut test_tool("leader"),
            || ready.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(5),
            |line| {
                if line.contains("rustcatch-test-descendant-ready") {
                    ready.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(ready.load(Ordering::Relaxed), "descendant never started");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn successful_process_preserves_its_report_and_exit_status() {
        let mut ready = false;
        let (status, _) = run(
            &mut test_tool("success"),
            || false,
            |line| {
                ready |= line.contains("rustcatch-test-success");
            },
        )
        .unwrap();
        assert!(status.success());
        assert!(ready);
    }

    #[test]
    fn subprocess_fixture() {
        match std::env::var("RUSTCATCH_PROCESS_TEST_ROLE").as_deref() {
            Ok("leader") => assert!(test_tool("descendant").status().unwrap().success()),
            Ok("descendant") => {
                println!("rustcatch-test-descendant-ready");
                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(12));
            }
            Ok("success") => println!("rustcatch-test-success"),
            _ => {}
        }
    }

    fn test_tool(role: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "tool_process::tests::subprocess_fixture",
            "--nocapture",
        ]);
        command.env("RUSTCATCH_PROCESS_TEST_ROLE", role);
        command
    }
}
