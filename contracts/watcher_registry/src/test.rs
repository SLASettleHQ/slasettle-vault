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

#[test]
fn test_watcher_count_starts_at_zero() {
    let (_, client, admin) = setup();
    client.initialize(&admin);
    assert_eq!(client.get_watcher_count(), 0);
}

#[test]
fn test_register_watcher_adds_to_set_and_increments_count() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);

    client.register_watcher(&admin, &watcher);

    assert!(client.is_watcher(&watcher));
    assert_eq!(client.get_watcher_count(), 1);
}

#[test]
fn test_register_watcher_twice_does_not_double_count() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);

    client.register_watcher(&admin, &watcher);
    client.register_watcher(&admin, &watcher);

    assert_eq!(client.get_watcher_count(), 1);
}

#[test]
fn test_register_watcher_by_non_admin_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let not_admin = Address::generate(&env);
    let watcher = Address::generate(&env);

    let result = client.try_register_watcher(&not_admin, &watcher);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_remove_watcher_removes_from_set_and_decrements_count() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    client.remove_watcher(&admin, &watcher);

    assert!(!client.is_watcher(&watcher));
    assert_eq!(client.get_watcher_count(), 0);
}

#[test]
fn test_remove_watcher_not_registered_is_a_no_op() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);

    // Removing an address that was never registered should not panic and
    // should not underflow WatcherCount.
    client.remove_watcher(&admin, &watcher);
    assert_eq!(client.get_watcher_count(), 0);
}
