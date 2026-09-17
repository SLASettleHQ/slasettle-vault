use soroban_sdk::{contracterror, contracttype, Address};

/// Storage keys for the watcher_registry contract.
///
/// `Admin`, `WatcherCount`, `Paused` live in instance storage.
/// `Watcher`, `Check`, `Tally` live in persistent storage, with TTL extended
/// on every write that touches them.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Watcher(Address),
    WatcherCount,
    Check(u64, u64, Address),
    Tally(u64, u64),
    Paused,
}

/// A single watcher's report for one round: whether the endpoint was
/// reachable or not, from that watcher's independent vantage point.
#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CheckStatus {
    Up,
    Down,
}

/// Running vote counts for one (sla_id, round_id) pair. This contract only
/// counts. It has no concept of a quorum threshold — that judgment belongs
/// entirely to sla_vault, and must stay there.
#[contracttype]
#[derive(Clone, Default, Debug)]
pub struct RoundTally {
    pub votes_up: u32,
    pub votes_down: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotAuthorized = 1,
    AlreadyInitialized = 2,
    NotAWatcher = 3,
    DuplicateCheck = 4,
    ContractPaused = 5,
}

/// TTL extension window for persistent entries (Watcher, Check, Tally), in
/// ledgers. Rounds are 60 seconds and ledgers close roughly every 5 seconds,
/// so 60 / 5 = 12 ledgers per round. We extend far past a single round,
/// covering roughly 30 days (~518400 ledgers), so a watcher's registration
/// and a round's vote history don't silently expire between checks. This is
/// a v1 constant, not tuned against real storage-rent cost data yet — revisit
/// once there's real usage to measure against.
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 518_400;
pub const PERSISTENT_TTL_THRESHOLD: u32 = 518_400 - 17_280; // extend once within ~1 day of expiry
