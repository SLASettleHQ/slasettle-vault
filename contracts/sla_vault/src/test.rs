#![cfg(test)]

use crate::{
    events::{SettlementPaid, SlaCreated},
    storage::Error,
    SlaVault, SlaVaultClient,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger, MockAuth, MockAuthInvoke},
    token, Address, BytesN, Env, Event as _, IntoVal,
};

/// Deploys a Stellar Asset Contract instance for use as the bond token in tests.
fn setup_token(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'static>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token_id = sac.address();
    let asset_client = token::StellarAssetClient::new(env, &token_id);
    (token_id, asset_client)
}

fn dummy_endpoint_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn endpoint_hash_a(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn endpoint_hash_b(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

fn setup() -> (Env, SlaVaultClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(60);

    let admin = Address::generate(&env);
    let registry_id = env.register(watcher_registry::WatcherRegistry, ());
    let registry_client = watcher_registry::WatcherRegistryClient::new(&env, &registry_id);
    registry_client.initialize(&admin);

    // Register a baseline set of 5 watchers so default SLA creation with quorum <= 5 succeeds
    for _ in 0..5 {
        let w = Address::generate(&env);
        registry_client.register_watcher(&admin, &w);
    }

    let contract_id = env.register(SlaVault, ());
    let client = SlaVaultClient::new(&env, &contract_id);
    client.initialize(&admin, &registry_id);

    (env, client, admin, registry_id)
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
        &dummy_endpoint_hash(env),
        &1_000i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );

    (sla_id, provider, beneficiary, token_id)
}

fn register_and_vote_down(
    env: &Env,
    registry: &Address,
    admin: &Address,
    sla_id: u64,
    round_id: u64,
    down_votes: u32,
) {
    register_and_vote_down_with_hash(
        env,
        registry,
        admin,
        sla_id,
        round_id,
        &dummy_endpoint_hash(env),
        down_votes,
    );
}

fn register_and_vote_down_with_hash(
    env: &Env,
    registry: &Address,
    admin: &Address,
    sla_id: u64,
    round_id: u64,
    endpoint_hash: &BytesN<32>,
    down_votes: u32,
) {
    let registry_client = watcher_registry::WatcherRegistryClient::new(env, registry);
    let needed_timestamp = round_id * 60;
    if env.ledger().timestamp() < needed_timestamp {
        env.ledger().set_timestamp(needed_timestamp);
    }
    for _ in 0..down_votes {
        let watcher = Address::generate(env);
        registry_client.register_watcher(admin, &watcher);
        registry_client.submit_check(
            &watcher,
            &sla_id,
            &round_id,
            endpoint_hash,
            &watcher_registry::CheckStatus::Down,
        );
    }
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
        &dummy_endpoint_hash(&env),
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
        &dummy_endpoint_hash(&env),
        &0i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_create_sla_zero_quorum_threshold_fails() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let result = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &9990u32,
        &0u32, // zero quorum, must be rejected with InvalidQuorum
        &500i128,
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::InvalidQuorum)));
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
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &9990u32,
        &3u32,
        &1_500i128, // penalty larger than bond
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
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &9990u32,
        &3u32,
        &0i128,
        &beneficiary,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
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
    let (_env, client, admin, _registry) = setup();

    let result = client.try_trigger_settlement(&admin, &999u64, &1u64);
    assert_eq!(result, Err(Ok(Error::SlaNotFound)));
}

#[test]
fn test_trigger_settlement_caps_payout_at_remaining_balance() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);

    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.trigger_settlement(&admin, &sla_id, &1u64); // balance now 500
    register_and_vote_down(&env, &registry, &admin, sla_id, 2, 3);
    client.trigger_settlement(&admin, &sla_id, &2u64); // balance now 0

    register_and_vote_down(&env, &registry, &admin, sla_id, 3, 3);
    let result = client.try_trigger_settlement(&admin, &sla_id, &3u64);

    assert_eq!(result, Err(Ok(Error::BondExhausted)));
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 1_000i128);
}

#[test]
fn test_trigger_settlement_pays_only_the_remaining_bond_when_it_is_below_the_penalty() {
    let (env, client, admin, registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);
    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &800i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );
    let token_client = token::Client::new(&env, &token_id);

    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.trigger_settlement(&admin, &sla_id, &1u64);
    assert_eq!(client.get_bond_balance(&sla_id), 300i128);
    assert_eq!(token_client.balance(&beneficiary), 500i128);

    register_and_vote_down(&env, &registry, &admin, sla_id, 2, 3);
    client.trigger_settlement(&admin, &sla_id, &2u64);
    let emitted = env.events().all().filter_by_contract(&client.address);

    assert_eq!(token_client.balance(&beneficiary), 800i128);
    assert_eq!(client.get_bond_balance(&sla_id), 0i128);
    assert_eq!(token_client.balance(&client.address), 0i128);
    assert!(client.is_round_settled(&sla_id, &2u64));
    assert_eq!(
        emitted,
        [SettlementPaid {
            sla_id,
            round_id: 2u64,
            payout: 300i128,
            beneficiary: beneficiary.clone(),
        }
        .to_xdr(&env, &client.address)],
    );

    register_and_vote_down(&env, &registry, &admin, sla_id, 3, 3);
    assert_eq!(
        client.try_trigger_settlement(&admin, &sla_id, &3u64),
        Err(Ok(Error::BondExhausted))
    );
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
    assert_eq!(token_client.balance(&provider), 10_000i128);
}

#[test]
fn test_withdraw_remaining_bond_twice_fails_second_time() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    client.cancel_sla(&provider, &sla_id);
    client.withdraw_remaining_bond(&provider, &sla_id);

    let result = client.try_withdraw_remaining_bond(&provider, &sla_id);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
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
    client.trigger_settlement(&admin, &sla_id, &1u64);
    client.cancel_sla(&provider, &sla_id);

    client.withdraw_remaining_bond(&provider, &sla_id);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&provider), 9_500i128);
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
        &dummy_endpoint_hash(&env),
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
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);

    client.pause(&admin);
    client.trigger_settlement(&admin, &sla_id, &1u64);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 500i128);
}

// ---------------------------------------------------------------------------
// Signature enforcement
// ---------------------------------------------------------------------------

fn is_host_auth_error<T: core::fmt::Debug, E: core::fmt::Debug>(
    result: &Result<Result<T, E>, Result<Error, soroban_sdk::InvokeError>>,
) -> bool {
    matches!(result, Err(Err(_)))
}

#[test]
fn test_provider_methods_reject_a_missing_signature_and_change_nothing() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    let unsigned = client.mock_auths(&[]);
    let token_client = token::Client::new(&env, &token_id);

    assert!(is_host_auth_error(
        &unsigned.try_top_up_bond(&provider, &sla_id, &100i128)
    ));
    assert!(is_host_auth_error(
        &unsigned.try_cancel_sla(&provider, &sla_id)
    ));
    assert!(is_host_auth_error(&unsigned.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &500i128,
        &9990u32,
        &3u32,
        &100i128,
        &provider,
    )));

    assert_eq!(client.get_bond_balance(&sla_id), 1_000i128);
    assert_eq!(token_client.balance(&client.address), 1_000i128);
    assert_eq!(
        client.get_sla(&sla_id).status,
        crate::storage::SLAStatus::Active
    );

    client.cancel_sla(&provider, &sla_id);
    assert!(is_host_auth_error(
        &unsigned.try_withdraw_remaining_bond(&provider, &sla_id)
    ));
    assert_eq!(client.get_bond_balance(&sla_id), 1_000i128);
}

#[test]
fn test_a_signature_from_someone_else_does_not_authorize_the_provider() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
    let attacker = Address::generate(&env);

    let signed_by_attacker_auths = [MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "cancel_sla",
            args: (&provider, &sla_id).into_val(&env),
            sub_invokes: &[],
        },
    }];
    let signed_by_attacker = client.mock_auths(&signed_by_attacker_auths);
    assert!(is_host_auth_error(
        &signed_by_attacker.try_cancel_sla(&provider, &sla_id)
    ));
    assert_eq!(
        client.get_sla(&sla_id).status,
        crate::storage::SLAStatus::Active
    );

    let signed_by_provider_auths = [MockAuth {
        address: &provider,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "cancel_sla",
            args: (&provider, &sla_id).into_val(&env),
            sub_invokes: &[],
        },
    }];
    let signed_by_provider = client.mock_auths(&signed_by_provider_auths);
    signed_by_provider.cancel_sla(&provider, &sla_id);
    assert_eq!(
        client.get_sla(&sla_id).status,
        crate::storage::SLAStatus::Cancelled
    );
}

#[test]
fn test_admin_methods_reject_a_missing_signature() {
    let (env, client, admin, _registry) = setup();
    let unsigned = client.mock_auths(&[]);

    assert!(is_host_auth_error(&unsigned.try_pause(&admin)));
    let (_sla_id, _provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
}

#[test]
fn test_trigger_settlement_needs_no_signature_and_no_role_from_its_caller() {
    let (env, client, admin, registry) = setup();
    let (sla_id, provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    let stranger = Address::generate(&env);
    assert!(stranger != admin && stranger != provider && stranger != beneficiary);

    client
        .mock_auths(&[])
        .trigger_settlement(&stranger, &sla_id, &1u64);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 500i128);
    assert!(client.is_round_settled(&sla_id, &1u64));
}

// =============================================================================
// v0.2 Specific Tests (A through T)
// =============================================================================

#[test]
fn test_create_sla_stores_endpoint_hash() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let hash = endpoint_hash_a(&env);
    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &hash,
        &1_000i128,
        &9900u32,
        &2u32,
        &200i128,
        &beneficiary,
    );

    let config = client.get_sla(&sla_id);
    assert_eq!(config.endpoint_hash, hash);
    assert_eq!(config.uptime_target_bps, 9900);
    assert_eq!(config.quorum_threshold, 2);
    assert_eq!(config.penalty_per_breach, 200);
}

#[test]
fn test_create_sla_uptime_target_bounds() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &50_000i128);

    // Min valid: 1 bps succeeds
    let res_min = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &1u32,
        &1u32,
        &100i128,
        &beneficiary,
    );
    assert!(res_min.is_ok());

    // Max valid: 10_000 bps succeeds
    let res_max = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &10_000u32,
        &1u32,
        &100i128,
        &beneficiary,
    );
    assert!(res_max.is_ok());

    // 0 bps fails with InvalidUptimeTarget
    let res_zero = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &0u32,
        &1u32,
        &100i128,
        &beneficiary,
    );
    assert_eq!(res_zero, Err(Ok(Error::InvalidUptimeTarget)));

    // 10_001 bps fails with InvalidUptimeTarget
    let res_over = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &10_001u32,
        &1u32,
        &100i128,
        &beneficiary,
    );
    assert_eq!(res_over, Err(Ok(Error::InvalidUptimeTarget)));
}

#[test]
fn test_create_sla_quorum_validation() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &50_000i128);

    // setup() registered 5 watchers.
    // quorum == watcher_count (5) succeeds
    let res_exact = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &9900u32,
        &5u32,
        &100i128,
        &beneficiary,
    );
    assert!(res_exact.is_ok());

    // quorum > watcher_count (6 > 5) fails with InvalidQuorum
    let res_over = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &9900u32,
        &6u32,
        &100i128,
        &beneficiary,
    );
    assert_eq!(res_over, Err(Ok(Error::InvalidQuorum)));
}

#[test]
fn test_create_sla_zero_watchers_fails_invalid_quorum() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    // Fresh registry with 0 watchers
    let empty_registry_id = env.register(watcher_registry::WatcherRegistry, ());
    let reg_client = watcher_registry::WatcherRegistryClient::new(&env, &empty_registry_id);
    reg_client.initialize(&admin);

    let vault_id = env.register(SlaVault, ());
    let client = SlaVaultClient::new(&env, &vault_id);
    client.initialize(&admin, &empty_registry_id);

    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    // Any quorum_threshold > 0 must fail InvalidQuorum when watcher_count == 0
    let res = client.try_create_sla(
        &provider,
        &token_id,
        &dummy_endpoint_hash(&env),
        &1_000i128,
        &9900u32,
        &1u32,
        &100i128,
        &beneficiary,
    );
    assert_eq!(res, Err(Ok(Error::InvalidQuorum)));
}

#[test]
fn test_settlement_endpoint_hash_isolation_and_verification() {
    let (env, client, admin, registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let hash_a = endpoint_hash_a(&env);
    let hash_b = endpoint_hash_b(&env);

    // Create SLA committed specifically to hash_a with quorum threshold 3
    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &hash_a,
        &1_000i128,
        &9900u32,
        &3u32,
        &500i128,
        &beneficiary,
    );

    // Round 1: 3 watchers vote Down on wrong hash (hash_b)
    register_and_vote_down_with_hash(&env, &registry, &admin, sla_id, 1, &hash_b, 3);

    // Attempting to settle round 1 fails with QuorumNotMet because SLA only queries hash_a tally
    let res_wrong = client.try_trigger_settlement(&admin, &sla_id, &1u64);
    assert_eq!(res_wrong, Err(Ok(Error::QuorumNotMet)));
    assert_eq!(client.get_bond_balance(&sla_id), 1_000i128);

    // Round 2: 3 watchers vote Down on correct hash (hash_a)
    register_and_vote_down_with_hash(&env, &registry, &admin, sla_id, 2, &hash_a, 3);

    // Settlement on round 2 succeeds and pays out
    let res_correct = client.try_trigger_settlement(&admin, &sla_id, &2u64);
    assert_eq!(res_correct, Ok(Ok(())));
    assert_eq!(client.get_bond_balance(&sla_id), 500i128);
}

#[test]
fn test_historical_valid_round_can_settle_later() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);

    // Votes collected for round 1 at timestamp 60
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);

    // Advance ledger far into the future (e.g. round 500 = timestamp 30_000)
    env.ledger().set_timestamp(30_000);

    // Settlement of round 1 does NOT enforce round freshness and settles successfully
    client.trigger_settlement(&admin, &sla_id, &1u64);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 500i128);
    assert!(client.is_round_settled(&sla_id, &1u64));
}

#[test]
fn test_cancel_sla_idempotency_second_call_fails() {
    let (env, client, admin, _registry) = setup();
    let (sla_id, provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);

    // First cancel succeeds
    let res1 = client.try_cancel_sla(&provider, &sla_id);
    assert_eq!(res1, Ok(Ok(())));

    // Second cancel fails SlaNotActive
    let res2 = client.try_cancel_sla(&provider, &sla_id);
    assert_eq!(res2, Err(Ok(Error::SlaNotActive)));
}

#[test]
fn test_bond_balance_never_becomes_negative() {
    let (env, client, admin, registry) = setup();
    let (sla_id, _provider, beneficiary, token_id) = create_test_sla(&env, &client, &admin);

    // Total bond = 1000, penalty = 500
    // Round 1 breach
    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.trigger_settlement(&admin, &sla_id, &1u64);
    assert_eq!(client.get_bond_balance(&sla_id), 500i128);

    // Round 2 breach
    register_and_vote_down(&env, &registry, &admin, sla_id, 2, 3);
    client.trigger_settlement(&admin, &sla_id, &2u64);
    assert_eq!(client.get_bond_balance(&sla_id), 0i128);

    // Round 3 breach on empty bond fails with BondExhausted without going negative
    register_and_vote_down(&env, &registry, &admin, sla_id, 3, 3);
    let res = client.try_trigger_settlement(&admin, &sla_id, &3u64);
    assert_eq!(res, Err(Ok(Error::BondExhausted)));
    assert_eq!(client.get_bond_balance(&sla_id), 0i128);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 1_000i128);
}

#[test]
fn test_extend_contract_ttl_succeeds() {
    let (_, client, _admin, _registry) = setup();
    client.extend_contract_ttl();
}

#[test]
fn test_sla_created_event_emitted() {
    let (env, client, admin, _registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);

    let hash = endpoint_hash_a(&env);
    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &hash,
        &1_000i128,
        &9950u32,
        &3u32,
        &250i128,
        &beneficiary,
    );

    let emitted = env.events().all().filter_by_contract(&client.address);
    assert_eq!(
        emitted,
        [SlaCreated {
            sla_id,
            provider,
            token: token_id,
            endpoint_hash: hash,
            bond_amount: 1_000i128,
            uptime_target_bps: 9950u32,
            quorum_threshold: 3u32,
            penalty_per_breach: 250i128,
            beneficiary,
        }
        .to_xdr(&env, &client.address)],
    );
}
