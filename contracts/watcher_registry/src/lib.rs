#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

use events::WatcherRegistered;
use soroban_sdk::{contract, contractimpl, Address, Env};
use storage::{DataKey, Error};

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
}
