#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, Address, Env};
use storage::{DataKey, Error};

#[contract]
pub struct SlaVault;

#[contractimpl]
impl SlaVault {
    /// Auth: `admin`. One-time setup. `watcher_registry` is the deployed
    /// address of the watcher_registry contract this vault will query for
    /// quorum tallies — it must already be deployed and initialized before
    /// this call, per the dependency order in the contract spec.
    pub fn initialize(env: Env, admin: Address, watcher_registry: Address) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WatcherRegistry, &watcher_registry);
        env.storage().instance().set(&DataKey::NextSlaId, &0u64);

        Ok(())
    }
}
