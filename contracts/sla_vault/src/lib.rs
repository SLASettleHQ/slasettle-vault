#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

mod watcher_registry_contract {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/watcher_registry.wasm");
}

use events::{BondToppedUp, BondWithdrawn, SettlementPaid, SlaCancelled, SlaCreated};
use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env};
use storage::{DataKey, Error, SLAConfig, SLAStatus};

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

fn require_not_paused(env: &Env) -> Result<(), Error> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(Error::ContractPaused);
    }
    Ok(())
}

fn bump_contract_ttl(env: &Env) {
    env.storage().instance().extend_ttl(
        storage::INSTANCE_TTL_THRESHOLD,
        storage::INSTANCE_TTL_EXTEND_TO,
    );
}

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

        bump_contract_ttl(&env);

        Ok(())
    }

    /// Public maintenance entrypoint. Extends instance and code TTL for this live
    /// contract without requiring an authorized caller or modifying contract state.
    pub fn extend_contract_ttl(env: Env) {
        bump_contract_ttl(&env);
    }

    /// Auth: `provider`. Locks `bond_amount` of `token` into this contract
    /// and creates a new SLA record. Returns the assigned `sla_id`.
    ///
    /// Validations:
    /// - `bond_amount > 0` (Error::InvalidAmount)
    /// - `penalty_per_breach > 0 && penalty_per_breach <= bond_amount` (Error::InvalidAmount)
    /// - `uptime_target_bps in 1..=10_000` (Error::InvalidUptimeTarget)
    /// - `quorum_threshold > 0` (Error::InvalidQuorum)
    /// - `quorum_threshold <= watcher_registry.get_watcher_count()` (Error::InvalidQuorum, also fails if watcher_count == 0)
    #[allow(clippy::too_many_arguments)]
    pub fn create_sla(
        env: Env,
        provider: Address,
        token: Address,
        endpoint_hash: BytesN<32>,
        bond_amount: i128,
        uptime_target_bps: u32,
        quorum_threshold: u32,
        penalty_per_breach: i128,
        beneficiary: Address,
    ) -> Result<u64, Error> {
        provider.require_auth();
        require_not_paused(&env)?;
        bump_contract_ttl(&env);

        if bond_amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if penalty_per_breach <= 0 || penalty_per_breach > bond_amount {
            return Err(Error::InvalidAmount);
        }
        if uptime_target_bps == 0 || uptime_target_bps > 10_000 {
            return Err(Error::InvalidUptimeTarget);
        }
        if quorum_threshold == 0 {
            return Err(Error::InvalidQuorum);
        }

        let registry_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::WatcherRegistry)
            .ok_or(Error::NotAuthorized)?;
        let registry_client = watcher_registry_contract::Client::new(&env, &registry_address);
        let watcher_count = registry_client.get_watcher_count();
        if watcher_count == 0 || quorum_threshold > watcher_count {
            return Err(Error::InvalidQuorum);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&provider, env.current_contract_address(), &bond_amount);

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
            endpoint_hash: endpoint_hash.clone(),
            bond_amount,
            uptime_target_bps,
            quorum_threshold,
            penalty_per_breach,
            beneficiary: beneficiary.clone(),
            status: SLAStatus::Active,
        };

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
            endpoint_hash,
            bond_amount,
            uptime_target_bps,
            quorum_threshold,
            penalty_per_breach,
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
        token_client.transfer(&caller, env.current_contract_address(), &amount);

        let balance_key = DataKey::BondBalance(sla_id);
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&balance_key, &(balance + amount));
        env.storage().persistent().extend_ttl(
            &balance_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );
        bump_contract_ttl(&env);

        BondToppedUp { sla_id, amount }.publish(&env);

        Ok(())
    }

    /// Permissionless keeper function.
    pub fn trigger_settlement(
        env: Env,
        caller: Address,
        sla_id: u64,
        round_id: u64,
    ) -> Result<(), Error> {
        // 1. Load config, must exist and be Active.
        let config: SLAConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Sla(sla_id))
            .ok_or(Error::SlaNotFound)?;
        if config.status != SLAStatus::Active {
            return Err(Error::SlaNotActive);
        }

        // 2. Idempotency — one payout per round, ever.
        let settled_key = DataKey::SettledRounds(sla_id, round_id);
        if env
            .storage()
            .persistent()
            .get(&settled_key)
            .unwrap_or(false)
        {
            return Err(Error::AlreadySettled);
        }

        // 3. Cross-contract call to watcher_registry for the endpoint-bound raw tally.
        let registry_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::WatcherRegistry)
            .ok_or(Error::NotAuthorized)?;
        let registry_client = watcher_registry_contract::Client::new(&env, &registry_address);
        let tally = registry_client.get_round_tally(&sla_id, &round_id, &config.endpoint_hash);

        // 4. Quorum check: tally.votes_down >= config.quorum_threshold
        if tally.votes_down < config.quorum_threshold {
            return Err(Error::QuorumNotMet);
        }

        // 5. Payout capped at what's actually left in the bond.
        let balance_key = DataKey::BondBalance(sla_id);
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        let payout = if config.penalty_per_breach < balance {
            config.penalty_per_breach
        } else {
            balance
        };
        if payout <= 0 {
            return Err(Error::BondExhausted);
        }

        // 6. Transfer, decrement balance, mark settled, emit.
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(
            &env.current_contract_address(),
            &config.beneficiary,
            &payout,
        );

        env.storage()
            .persistent()
            .set(&balance_key, &(balance - payout));
        env.storage().persistent().extend_ttl(
            &balance_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        env.storage().persistent().set(&settled_key, &true);
        env.storage().persistent().extend_ttl(
            &settled_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        bump_contract_ttl(&env);

        let _ = &caller;

        SettlementPaid {
            sla_id,
            round_id,
            payout,
            beneficiary: config.beneficiary,
        }
        .publish(&env);

        Ok(())
    }

    /// Public view.
    pub fn is_round_settled(env: Env, sla_id: u64, round_id: u64) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::SettledRounds(sla_id, round_id))
            .unwrap_or(false)
    }

    /// Auth: `caller`, must be the SLA's provider. Marks the SLA Cancelled.
    /// Only active SLAs may be cancelled; repeated calls fail with SlaNotActive.
    pub fn cancel_sla(env: Env, caller: Address, sla_id: u64) -> Result<(), Error> {
        caller.require_auth();

        let sla_key = DataKey::Sla(sla_id);
        let mut config: SLAConfig = env
            .storage()
            .persistent()
            .get(&sla_key)
            .ok_or(Error::SlaNotFound)?;

        if config.provider != caller {
            return Err(Error::NotAuthorized);
        }

        if config.status != SLAStatus::Active {
            return Err(Error::SlaNotActive);
        }

        config.status = SLAStatus::Cancelled;
        env.storage().persistent().set(&sla_key, &config);
        env.storage().persistent().extend_ttl(
            &sla_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );
        bump_contract_ttl(&env);

        SlaCancelled { sla_id }.publish(&env);

        Ok(())
    }

    /// Auth: `caller`, must be the SLA's provider. Requires the SLA is
    /// already Cancelled.
    pub fn withdraw_remaining_bond(env: Env, caller: Address, sla_id: u64) -> Result<(), Error> {
        caller.require_auth();

        let config: SLAConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Sla(sla_id))
            .ok_or(Error::SlaNotFound)?;

        if config.provider != caller {
            return Err(Error::NotAuthorized);
        }
        if config.status != SLAStatus::Cancelled {
            return Err(Error::SlaNotActive);
        }

        let balance_key = DataKey::BondBalance(sla_id);
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);

        if balance <= 0 {
            return Err(Error::InvalidAmount);
        }

        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &caller, &balance);
        env.storage().persistent().set(&balance_key, &0i128);
        env.storage().persistent().extend_ttl(
            &balance_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );
        bump_contract_ttl(&env);

        BondWithdrawn {
            sla_id,
            amount: balance,
        }
        .publish(&env);

        Ok(())
    }

    /// Auth: Admin. Stops create_sla from accepting new SLAs.
    pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        bump_contract_ttl(&env);
        Ok(())
    }

    /// Auth: Admin.
    pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        bump_contract_ttl(&env);
        Ok(())
    }
}
