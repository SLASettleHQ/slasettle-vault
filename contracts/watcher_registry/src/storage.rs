use soroban_sdk::{contracterror, contracttype, Address, BytesN};

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
    Tally(u64, u64, BytesN<32>),
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

/// A recorded check submitted by an eligible watcher for one round.
/// Stores the watcher's committed endpoint_hash and observed status.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckRecord {
    pub endpoint_hash: BytesN<32>,
    pub status: CheckStatus,
}

/// Running vote counts for one (sla_id, round_id, endpoint_hash) tuple.
/// This contract only counts. It has no concept of a quorum threshold — that
/// judgment belongs entirely to sla_vault, and must stay there.
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
    InvalidRound = 6,
}

pub const INSTANCE_TTL_EXTEND_TO: u32 = 518_400;
pub const INSTANCE_TTL_THRESHOLD: u32 = 501_120; // extend once within ~1 day of expiry
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 518_400;
pub const PERSISTENT_TTL_THRESHOLD: u32 = 501_120;
pub const ROUND_LENGTH_SECONDS: u64 = 60;
