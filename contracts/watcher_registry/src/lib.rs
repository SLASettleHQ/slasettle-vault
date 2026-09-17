#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

use events::{CheckSubmitted, WatcherRegistered, WatcherRemoved};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};
use storage::{DataKey, Error, RoundTally};
pub use storage::CheckStatus;

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

    /// Auth: Admin. Stops submit_check from accepting new checks.
    /// Registration and view functions are unaffected — pausing halts
    /// incoming data, it does not lock the contract entirely.
    pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    /// Auth: Admin.
    pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Auth: `watcher` (the caller must be the watcher whose vote this is).
    ///
    /// Records one watcher's independent check for one round. Exactly one
    /// vote per watcher per round — a duplicate submission is rejected
    /// outright, not silently overwritten, so a watcher cannot "correct"
    /// their vote after seeing how the round is trending.
    ///
    /// Known limitation, not solved in this version: because contract state
    /// is public, a watcher who submits late can see how earlier watchers
    /// voted before deciding what to submit. A dishonest watcher could copy
    /// the emerging majority instead of reporting what they actually
    /// observed. The standard fix is a commit-reveal scheme, which roughly
    /// doubles the transaction count per watcher per round. This version
    /// ships without it — see SLASettle-contract-spec.md for the full
    /// reasoning. Revisit before this handles anything beyond a demo bond.
    ///
    /// `endpoint_hash` is stored on the check record but not validated
    /// against anything here — it exists so a later dispute can verify
    /// which endpoint a watcher actually checked, cross-referenced against
    /// sla_vault's stored config. That cross-check is not this contract's
    /// job.
    pub fn submit_check(
        env: Env,
        watcher: Address,
        sla_id: u64,
        round_id: u64,
        endpoint_hash: BytesN<32>,
        status: CheckStatus,
    ) -> Result<(), Error> {
        watcher.require_auth();

        if Self::is_paused(&env) {
            return Err(Error::ContractPaused);
        }

        if !env
            .storage()
            .persistent()
            .has(&DataKey::Watcher(watcher.clone()))
        {
            return Err(Error::NotAWatcher);
        }

        let check_key = DataKey::Check(sla_id, round_id, watcher.clone());
        if env.storage().persistent().has(&check_key) {
            return Err(Error::DuplicateCheck);
        }

        // endpoint_hash is part of the record but intentionally unused in
        // this version's logic beyond storage — see doc comment above.
        let _ = &endpoint_hash;
        env.storage().persistent().set(&check_key, &status);
        env.storage().persistent().extend_ttl(
            &check_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        let tally_key = DataKey::Tally(sla_id, round_id);
        let mut tally: RoundTally = env
            .storage()
            .persistent()
            .get(&tally_key)
            .unwrap_or_default();
        match status {
            CheckStatus::Up => tally.votes_up += 1,
            CheckStatus::Down => tally.votes_down += 1,
        }
        env.storage().persistent().set(&tally_key, &tally);
        env.storage().persistent().extend_ttl(
            &tally_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        CheckSubmitted {
            sla_id,
            watcher,
            round_id,
            status,
        }
        .publish(&env);

        Ok(())
    }

    /// Public view. Raw vote counts for a round — no quorum interpretation
    /// happens here, by design. Returns zeros if no checks have landed yet
    /// for this (sla_id, round_id), rather than erroring.
    pub fn get_round_tally(env: Env, sla_id: u64, round_id: u64) -> RoundTally {
        env.storage()
            .persistent()
            .get(&DataKey::Tally(sla_id, round_id))
            .unwrap_or_default()
    }

    /// Public view.
    pub fn has_watcher_voted(env: Env, sla_id: u64, round_id: u64, watcher: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Check(sla_id, round_id, watcher))
    }
}
