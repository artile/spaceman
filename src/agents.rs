//! Processes an AI agent drives: the `agent = true` rule matcher reads this registry, and
//! `spaceman ctl agent claim|release|list` writes it. It lives in memory only; a restart
//! forgets every claim, and the agent claims its process again.
//!
//! An app an agent launches through Launch Services has `launchd` as its parent, so the process
//! tree cannot tell agent windows apart. The agent names its process instead.

use std::collections::BTreeSet;
use std::sync::{LazyLock, RwLock};

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
    fn refuses_launchd_and_missing_processes() {
        assert!(claim(1).is_err());
        assert!(claim(0).is_err());
        assert!(claim(i32::MAX).is_err());
    }
}
