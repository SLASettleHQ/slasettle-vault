#![cfg(test)]

use crate::{
    events::CheckSubmitted,
    storage::{CheckRecord, CheckStatus, DataKey, Error},
    WatcherRegistry, WatcherRegistryClient,
};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Event as _,
};

fn setup() -> (Env, WatcherRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(60);
    let contract_id = env.register(WatcherRegistry, ());
    let client = WatcherRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    (env, client, admin)
}

fn dummy_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn sample_hash_a(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn sample_hash_b(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

#[test]
fn test_initialize_succeeds() {
    let (_, client, admin) = setup();
    client.initialize(&admin);
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
}

#[test]
fn test_submit_check_success_updates_tally() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    client.submit_check(
        &watcher,
        &1u64,
        &1u64,
        &dummy_hash(&env),
        &CheckStatus::Down,
    );

    let tally = client.get_round_tally(&1u64, &1u64, &dummy_hash(&env));
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

    // Round 1 submitted at timestamp 60 (current_round = 1)
    client.submit_check(&watcher, &1u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);

    // Advance time to round 2 (timestamp 120, current_round = 2)
    env.ledger().set_timestamp(120);

    // Same watcher, same sla_id, next round — must succeed
    client.submit_check(
        &watcher,
        &1u64,
        &2u64,
        &dummy_hash(&env),
        &CheckStatus::Down,
    );

    assert_eq!(
        client
            .get_round_tally(&1u64, &1u64, &dummy_hash(&env))
            .votes_up,
        1
    );
    assert_eq!(
        client
            .get_round_tally(&1u64, &2u64, &dummy_hash(&env))
            .votes_down,
        1
    );
}

#[test]
fn test_submit_check_while_paused_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);
    client.pause(&admin);

    let result =
        client.try_submit_check(&watcher, &1u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);
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
    let (env, client, admin) = setup();
    client.initialize(&admin);

    let tally = client.get_round_tally(&999u64, &999u64, &dummy_hash(&env));
    assert_eq!(tally.votes_up, 0);
    assert_eq!(tally.votes_down, 0);
}

#[test]
fn test_admin_and_watcher_methods_reject_a_missing_signature() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);
    let unsigned = client.mock_auths(&[]);

    let stranger = Address::generate(&env);
    let register = unsigned.try_register_watcher(&admin, &stranger);
    assert!(matches!(register, Err(Err(_))));
    let pause = unsigned.try_pause(&admin);
    assert!(matches!(pause, Err(Err(_))));
    let vote = unsigned.try_submit_check(
        &watcher,
        &1u64,
        &1u64,
        &BytesN::from_array(&env, &[0u8; 32]),
        &CheckStatus::Down,
    );
    assert!(matches!(vote, Err(Err(_))));

    assert!(!client.is_watcher(&stranger));
    assert_eq!(client.get_watcher_count(), 1);
    assert_eq!(
        client
            .get_round_tally(&1u64, &1u64, &dummy_hash(&env))
            .votes_down,
        0
    );
    assert!(!client.has_watcher_voted(&1u64, &1u64, &watcher));
    client.submit_check(
        &watcher,
        &1u64,
        &1u64,
        &BytesN::from_array(&env, &[0u8; 32]),
        &CheckStatus::Up,
    );
}

// =============================================================================
// v0.2 Specific Tests (A through N)
// =============================================================================

#[test]
fn test_round_freshness_current_round_succeeds() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    env.ledger().set_timestamp(120); // current_round = 2
    let res = client.try_submit_check(&watcher, &10u64, &2u64, &dummy_hash(&env), &CheckStatus::Up);
    assert_eq!(res, Ok(Ok(())));
}

#[test]
fn test_round_freshness_previous_round_succeeds() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    env.ledger().set_timestamp(120); // current_round = 2, previous round = 1
    let res = client.try_submit_check(&watcher, &10u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);
    assert_eq!(res, Ok(Ok(())));
}

#[test]
fn test_round_freshness_future_round_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    env.ledger().set_timestamp(120); // current_round = 2
    let res = client.try_submit_check(&watcher, &10u64, &3u64, &dummy_hash(&env), &CheckStatus::Up);
    assert_eq!(res, Err(Ok(Error::InvalidRound)));
}

#[test]
fn test_round_freshness_stale_round_fails() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    env.ledger().set_timestamp(180); // current_round = 3, previous = 2, round 1 is stale
    let res = client.try_submit_check(&watcher, &10u64, &1u64, &dummy_hash(&env), &CheckStatus::Up);
    assert_eq!(res, Err(Ok(Error::InvalidRound)));
}

#[test]
fn test_round_freshness_round_zero_at_genesis() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    env.ledger().set_timestamp(0); // current_round = 0
                                   // Round 0 must succeed
    let res0 =
        client.try_submit_check(&watcher, &10u64, &0u64, &dummy_hash(&env), &CheckStatus::Up);
    assert_eq!(res0, Ok(Ok(())));

    let watcher2 = Address::generate(&env);
    client.register_watcher(&admin, &watcher2);
    // Round 1 is in the future relative to round 0 and must fail InvalidRound without underflow
    let res1 = client.try_submit_check(
        &watcher2,
        &10u64,
        &1u64,
        &dummy_hash(&env),
        &CheckStatus::Up,
    );
    assert_eq!(res1, Err(Ok(Error::InvalidRound)));
}

#[test]
fn test_endpoint_hash_isolation_and_one_vote_invariant() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher1 = Address::generate(&env);
    let watcher2 = Address::generate(&env);
    client.register_watcher(&admin, &watcher1);
    client.register_watcher(&admin, &watcher2);

    let hash_a = sample_hash_a(&env);
    let hash_b = sample_hash_b(&env);
    let sla_id = 100u64;
    let round_id = 1u64; // current_round is 1 at timestamp 60

    // Watcher 1 submits for hash A (Up)
    let res1 = client.try_submit_check(&watcher1, &sla_id, &round_id, &hash_a, &CheckStatus::Up);
    assert_eq!(res1, Ok(Ok(())));

    // Watcher 1 tries to submit for hash B in the same round -> must fail DuplicateCheck!
    // One-vote-per-watcher invariant: changing endpoint_hash cannot grant a second vote!
    let res_dup =
        client.try_submit_check(&watcher1, &sla_id, &round_id, &hash_b, &CheckStatus::Down);
    assert_eq!(res_dup, Err(Ok(Error::DuplicateCheck)));

    // Watcher 2 submits for hash B (Down)
    let res2 = client.try_submit_check(&watcher2, &sla_id, &round_id, &hash_b, &CheckStatus::Down);
    assert_eq!(res2, Ok(Ok(())));

    // Verify tallies are completely independent
    let tally_a = client.get_round_tally(&sla_id, &round_id, &hash_a);
    assert_eq!(tally_a.votes_up, 1);
    assert_eq!(tally_a.votes_down, 0);

    let tally_b = client.get_round_tally(&sla_id, &round_id, &hash_b);
    assert_eq!(tally_b.votes_up, 0);
    assert_eq!(tally_b.votes_down, 1);
}

#[test]
fn test_check_record_preserves_endpoint_hash_and_status() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    let hash = sample_hash_a(&env);
    client.submit_check(&watcher, &42u64, &1u64, &hash, &CheckStatus::Down);

    env.as_contract(&client.address, || {
        let key = DataKey::Check(42u64, 1u64, watcher.clone());
        let record: CheckRecord = env.storage().persistent().get(&key).unwrap();
        assert_eq!(record.endpoint_hash, hash);
        assert_eq!(record.status, CheckStatus::Down);
    });
}

#[test]
fn test_get_current_round_matches_ledger_timestamp() {
    let (env, client, admin) = setup();
    client.initialize(&admin);

    env.ledger().set_timestamp(0);
    assert_eq!(client.get_current_round(), 0);

    env.ledger().set_timestamp(59);
    assert_eq!(client.get_current_round(), 0);

    env.ledger().set_timestamp(60);
    assert_eq!(client.get_current_round(), 1);

    env.ledger().set_timestamp(119);
    assert_eq!(client.get_current_round(), 1);

    env.ledger().set_timestamp(120);
    assert_eq!(client.get_current_round(), 2);

    env.ledger().set_timestamp(600_000);
    assert_eq!(client.get_current_round(), 10_000);
}

#[test]
fn test_extend_contract_ttl_succeeds() {
    let (_, client, admin) = setup();
    client.initialize(&admin);

    // extend_contract_ttl should execute cleanly without privilege or panic
    client.extend_contract_ttl();
}

#[test]
fn test_check_submitted_event_emitted() {
    let (env, client, admin) = setup();
    client.initialize(&admin);
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    let hash = sample_hash_a(&env);
    client.submit_check(&watcher, &1u64, &1u64, &hash, &CheckStatus::Up);

    let emitted = env.events().all().filter_by_contract(&client.address);
    assert_eq!(
        emitted,
        [CheckSubmitted {
            sla_id: 1u64,
            watcher: watcher.clone(),
            round_id: 1u64,
            endpoint_hash: hash,
            status: CheckStatus::Up,
        }
        .to_xdr(&env, &client.address)],
    );
}
