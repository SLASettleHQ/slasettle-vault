#![cfg(test)]

use crate::{
    storage::{CheckStatus, Error},
    WatcherRegistry, WatcherRegistryClient,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

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

#[test]
fn test_pause_by_non_admin_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let not_admin = Address::generate(&env);

    let result = client.try_pause(&not_admin);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_pause_then_unpause_by_admin_succeeds() {
    let (_, client, admin) = setup();
    client.initialize(&admin);

    client.pause(&admin);
    client.unpause(&admin);
    // No panic across either call is the assertion here — submit_check's
    // own tests cover the actual paused-rejection behavior below.
}

fn dummy_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

#[test]
fn test_submit_check_success_updates_tally() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    client.submit_check(&watcher, &1u64, &1u64, &dummy_hash(&env), &CheckStatus::Down);

    let tally = client.get_round_tally(&1u64, &1u64);
    assert_eq!(tally.votes_down, 1);
    assert_eq!(tally.votes_up, 0);
}

#[test]
fn test_submit_check_by_non_watcher_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let not_a_watcher = Address::generate(&env);

    let result = client.try_submit_check(
        &not_a_watcher,
        &1u64,
        &1u64,
        &dummy_hash(&env),
        &CheckStatus::Up,
    );
    assert_eq!(result, Err(Ok(Error::NotAWatcher)));
}

#[test]
fn test_submit_check_duplicate_in_same_round_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    client.submit_check(&watcher, &1u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);
    let result = client.try_submit_check(
        &watcher,
        &1u64,
        &1u64,
        &dummy_hash(&env),
        &CheckStatus::Down,
    );

    assert_eq!(result, Err(Ok(Error::DuplicateCheck)));
}

#[test]
fn test_submit_check_same_watcher_different_round_succeeds() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    client.submit_check(&watcher, &1u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);
    // Same watcher, same sla_id, next round — must succeed, this is not a
    // duplicate.
    client.submit_check(&watcher, &1u64, &2u64, &dummy_hash(&env), &CheckStatus::Down);

    assert_eq!(client.get_round_tally(&1u64, &1u64).votes_up, 1);
    assert_eq!(client.get_round_tally(&1u64, &2u64).votes_down, 1);
}

#[test]
fn test_submit_check_while_paused_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);
    client.pause(&admin);

    let result = client.try_submit_check(
        &watcher,
        &1u64,
        &1u64,
        &dummy_hash(&env),
        &CheckStatus::Up,
    );
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_has_watcher_voted() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    let other = Address::generate(&env);
    client.register_watcher(&admin, &watcher);
    client.register_watcher(&admin, &other);

    client.submit_check(&watcher, &1u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);

    assert!(client.has_watcher_voted(&1u64, &1u64, &watcher));
    assert!(!client.has_watcher_voted(&1u64, &1u64, &other));
}

#[test]
fn test_get_round_tally_with_no_votes_returns_zeros() {
    let (_, client, admin) = setup();
    client.initialize(&admin);

    let tally = client.get_round_tally(&999u64, &999u64);
    assert_eq!(tally.votes_up, 0);
    assert_eq!(tally.votes_down, 0);
}
