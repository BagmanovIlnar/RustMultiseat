//! In-process session-agent registry and heartbeat watchdog.
//!
//! Agents are least-privilege processes inside user sessions. Missing
//! heartbeats trigger fail-open so host input is never stuck suppressed.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use multiseat_core::SeatId;

/// Default interval advertised to agents in [`multiseat_core::ServerResponse::AgentAck`].
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

/// How long after the last heartbeat an agent is considered dead.
pub const DEFAULT_AGENT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
struct AgentEntry {
    last_seen: Instant,
    agent_version: u32,
}

/// Tracks live session agents by seat.
#[derive(Debug, Default)]
pub struct AgentRegistry {
    agents: HashMap<u32, AgentEntry>,
    timeout: Duration,
    heartbeat_interval: Duration,
}

impl AgentRegistry {
    /// Creates a registry with default heartbeat/timeout windows.
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            timeout: DEFAULT_AGENT_TIMEOUT,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
        }
    }

    /// Creates a registry with custom timing (tests).
    pub fn with_timeouts(heartbeat_interval: Duration, timeout: Duration) -> Self {
        Self {
            agents: HashMap::new(),
            timeout,
            heartbeat_interval,
        }
    }

    /// Heartbeat interval sent to agents.
    pub fn heartbeat_interval_ms(&self) -> u64 {
        self.heartbeat_interval.as_millis() as u64
    }

    /// Registers or refreshes an agent after `AgentHello`.
    pub fn register(&mut self, seat_id: SeatId, agent_version: u32, now: Instant) {
        self.agents.insert(
            seat_id.0,
            AgentEntry {
                last_seen: now,
                agent_version,
            },
        );
    }

    /// Updates last-seen time for an existing agent. Returns false if unknown.
    pub fn heartbeat(&mut self, seat_id: SeatId, now: Instant) -> bool {
        match self.agents.get_mut(&seat_id.0) {
            Some(entry) => {
                entry.last_seen = now;
                true
            }
            None => false,
        }
    }

    /// Removes an agent (explicit fail-open / disconnect).
    pub fn remove(&mut self, seat_id: SeatId) -> bool {
        self.agents.remove(&seat_id.0).is_some()
    }

    /// True when the seat has a registered agent.
    pub fn is_online(&self, seat_id: SeatId) -> bool {
        self.agents.contains_key(&seat_id.0)
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Whether no agents are registered.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Agent version for diagnostics.
    pub fn agent_version(&self, seat_id: SeatId) -> Option<u32> {
        self.agents.get(&seat_id.0).map(|e| e.agent_version)
    }

    /// Seats whose last heartbeat exceeded the timeout.
    pub fn timed_out(&self, now: Instant) -> Vec<SeatId> {
        self.agents
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_seen) > self.timeout)
            .map(|(&id, _)| SeatId(id))
            .collect()
    }

    /// Drops timed-out agents and returns their seat ids.
    pub fn reap_timed_out(&mut self, now: Instant) -> Vec<SeatId> {
        let dead = self.timed_out(now);
        for seat in &dead {
            self.agents.remove(&seat.0);
        }
        dead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_refreshes_timeout_window() {
        let mut reg =
            AgentRegistry::with_timeouts(Duration::from_millis(100), Duration::from_millis(300));
        let t0 = Instant::now();
        reg.register(SeatId(1), 1, t0);
        assert!(reg.timed_out(t0 + Duration::from_millis(200)).is_empty());
        assert!(reg.heartbeat(SeatId(1), t0 + Duration::from_millis(250)));
        assert!(reg.timed_out(t0 + Duration::from_millis(400)).is_empty());
        assert_eq!(
            reg.timed_out(t0 + Duration::from_millis(600)),
            vec![SeatId(1)]
        );
    }

    #[test]
    fn reap_removes_dead_agents() {
        let mut reg =
            AgentRegistry::with_timeouts(Duration::from_millis(50), Duration::from_millis(100));
        let t0 = Instant::now();
        reg.register(SeatId(2), 1, t0);
        let dead = reg.reap_timed_out(t0 + Duration::from_millis(250));
        assert_eq!(dead, vec![SeatId(2)]);
        assert!(!reg.is_online(SeatId(2)));
    }

    #[test]
    fn heartbeat_unknown_seat_fails() {
        let mut reg = AgentRegistry::new();
        assert!(!reg.heartbeat(SeatId(9), Instant::now()));
    }
}
