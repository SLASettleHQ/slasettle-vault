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
