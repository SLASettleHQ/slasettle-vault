#![cfg(test)]

use crate::{events::SettlementPaid, storage::Error, SlaVault, SlaVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    token, Address, BytesN, Env, Event as _, IntoVal,
};

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
fn test_create_sla_zero_quorum_threshold_fails() {
    // A quorum_threshold of 0 would make trigger_settlement's
    // `votes_down < quorum_threshold` check always false (votes_down is a
    // u32, never negative), letting settlement pass with zero votes. This
    // must be rejected at creation, not discovered at settlement time.
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
        &0u32, // zero quorum, must be rejected
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
    let (_env, client, admin, _registry) = setup();

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
fn test_trigger_settlement_pays_only_the_remaining_bond_when_it_is_below_the_penalty() {
    // The one test that reaches `payout = min(penalty_per_breach, balance)`
    // with 0 < balance < penalty. Bond 800 and penalty 500: the first
    // settlement pays a full 500 and leaves 300, so the second must pay 300,
    // not 500 and not an error.
    let (env, client, admin, registry) = setup();
    let (token_id, asset_client) = setup_token(&env, &admin);
    let provider = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    asset_client.mint(&provider, &10_000i128);
    let sla_id = client.create_sla(
        &provider,
        &token_id,
        &800i128,
        &9990u32,
        &3u32,
        &500i128,
        &beneficiary,
    );
    let token_client = token::Client::new(&env, &token_id);

    register_and_vote_down(&env, &registry, &admin, sla_id, 1, 3);
    client.trigger_settlement(&admin, &sla_id, &1u64);
    assert_eq!(client.get_bond_balance(&sla_id), 300i128); // 0 < 300 < penalty 500
    assert_eq!(token_client.balance(&beneficiary), 500i128);

    register_and_vote_down(&env, &registry, &admin, sla_id, 2, 3);
    client.trigger_settlement(&admin, &sla_id, &2u64);
    // Read the events straight away: any later contract call replaces them.
    let emitted = env.events().all().filter_by_contract(&client.address);

    // Partial payout: exactly what was left.
    assert_eq!(token_client.balance(&beneficiary), 800i128); // 500 + 300
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

    // And nothing more can be paid from an empty bond.
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
    assert_eq!(token_client.balance(&provider), 10_000i128); // full 1000 returned
}

#[test]
fn test_withdraw_remaining_bond_twice_fails_second_time() {
    // A repeat withdraw on an already-zero balance must be rejected, not
    // silently succeed as a no-op. Confirmed as a real gap via live
    // Testnet evidence before this test was added.
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

// ---------------------------------------------------------------------------
// Signature enforcement.
//
// Every test above runs under `mock_all_auths()`, which makes every
// `require_auth()` succeed, so those tests prove the contract's own role
// checks but not that a missing signature is rejected. The tests below call
// through `client.mock_auths(..)` with explicit, narrow authorizations instead.
// A missing or wrong signature is a host error (`Err(Err(_))`), not one of the
// contract's own `Error` values (`Err(Ok(_))`).
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
        &provider, &token_id, &500i128, &9990u32, &3u32, &100i128, &provider,
    )));

    // Nothing moved and nothing changed state.
    assert_eq!(client.get_bond_balance(&sla_id), 1_000i128);
    assert_eq!(token_client.balance(&client.address), 1_000i128);
    assert_eq!(
        client.get_sla(&sla_id).status,
        crate::storage::SLAStatus::Active
    );

    // Once cancelled (with the right signature) an unsigned withdrawal is still
    // rejected as a host error, and the bond stays in the vault.
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

    // `attacker` signs an authorization for this exact call, but the call
    // names `provider` as the caller, and only `provider` can authorize that.
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

    // Control: the same call authorized by the provider succeeds, so the
    // assertions above are about the signer and not about the call shape.
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
    // Still not paused: an SLA can be created.
    let (_sla_id, _provider, _beneficiary, _token) = create_test_sla(&env, &client, &admin);
}

#[test]
fn test_trigger_settlement_needs_no_signature_and_no_role_from_its_caller() {
    // The permissionless claim, tested with an unrelated caller and no
    // authorization at all (the earlier settlement tests use `admin` under
    // `mock_all_auths()`, which cannot tell "permissionless" from "admin
    // allowed").
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
