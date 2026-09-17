use soroban_sdk::{contracterror, contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    WatcherRegistry,
    NextSlaId,
    Sla(u64),
    BondBalance(u64),
    SettledRounds(u64, u64),
    Paused,
}

#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SLAStatus {
    Active,
    Cancelled,
}

/// A provider's bonded uptime guarantee.
///
/// `uptime_target_bps` is stored and can be shown in a UI, but this version
/// does not compute an actual uptime percentage over a billing period and
/// does not enforce that number. What actually happens is simpler: every
/// time a round is independently confirmed down by quorum, a fixed penalty
/// fires. That is a genuinely different, simpler product than "enforce a
/// 99.9% monthly SLA" — true aggregate-uptime enforcement is out of scope
/// for this version. See SLASettle-contract-spec.md.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SLAConfig {
    pub provider: Address,
    pub token: Address,
    pub bond_amount: i128,
    pub uptime_target_bps: u32,
    pub quorum_threshold: u32,
    pub penalty_per_breach: i128,
    pub beneficiary: Address,
    pub status: SLAStatus,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotAuthorized = 1,
    AlreadyInitialized = 2,
    SlaNotActive = 3,
    AlreadySettled = 4,
    QuorumNotMet = 5,
    BondExhausted = 6,
    InvalidAmount = 7,
    ContractPaused = 8,
    SlaNotFound = 9,
}

/// TTL extension window for persistent entries (Sla, BondBalance,
/// SettledRounds), in ledgers. Rounds are 60 seconds and ledgers close
/// roughly every 5 seconds, so 60 / 5 = 12 ledgers per round. We extend far
/// past a single round, covering roughly 30 days (~518400 ledgers), so an
/// SLA's config and settlement history don't silently expire between checks.
/// This is a v1 constant, not tuned against real storage-rent cost data yet —
/// revisit once there's real usage to measure against.
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 518_400;
pub const PERSISTENT_TTL_THRESHOLD: u32 = 518_400 - 17_280; // extend once within ~1 day of expiry
