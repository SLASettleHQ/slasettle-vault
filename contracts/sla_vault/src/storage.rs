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
