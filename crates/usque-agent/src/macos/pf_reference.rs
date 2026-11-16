//! PF enable references can be reused. A token alone is not durable ownership.
use super::command::{self, Program, Runner};
use serde::{Deserialize, Serialize};
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub token: u64,
    pub pid: u32,
    pub created_min: u64,
    pub created_max: u64,
}
fn invalid() -> io::Error {
    io::Error::other("PF reference identity unavailable; recovery incomplete")
}
fn now() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(io::Error::other)
}
#[derive(Debug)]
struct Observed {
    token: u64,
    pid: u32,
    name: String,
    age: u64,
}
fn parse(text: &str) -> io::Result<Vec<Observed>> {
    if text.trim() == "No pf starter references held" {
        return Ok(vec![]);
    }
    let mut lines = text.lines();
    if lines.next() != Some("TOKENS:")
        || lines
            .next()
            .map(|s| s.split_whitespace().collect::<Vec<_>>())
            != Some(vec!["PID", "Process", "Name", "TOKEN", "TIMESTAMP"])
    {
        return Err(invalid());
    }
    let mut result = vec![];
    for line in lines.filter(|s| !s.trim().is_empty()) {
        if line.chars().any(|c| c.is_control() && c != '\t') || result.len() >= 4096 {
            return Err(invalid());
        }
        let words = line.split_whitespace().collect::<Vec<_>>();
        if words.len() < 6 || words[words.len() - 2] != "days" {
            return Err(invalid());
        }
        let n = words.len();
        let token: u64 = words[n - 4].parse().map_err(|_| invalid())?;
        let pid = words[0].parse().map_err(|_| invalid())?;
        let days: u64 = words[n - 3].parse().map_err(|_| invalid())?;
        let clock = words[n - 1]
            .split(':')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid())?;
        if token == 0
            || pid == 0
            || clock.len() != 3
            || clock[0] > 23
            || clock[1] > 59
            || clock[2] > 59
            || result.iter().any(|r: &Observed| r.token == token)
        {
            return Err(invalid());
        }
        let age = days
            .checked_mul(86400)
            .and_then(|v| v.checked_add(clock[0] * 3600 + clock[1] * 60 + clock[2]))
            .ok_or_else(invalid)?;
        result.push(Observed {
            token,
            pid,
            name: words[1..n - 4].join(" "),
            age,
        });
    }
    Ok(result)
}
fn observations(runner: &mut impl Runner) -> io::Result<(u64, Vec<Observed>)> {
    let output = command::checked(
        runner,
        Program::Pfctl,
        &command::arguments(&["-s", "References"]),
        None,
    )?;
    Ok((now()?, parse(&output.stdout)?))
}
fn owned(reference: &Reference, observed: &Observed, time: u64) -> bool {
    let Some(created) = time.checked_sub(observed.age) else {
        return false;
    };
    reference.pid == observed.pid
        && observed.name == "pfctl"
        && created >= reference.created_min
        && created <= reference.created_max
}
pub fn acquire(runner: &mut impl Runner) -> io::Result<Reference> {
    let start = now()?;
    let output = command::checked(runner, Program::Pfctl, &command::arguments(&["-E"]), None)?;
    let token = output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .find_map(|s| {
            s.trim()
                .strip_prefix("Token :")
                .or_else(|| s.trim().strip_prefix("Token:"))
        })
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n != 0)
        .ok_or_else(invalid)?;
    let reference = Reference {
        token,
        pid: output.pid,
        created_min: start.saturating_sub(2),
        created_max: now()?.saturating_add(2),
    };
    let (time, entries) = observations(runner)?;
    if entries
        .iter()
        .find(|e| e.token == token)
        .is_none_or(|entry| !owned(&reference, entry, time))
    {
        return Err(invalid());
    }
    Ok(reference)
}
/// Called only after a durable release intent. Absence is an idempotent retry;
/// reuse by a different process/time is never permission to release that owner.
pub fn release(runner: &mut impl Runner, reference: &Reference) -> io::Result<()> {
    let (time, entries) = observations(runner)?;
    if let Some(entry) = entries.iter().find(|e| e.token == reference.token) {
        if !owned(reference, entry, time) {
            return Err(invalid());
        }
        command::checked(
            runner,
            Program::Pfctl,
            &command::arguments(&["-X", &reference.token.to_string()]),
            None,
        )?;
    }
    let (_, entries) = observations(runner)?;
    if entries.iter().any(|e| e.token == reference.token) {
        return Err(invalid());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_parser_preserves_owner_and_rejects_duplicates() {
        let row = "123 pfctl 42 1 days 01:02:03\n";
        let header = "TOKENS:\nPID Process Name TOKEN TIMESTAMP\n";
        let entries = parse(&(header.to_owned() + row)).unwrap();
        assert_eq!(entries[0].age, 90123);
        assert!(parse(&(header.to_owned() + row + row)).is_err());
        assert!(parse("permission denied").is_err());
        assert!(parse("No pf starter references held\n").unwrap().is_empty());
        let reference = Reference {
            token: 42,
            pid: 123,
            created_min: 100,
            created_max: 104,
        };
        assert!(owned(&reference, &entries[0], 90225));
        assert!(!owned(
            &Reference {
                pid: 124,
                ..reference.clone()
            },
            &entries[0],
            90225
        ));
        assert!(!owned(&reference, &entries[0], 90230));
    }
    #[test]
    fn release_retry_never_releases_a_reused_token() {
        struct Fake {
            pid: Option<u32>,
            releases: usize,
        }
        impl Runner for Fake {
            fn run(
                &mut self,
                _: Program,
                args: &[String],
                _: Option<&str>,
            ) -> io::Result<command::Output> {
                if args.first().is_some_and(|s| s == "-X") {
                    self.releases += 1;
                    self.pid = None;
                }
                Ok(command::Output {
                    pid: 123,
                    success: true,
                    stderr: String::new(),
                    stdout: match self.pid {
                        Some(pid) => format!(
                            "TOKENS:\nPID Process Name TOKEN TIMESTAMP\n{pid} pfctl 42 0 days 00:00:00\n"
                        ),
                        None => "No pf starter references held".into(),
                    },
                })
            }
        }
        let now = now().unwrap();
        let reference = Reference {
            token: 42,
            pid: 123,
            created_min: now.saturating_sub(2),
            created_max: now + 2,
        };
        let mut fake = Fake {
            pid: Some(999),
            releases: 0,
        };
        assert!(release(&mut fake, &reference).is_err());
        assert_eq!(fake.releases, 0);
        fake.pid = Some(123);
        release(&mut fake, &reference).unwrap();
        release(&mut fake, &reference).unwrap();
        assert_eq!(fake.releases, 1);
    }
}
