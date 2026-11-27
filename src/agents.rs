//! Processes an AI agent drives: the `agent = true` rule matcher reads this registry, and
//! `spaceman ctl agent claim|release|list` writes it. It lives in memory only; a restart
//! forgets every claim, and the agent claims its process again.
//!
//! An app an agent launches through Launch Services has `launchd` as its parent, so the process
//! tree cannot tell agent windows apart. The agent names its process instead.
//!
//! A launcher that returns the pid only once the first window is up (cua-driver `launch_app`)
//! claims too late: the window opens on the user's workspace first. `ctl agent expect <bundle>`
//! comes before the launch instead: the next process of that bundle that starts within
//! [`EXPECT_FOR`] is claimed when Spaceman first sees its window.

use std::collections::BTreeSet;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

/// How long an expectation waits for its process.
pub const EXPECT_FOR: Duration = Duration::from_secs(60);
/// Launch Services can date a launch a little before the request that caused it.
const LAUNCH_SLACK_MS: u64 = 2_000;

struct Expected {
    bundle_id: String,
    /// Milliseconds since the epoch, as `AppInstanceId::launched_at`: only a process launched
    /// after this is the agent's, never one the user already runs.
    since_ms: u64,
    until: Instant,
}

static EXPECTED: LazyLock<RwLock<Vec<Expected>>> = LazyLock::new(|| RwLock::new(Vec::new()));

static CLAIMED: LazyLock<RwLock<BTreeSet<i32>>> = LazyLock::new(|| RwLock::new(BTreeSet::new()));
/// Fake pids tests claim: they count as running, so a parallel test's pruning keeps them.
#[cfg(test)]
static FAKE_LIVE: LazyLock<RwLock<BTreeSet<i32>>> = LazyLock::new(|| RwLock::new(BTreeSet::new()));

fn alive(pid: i32) -> bool {
    #[cfg(test)]
    if FAKE_LIVE.read().unwrap().contains(&pid) {
        return true;
    }
    // Signal 0 checks that the process exists without sending anything.
    // SAFETY: kill(2) takes plain integers and touches no memory of this process.
    #[allow(unsafe_code)]
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Marks `pid` as an agent's process. Forgets claimed processes that exited.
pub fn claim(pid: i32) -> Result<bool, String> {
    if pid <= 1 {
        return Err(format!("pid {pid} cannot be an agent process"));
    }
    if !alive(pid) {
        return Err(format!("no process with pid {pid}"));
    }
    let mut claimed = CLAIMED.write().map_err(|_| "agent registry is poisoned")?;
    claimed.retain(|pid| alive(*pid));
    Ok(claimed.insert(pid))
}

/// Expects the next process of `bundle_id` that launches within [`EXPECT_FOR`] to be an agent's.
pub fn expect(bundle_id: &str) -> Result<(), String> {
    if bundle_id.trim().is_empty() || bundle_id.contains(char::is_whitespace) {
        return Err(format!("expected a bundle ID, got {bundle_id:?}"));
    }
    let now_ms = u64::try_from(crate::config::now().as_millis()).unwrap_or(u64::MAX);
    let mut expected = EXPECTED.write().map_err(|_| "agent registry is poisoned")?;
    let now = Instant::now();
    expected.retain(|entry| entry.until > now && entry.bundle_id != bundle_id);
    expected.push(Expected {
        bundle_id: bundle_id.to_owned(),
        since_ms: now_ms.saturating_sub(LAUNCH_SLACK_MS),
        until: now + EXPECT_FOR,
    });
    Ok(())
}

/// Whether the process `pid` of `bundle_id`, launched at `launched_at_ms`, is an agent's: claimed,
/// or the one an expectation waits for (which claims it).
#[must_use]
pub fn is_agent_process(pid: i32, bundle_id: &str, launched_at_ms: u64) -> bool {
    if is_agent(pid) {
        return true;
    }
    let Ok(mut expected) = EXPECTED.write() else {
        return false;
    };
    let now = Instant::now();
    expected.retain(|entry| entry.until > now);
    let Some(index) = expected
        .iter()
        .position(|entry| entry.bundle_id == bundle_id && launched_at_ms >= entry.since_ms)
    else {
        return false;
    };
    expected.remove(index);
    drop(expected);
    CLAIMED.write().is_ok_and(|mut claimed| {
        claimed.insert(pid);
        true
    })
}

/// Forgets `pid`. Returns whether it was claimed.
pub fn release(pid: i32) -> bool {
    CLAIMED
        .write()
        .is_ok_and(|mut claimed| claimed.remove(&pid))
}

/// Whether an agent claimed `pid`.
#[must_use]
pub fn is_agent(pid: i32) -> bool {
    CLAIMED.read().is_ok_and(|claimed| claimed.contains(&pid))
}

/// The claimed processes that still run, in pid order.
#[must_use]
pub fn list() -> Vec<i32> {
    CLAIMED.write().map_or_else(
        |_| Vec::new(),
        |mut claimed| {
            claimed.retain(|pid| alive(*pid));
            claimed.iter().copied().collect()
        },
    )
}

/// Claims `pid` without the liveness check: tests use fake pids.
#[cfg(test)]
pub(crate) fn claim_for_test(pid: i32) {
    FAKE_LIVE.write().unwrap().insert(pid);
    CLAIMED.write().unwrap().insert(pid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_a_live_process_and_releases_it() {
        let pid = i32::try_from(std::process::id()).unwrap();
        assert_eq!(claim(pid), Ok(true));
        assert_eq!(claim(pid), Ok(false));
        assert!(is_agent(pid));
        assert!(list().contains(&pid));
        assert!(release(pid));
        assert!(!is_agent(pid));
        assert!(!release(pid));
    }

    #[test]
    fn an_expectation_claims_the_next_process_of_its_bundle_once() {
        let now = crate::config::now().as_millis() as u64;
        // Keeps the fake pid when a parallel test prunes exited processes.
        FAKE_LIVE.write().unwrap().insert(9103);
        expect("com.example.Expected").unwrap();
        // A process the user started before the expectation stays theirs.
        assert!(!is_agent_process(
            9101,
            "com.example.Expected",
            now - 60_000
        ));
        assert!(!is_agent_process(9102, "com.example.Other", now));
        assert!(is_agent_process(9103, "com.example.Expected", now));
        assert!(is_agent(9103));
        // Consumed: the next process of the bundle is not the agent's.
        assert!(!is_agent_process(9104, "com.example.Expected", now));
        assert!(release(9103));
        assert!(expect("not a bundle").is_err());
    }

    #[test]
    fn refuses_launchd_and_missing_processes() {
        assert!(claim(1).is_err());
        assert!(claim(0).is_err());
        assert!(claim(i32::MAX).is_err());
    }
}
