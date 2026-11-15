//! Bounded native adapters. Callers supply generated arguments, never a shell.
use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
/// Darwin lacks atomic receive-with-CLOEXEC. Agent receive and child creation
/// share this lock so an unexpected descriptor cannot leak into an adapter.
pub use usque_ipc::darwin_fd::DESCRIPTOR_EXEC_LOCK;
#[derive(Clone, Copy, Debug)]
pub enum Program {
    Pfctl,
    Route,
    Netstat,
    Ifconfig,
    InstalledAgent,
}
impl Program {
    fn path(self) -> &'static str {
        match self {
            Self::Pfctl => "/sbin/pfctl",
            Self::Route => "/sbin/route",
            Self::Netstat => "/usr/sbin/netstat",
            Self::Ifconfig => "/sbin/ifconfig",
            Self::InstalledAgent => "/Library/PrivilegedHelperTools/com.usque.agent/usque-agent",
        }
    }
}
#[derive(Debug)]
pub struct Output {
    pub pid: u32,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}
pub trait Runner: Send {
    fn run(&mut self, program: Program, args: &[String], input: Option<&str>)
    -> io::Result<Output>;
}
#[derive(Default)]
pub struct NativeRunner;
impl Runner for NativeRunner {
    fn run(
        &mut self,
        program: Program,
        args: &[String],
        input: Option<&str>,
    ) -> io::Result<Output> {
        super::auth::trusted_file(std::path::Path::new(program.path()))?;
        if args.len() > 32
            || args.iter().any(|s| s.len() > 4096 || s.contains('\0'))
            || input.is_some_and(|s| s.len() > 128 * 1024)
        {
            return Err(io::Error::other("native adapter input exceeds bound"));
        }
        let mut child = {
            let _serial = DESCRIPTOR_EXEC_LOCK
                .lock()
                .map_err(|_| io::Error::other("descriptor guard poisoned"))?;
            Command::new(program.path())
                .args(args)
                .env_clear()
                .env("LC_ALL", "C")
                .stdin(if input.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        };
        let pid = child.id();
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let output = thread::spawn(move || read(stdout));
        let errors = thread::spawn(move || read(stderr));
        let input = match (input, child.stdin.take()) {
            (Some(text), Some(mut stdin)) => {
                let bytes = text.as_bytes().to_vec();
                Some(thread::spawn(move || stdin.write_all(&bytes)))
            }
            _ => None,
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(result.err().unwrap_or_else(|| {
                        io::Error::new(io::ErrorKind::TimedOut, "native adapter timeout")
                    }));
                }
            }
        };
        let stdout = output
            .join()
            .map_err(|_| io::Error::other("native output worker failed"))?;
        let stderr = errors
            .join()
            .map_err(|_| io::Error::other("native error worker failed"))?;
        if let Some(input) = input {
            input
                .join()
                .map_err(|_| io::Error::other("native input worker failed"))??;
        }
        Ok(Output {
            pid,
            stdout: stdout?,
            stderr: stderr?,
            success: status?.success(),
        })
    }
}
fn read(reader: impl Read) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("native adapter output exceeds bound"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("invalid native adapter output"))
}
pub fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}
pub fn checked(
    runner: &mut impl Runner,
    program: Program,
    args: &[String],
    input: Option<&str>,
) -> io::Result<Output> {
    let result = runner.run(program, args, input)?;
    if !result.success {
        return Err(io::Error::other("native adapter rejected operation"));
    }
    Ok(result)
}
