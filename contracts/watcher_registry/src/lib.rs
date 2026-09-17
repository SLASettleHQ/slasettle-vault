#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

use events::{WatcherRegistered, WatcherRemoved};
use soroban_sdk::{contract, contractimpl, Address, Env};
use storage::{DataKey, Error};

fn require_admin(env: &Env, caller: &Address) -> Result<(), Error> {
    caller.require_auth();
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotAuthorized)?;
    if &admin != caller {
        return Err(Error::NotAuthorized);
    }
    Ok(())
}

#[contract]
pub struct WatcherRegistry;

#[contractimpl]
impl WatcherRegistry {
    /// Auth: `admin`. One-time setup. Fails if already initialized.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::WatcherCount, &0u32);

        Ok(())
    }

    /// Auth: Admin. Adds `watcher` to the eligible set. A no-op, not an
    /// error, if the address is already registered — re-registering an
    /// existing watcher shouldn't double-count WatcherCount.
    pub fn register_watcher(env: Env, caller: Address, watcher: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;

        let key = DataKey::Watcher(watcher.clone());
        if env.storage().persistent().has(&key) {
            return Ok(());
        }

        env.storage().persistent().set(&key, &true);
        env.storage().persistent().extend_ttl(
            &key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WatcherCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::WatcherCount, &(count + 1));

        WatcherRegistered { watcher }.publish(&env);

        Ok(())
    }

    /// Auth: Admin. Removes `watcher` from the eligible set. A no-op if the
    /// address was not registered, symmetric with register_watcher.
    pub fn remove_watcher(env: Env, caller: Address, watcher: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;

        let key = DataKey::Watcher(watcher.clone());
        if !env.storage().persistent().has(&key) {
            return Ok(());
        }

        env.storage().persistent().remove(&key);

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WatcherCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::WatcherCount, &count.saturating_sub(1));

        WatcherRemoved { watcher }.publish(&env);

        Ok(())
    }

    /// Public view. True if `watcher` is currently in the eligible set.
    pub fn is_watcher(env: Env, watcher: Address) -> bool {
        env.storage().persistent().has(&DataKey::Watcher(watcher))
    }

    /// Public view. Total number of currently eligible watchers.
    pub fn get_watcher_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::WatcherCount)
            .unwrap_or(0)
    }
}
