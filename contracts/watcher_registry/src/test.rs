#![cfg(test)]

use crate::{storage::Error, WatcherRegistry, WatcherRegistryClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, WatcherRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(WatcherRegistry, ());
    let client = WatcherRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    (env, client, admin)
}

#[test]
fn test_initialize_succeeds() {
    let (_, client, admin) = setup();
    client.initialize(&admin);
    // No assertion beyond "did not panic" here — get_watcher_count is added
    // in a later commit; test_watcher_count_starts_at_zero in that commit
    // covers this once the getter exists.
}

#[test]
fn test_initialize_twice_fails() {
    let (_, client, admin) = setup();
    client.initialize(&admin);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}
