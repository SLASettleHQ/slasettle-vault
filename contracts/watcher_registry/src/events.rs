use crate::storage::CheckStatus;
use soroban_sdk::{contractevent, Address};

/// Emitted when an address is added to the eligible watcher set.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatcherRegistered {
    #[topic]
    pub watcher: Address,
}

/// Emitted when an address is removed from the eligible watcher set.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatcherRemoved {
    #[topic]
    pub watcher: Address,
}

/// Emitted every time a watcher submits a check for a round. `sla_id` is a
/// topic so the indexer, and anyone else watching the event stream, can
/// filter to a single SLA's check-ins without scanning every event.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckSubmitted {
    #[topic]
    pub sla_id: u64,
    #[topic]
    pub watcher: Address,
    pub round_id: u64,
    pub status: CheckStatus,
}
