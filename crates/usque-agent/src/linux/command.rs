use std::{
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use usque_ipc::{
    agent_v1::{LinuxCommandAdapter, LinuxFailureDetail, LinuxFailureReason},
    linux_failure,
};

const MAX_OUTPUT: u64 = 1024 * 1024;

/// Fixed executable paths, an empty environment and bounded pipe readers keep
/// network managers' D-Bus/nft JSON adapters out of shell parsing and PATH.
pub fn run(
    program: &'static str,
    arguments: &[String],
    input: Option<Vec<u8>>,
) -> io::Result<Vec<u8>> {
    run_inner(program, arguments, input).map_err(|error| {
        let mut detail = linux_failure::detail(&error).unwrap_or_default();
        if detail.reason == 0 {
            detail.reason = if error.kind() == io::ErrorKind::TimedOut {
                LinuxFailureReason::Timeout
            } else {
                LinuxFailureReason::Io
            } as i32;
        }
        detail.adapter = match program {
            "/usr/sbin/nft" => LinuxCommandAdapter::Nft,
            "/usr/bin/busctl" => LinuxCommandAdapter::Busctl,
            _ => LinuxCommandAdapter::Unspecified,
        } as i32;
        detail.os_errno = detail
            .os_errno
            .or_else(|| error.raw_os_error().filter(|n| (1..=4095).contains(n)));
        linux_failure::with_detail(detail, error)
    })
}

fn run_inner(
    program: &'static str,
    arguments: &[String],
    input: Option<Vec<u8>>,
) -> io::Result<Vec<u8>> {
    super::auth::open_trusted_file(Path::new(program))?;
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output = thread::spawn(move || bounded_read(stdout));
    let errors = thread::spawn(move || bounded_read(stderr));
    let input = match (input, child.stdin.take()) {
        (Some(bytes), Some(mut pipe)) => Some(thread::spawn(move || pipe.write_all(&bytes))),
        _ => None,
    };
    let deadline = Instant::now() + Duration::from_secs(8);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Linux network adapter timed out",
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
        }
    };
    let stdout = output
        .join()
        .map_err(|_| io::Error::other("Linux adapter output worker failed"))
        .and_then(|result| result);
    let _stderr = errors
        .join()
        .map_err(|_| io::Error::other("Linux adapter error worker failed"))
        .and_then(|result| result);
    let input = if let Some(input) = input {
        input
            .join()
            .map_err(|_| io::Error::other("Linux adapter input worker failed"))
            .and_then(|result| result)
    } else {
        Ok(())
    };
    let status = status?;
    if !status.success() {
        return Err(linux_failure::with_detail(
            LinuxFailureDetail {
                reason: if status.code().is_some() {
                    LinuxFailureReason::CommandRejected
                } else {
                    LinuxFailureReason::CommandSignaled
                } as i32,
                adapter: if program == "/usr/sbin/nft" {
                    LinuxCommandAdapter::Nft
                } else {
                    LinuxCommandAdapter::Busctl
                } as i32,
                exit_code: status.code().filter(|n| (1..=255).contains(n)),
                ..Default::default()
            },
            io::Error::other("Linux network adapter rejected the operation"),
        ));
    }
    input?;
    _stderr?;
    stdout
}

fn bounded_read(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err(io::Error::other("Linux adapter output exceeds limit"));
    }
    Ok(bytes)
}
