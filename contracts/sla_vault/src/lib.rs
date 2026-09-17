#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

use events::{BondToppedUp, SlaCreated};
use soroban_sdk::{contract, contractimpl, token, Address, Env};
use storage::{DataKey, Error, SLAConfig, SLAStatus};

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

    /// Auth: `provider`. Locks `bond_amount` of `token` into this contract
    /// and creates a new SLA record. Returns the assigned `sla_id`.
    ///
    /// `uptime_target_bps` is stored for display only in this version — see
    /// the doc comment on SLAConfig. Settlement is per-round, driven by
    /// `penalty_per_breach`, not by an aggregate percentage calculation.
    #[allow(clippy::too_many_arguments)]
    pub fn create_sla(
        env: Env,
        provider: Address,
        token: Address,
        bond_amount: i128,
        uptime_target_bps: u32,
        quorum_threshold: u32,
        penalty_per_breach: i128,
        beneficiary: Address,
    ) -> Result<u64, Error> {
        provider.require_auth();

        if bond_amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if penalty_per_breach <= 0 || penalty_per_breach > bond_amount {
            return Err(Error::InvalidAmount);
        }

        // Standard SAC pattern: the provider's transaction already carries
        // the auth entry for this transfer (from require_auth above plus
        // the transaction's own signature), no separate approve step.
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&provider, &env.current_contract_address(), &bond_amount);

        let sla_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextSlaId)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::NextSlaId, &(sla_id + 1));

        let config = SLAConfig {
            provider: provider.clone(),
            token: token.clone(),
            bond_amount,
            uptime_target_bps,
            quorum_threshold,
            penalty_per_breach,
            beneficiary: beneficiary.clone(),
            status: SLAStatus::Active,
        };
        // Per-SLA records go in persistent storage, not instance storage —
        // instance storage is for small, contract-wide singletons, and this
        // data grows by one entry per SLA created. Same pattern as
        // watcher_registry's Watcher/Check/Tally keys.
        let sla_key = DataKey::Sla(sla_id);
        let balance_key = DataKey::BondBalance(sla_id);
        env.storage().persistent().set(&sla_key, &config);
        env.storage().persistent().set(&balance_key, &bond_amount);
        env.storage().persistent().extend_ttl(
            &sla_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );
        env.storage().persistent().extend_ttl(
            &balance_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        SlaCreated {
            sla_id,
            provider,
            token,
            bond_amount,
            beneficiary,
        }
        .publish(&env);

        Ok(sla_id)
    }

    /// Public view.
    pub fn get_sla(env: Env, sla_id: u64) -> Result<SLAConfig, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Sla(sla_id))
            .ok_or(Error::SlaNotFound)
    }

    /// Public view.
    pub fn get_bond_balance(env: Env, sla_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::BondBalance(sla_id))
            .unwrap_or(0)
    }

    /// Auth: `caller`, and `caller` must be the SLA's original provider.
    /// Adds `amount` of the SLA's own token to its bond balance.
    pub fn top_up_bond(env: Env, caller: Address, sla_id: u64, amount: i128) -> Result<(), Error> {
        caller.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let config: SLAConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Sla(sla_id))
            .ok_or(Error::SlaNotFound)?;

        if config.provider != caller {
            return Err(Error::NotAuthorized);
        }

        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&caller, &env.current_contract_address(), &amount);

        let balance_key = DataKey::BondBalance(sla_id);
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        env.storage().persistent().set(&balance_key, &(balance + amount));
        env.storage().persistent().extend_ttl(
            &balance_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        BondToppedUp { sla_id, amount }.publish(&env);

        Ok(())
    }
}
