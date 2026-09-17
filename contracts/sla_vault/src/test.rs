#![cfg(test)]

use crate::{storage::Error, SlaVault, SlaVaultClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env};

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
