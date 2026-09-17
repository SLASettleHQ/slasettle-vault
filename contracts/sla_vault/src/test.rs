#![cfg(test)]

use crate::{storage::Error, SlaVault, SlaVaultClient};
use soroban_sdk::{testutils::Address as _, token, Address, BytesN, Env};

/// Deploys a Stellar Asset Contract instance for use as the bond token in
/// tests, via the SDK's own test helper for the built-in asset contract —
/// this is the standard way to get a working SEP-41 token in a Soroban test
/// environment without hand-rolling a mock.
fn setup_token(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'static>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token_id = sac.address();
    let asset_client = token::StellarAssetClient::new(env, &token_id);
    (token_id, asset_client)
}

fn setup() -> (Env, SlaVaultClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let registry_id = env.register(watcher_registry::WatcherRegistry, ());
    let registry_client = watcher_registry::WatcherRegistryClient::new(&env, &registry_id);
    registry_client.initialize(&admin);

    let contract_id = env.register(SlaVault, ());
    let client = SlaVaultClient::new(&env, &contract_id);
    client.initialize(&admin, &registry_id);

    (env, client, admin, registry_id)
}

#[test]
fn test_initialize_succeeds() {
    let (_, _client, _admin, _registry) = setup();
}

#[test]
fn test_initialize_twice_fails() {
    let (_, client, admin, registry) = setup();
    let result = client.try_initialize(&admin, &registry);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_create_sla_locks_bond_and_returns_id() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &1_000i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );

    assert_eq!(sla_id, 0);
    assert_eq!(client.get_bond_balance(&sla_id), 1_000i128);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&provider), 9_000i128);
}

#[test]
fn test_create_sla_zero_bond_fails() {
    let (env, client, admin, _registry) = setup();
    let (token_id, _asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);

    let result = client.try_create_sla(
        &provider,
        &token_id,
        &0i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_create_sla_penalty_exceeding_bond_fails() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let result = client.try_create_sla(
        &provider,
        &token_id,
        &1_000i128,
        &9990u32,
        &3u32,
        &1_500i128, // penalty larger than bond — must be rejected
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_create_sla_zero_penalty_fails() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let result = client.try_create_sla(
        &provider,
        &token_id,
        &1_000i128,
        &9990u32,
        &3u32,
        &0i128,
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

fn create_test_sla(
    env: &Env,
    client: &SlaVaultClient<'static>,
    admin: &Address,
) -> (u64, Address, Address, Address) {
    let (token_id, asset_client) = setup_token(env, admin);
    let provider = Address::generate(env);
    let beneficiary = Address::generate(env);
    asset_client.mint(&provider, &10_000i128);

    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &1_000i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );

    (sla_id, provider, beneficiary, token_id)
}

#[test]
fn test_top_up_bond_increases_balance() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    client.top_up_bond(&provider, &sla_id, &300i128);

    assert_eq!(client.get_bond_balance(&sla_id), 1_300i128);
}

#[test]
fn test_top_up_bond_by_non_provider_fails() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, _provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    let not_provider = Address::generate(&env);

    let result = client.try_top_up_bond(&not_provider, &sla_id, &300i128);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_top_up_bond_zero_amount_fails() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    let result = client.try_top_up_bond(&provider, &sla_id, &0i128);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_top_up_bond_unknown_sla_fails() {
    let (env, client, admin, _registry) = setup();
    let (_sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    let result = client.try_top_up_bond(&provider, &999u64, &300i128);
    assert_eq!(result, Err(Ok(Error::SlaNotFound)));
}

fn register_and_vote_down(
    env: &Env,
    registry: &Address,
    admin: &Address,
    sla_id: u64,
    round_id: u64,
    down_votes: u32,
) {
    let registry_client = watcher_registry::WatcherRegistryClient::new(env, registry);
    for _ in 0..down_votes {
        let watcher = Address::generate(env);
        registry_client.register_watcher(admin, &watcher);
        registry_client.submit_check(
            &watcher,
            &sla_id,
            &round_id,
            &BytesN::from_array(env, &[0u8; 32]),
            &watcher_registry::CheckStatus::Down,
        );
    }
}

#[test]
fn test_trigger_settlement_pays_out_when_quorum_confirms_breach() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3); // quorum_threshold is 3

    client.trigger_settlement(&admin, &sla_id, &1u64);

    assert_eq!(client.get_bond_balance(&sla_id), 500i128); // 1000 - 500 penalty
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 500i128);
    assert!(client.is_round_settled(&sla_id, &1u64));
}

#[test]
fn test_trigger_settlement_below_quorum_fails() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 2); // below threshold of 3

    let result = client.try_trigger_settlement(&admin, &sla_id, &1u64);
    assert_eq!(result, Err(Ok(Error::QuorumNotMet)));
}

#[test]
fn test_trigger_settlement_twice_same_round_fails() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);

    client.trigger_settlement(&admin, &sla_id, &1u64);
    let result = client.try_trigger_settlement(&admin, &sla_id, &1u64);

    assert_eq!(result, Err(Ok(Error::AlreadySettled)));
}

#[test]
fn test_trigger_settlement_unknown_sla_fails() {
    let (env, client, admin, _registry) = setup();

    let result = client.try_trigger_settlement(&admin, &999u64, &1u64);
    assert_eq!(result, Err(Ok(Error::SlaNotFound)));
}

#[test]
fn test_trigger_settlement_caps_payout_at_remaining_balance() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    // Drain the bond down to less than one full penalty across two rounds,
    // then confirm the third settlement pays out only what's left rather
    // than erroring or overpaying.
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.trigger_settlement(&admin, &sla_id, &1u64); // balance now 500
    register_and_vote_down(&env, &registry, &admin, sla_id, 2, 3);
    client.trigger_settlement(&admin, &sla_id, &2u64); // balance now 0

    register_and_vote_down(&env, &registry, &admin, sla_id, 3, 3);
    let result = client.try_trigger_settlement(&admin, &sla_id, &3u64);

    // Bond is fully exhausted, so a third confirmed breach cannot pay
    // anything out.
    assert_eq!(result, Err(Ok(Error::BondExhausted)));
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 1_000i128); // 500 + 500, capped correctly both times
}

#[test]
fn test_cancel_sla_by_provider_succeeds() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    client.cancel_sla(&provider, &sla_id);

    let config = client.get_sla(&sla_id);
    assert_eq!(config.status, crate::storage::SLAStatus::Cancelled);
}

#[test]
fn test_cancel_sla_by_non_provider_fails() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, _provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    let not_provider = Address::generate(&env);

    let result = client.try_cancel_sla(&not_provider, &sla_id);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_cancel_sla_unknown_id_fails() {
    let (env, client, admin, _registry) = setup();
    let (_sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    let result = client.try_cancel_sla(&provider, &999u64);
    assert_eq!(result, Err(Ok(Error::SlaNotFound)));
}

#[test]
fn test_cancelled_sla_cannot_be_settled() {
    let (env, client, admin, registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.cancel_sla(&provider, &sla_id);

    let result = client.try_trigger_settlement(&admin, &sla_id, &1u64);
    assert_eq!(result, Err(Ok(Error::SlaNotActive)));
}

#[test]
fn test_withdraw_remaining_bond_after_cancel_succeeds() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    client.cancel_sla(&provider, &sla_id);

    client.withdraw_remaining_bond(&provider, &sla_id);

    assert_eq!(client.get_bond_balance(&sla_id), 0i128);
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&provider), 10_000i128); // full 1000 returned
}

#[test]
fn test_withdraw_remaining_bond_without_cancel_fails() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    let result = client.try_withdraw_remaining_bond(&provider, &sla_id);
    assert_eq!(result, Err(Ok(Error::SlaNotActive)));
}

#[test]
fn test_withdraw_remaining_bond_by_non_provider_fails() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    client.cancel_sla(&provider, &sla_id);
    let not_provider = Address::generate(&env);

    let result = client.try_withdraw_remaining_bond(&not_provider, &sla_id);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_withdraw_remaining_bond_unknown_sla_fails() {
    let (env, client, admin, _registry) = setup();
    let (_sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    let result = client.try_withdraw_remaining_bond(&provider, &999u64);
    assert_eq!(result, Err(Ok(Error::SlaNotFound)));
}

#[test]
fn test_withdraw_remaining_bond_after_partial_settlement_returns_what_is_left() {
    let (env, client, admin, registry) = setup();
    let (sla_id, provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.trigger_settlement(&admin, &sla_id, &1u64); // balance now 500
    client.cancel_sla(&provider, &sla_id);

    client.withdraw_remaining_bond(&provider, &sla_id);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&provider), 9_500i128); // 10000 - 1000 + 500 back
    assert_eq!(token_client.balance(&beneficiary), 500i128);
    assert_eq!(client.get_bond_balance(&sla_id), 0i128);
}

#[test]
fn test_pause_by_non_admin_fails() {
    let (env, client, _admin, _registry) = setup();
    let not_admin = Address::generate(&env);

    let result = client.try_pause(&not_admin);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_paused_contract_rejects_new_sla() {
    let (env, client, admin, _registry) = setup();
    client.pause(&admin);
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let result = client.try_create_sla(
        &provider,
        &token_id,
        &1_000i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_paused_contract_still_allows_settlement_of_existing_sla() {
    // Pausing halts new business, it does not freeze obligations already
    // made — a confirmed breach on an SLA created before the pause must
    // still settle.
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);

    client.pause(&admin);
    client.trigger_settlement(&admin, &sla_id, &1u64);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 500i128);
}
