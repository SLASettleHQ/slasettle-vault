#![no_std]

mod events;
mod storage;

#[cfg(test)]
mod test;

use events::{BondToppedUp, BondWithdrawn, SettlementPaid, SlaCancelled, SlaCreated};
use soroban_sdk::{contract, contractimpl, token, Address, Env};
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
        require_not_paused(&env)?;

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

    /// No `require_auth` on `caller` at all — this is the permissionless
    /// keeper function, deliberately. `caller` is recorded only in the
    /// event, not authorized, because they aren't moving any of their own
    /// funds. Anyone can call this the moment they believe quorum has
    /// formed; the idempotency check below (step 2) is what makes that
    /// safe.
    ///
    /// Every step here is a real, distinct check. Do not collapse them.
    pub fn trigger_settlement(env: Env, caller: Address, sla_id: u64, round_id: u64) -> Result<(), Error> {
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
        if env.storage().persistent().get(&settled_key).unwrap_or(false) {
            return Err(Error::AlreadySettled);
        }

        // 3. Cross-contract call to watcher_registry for the raw tally.
        //    sla_vault owns quorum interpretation; watcher_registry only
        //    counts. Do not move this logic into the registry.
        let registry_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::WatcherRegistry)
            .ok_or(Error::NotAuthorized)?;
        let registry_client = watcher_registry::WatcherRegistryClient::new(&env, &registry_address);
        let tally = registry_client.get_round_tally(&sla_id, &round_id);

        // 4. Quorum check, sla_vault's judgment call.
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

        // 6. Transfer, decrement balance, mark settled, emit. In that
        //    order — the balance write and the settled-flag write both
        //    happen before the event, so a reader of the event can trust
        //    both are already true on-chain.
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &config.beneficiary, &payout);

        env.storage().persistent().set(&balance_key, &(balance - payout));
        env.storage().persistent().set(&settled_key, &true);
        env.storage().persistent().extend_ttl(
            &settled_key,
            storage::PERSISTENT_TTL_THRESHOLD,
            storage::PERSISTENT_TTL_EXTEND_TO,
        );

        // caller is intentionally not stored — it's informational only,
        // captured in the transaction itself, not part of contract state.
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
    /// This is a required, visible step before withdraw_remaining_bond will
    /// allow anything — a provider cannot quietly drain their bond while
    /// still advertising an active guarantee. Cancelling is a public state
    /// change anyone watching the status page can see.
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

        config.status = SLAStatus::Cancelled;
        env.storage().persistent().set(&sla_key, &config);

        SlaCancelled { sla_id }.publish(&env);

        Ok(())
    }

    /// Auth: `caller`, must be the SLA's provider. Requires the SLA is
    /// already Cancelled — reusing SlaNotActive as the error here on
    /// purpose, since the condition is the same either way: "you can't
    /// touch this bond while the SLA is still live."
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

        if balance > 0 {
            let token_client = token::Client::new(&env, &config.token);
            token_client.transfer(&env.current_contract_address(), &caller, &balance);
            env.storage().persistent().set(&balance_key, &0i128);
        }

        BondWithdrawn {
            sla_id,
            amount: balance,
        }
        .publish(&env);

        Ok(())
    }

    /// Auth: Admin. Stops create_sla from accepting new SLAs. Deliberately
    /// does not stop top_up_bond or trigger_settlement — an existing
    /// provider should still be able to top up a bond, and a confirmed
    /// breach should still settle, even while the contract is paused to new
    /// business. Pausing halts new commitments, not existing obligations.
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
}
