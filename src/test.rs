use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{Client as TokenClient, StellarAssetClient},
    vec, Address, Env, String,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn setup(
    env: &Env,
) -> (
    Address,
    StellarAssetClient<'_>,
    Address,
    NovaEventsContractClient<'_>,
) {
    let token_admin = Address::generate(env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = token_contract.address();
    let token_admin_client = StellarAssetClient::new(env, &token_addr);

    let contract_id = env.register(NovaEventsContract, ());
    let client = NovaEventsContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &token_addr);

    (token_addr, token_admin_client, contract_id, client)
}

fn default_tiers(env: &Env) -> Vec<TierInput> {
    vec![
        env,
        TierInput {
            name: String::from_str(env, "General"),
            price: 10_000_000_i128, // 1 USDC
            supply_cap: 100,
        },
        TierInput {
            name: String::from_str(env, "VIP"),
            price: 50_000_000_i128, // 5 USDC
            supply_cap: 20,
        },
    ]
}

fn create_test_event(env: &Env, client: &NovaEventsContractClient, organizer: &Address) -> u32 {
    client.create_event(
        organizer,
        &String::from_str(env, "Stellar Summit"),
        &String::from_str(env, "The biggest Stellar dev conference"),
        &String::from_str(env, "San Francisco"),
        &1_750_000_000_u64,
        &500_000_000_i128,
        &default_tiers(env),
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn test_create_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    assert_eq!(client.event_count(), 0);

    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    assert_eq!(event_id, 0);
    assert_eq!(client.event_count(), 1);

    let event = client.get_event(&0);
    assert_eq!(event.organizer, organizer);
    assert_eq!(event.balance, 0);
    assert_eq!(event.status, EventStatus::Active);
    assert_eq!(event.funding_goal, 500_000_000_i128);

    let tiers = client.get_tiers(&0);
    assert_eq!(tiers.len(), 2);
    assert_eq!(tiers.get(0).unwrap().tickets_sold, 0);
    assert_eq!(tiers.get(1).unwrap().price, 50_000_000_i128);
}

#[test]
fn test_multiple_events_get_distinct_ids() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let id0 = create_test_event(&env, &client, &organizer);
    let id1 = create_test_event(&env, &client, &organizer);
    let id2 = create_test_event(&env, &client, &organizer);

    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(client.event_count(), 3);
}

#[test]
fn test_buy_ticket() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128); // 10 USDC

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0); // General tier: 1 USDC

    assert_eq!(ticket_id, 0);
    assert_eq!(client.ticket_count(&event_id), 1);

    let ticket = client.get_ticket(&event_id, &ticket_id);
    assert_eq!(ticket.owner, buyer);
    assert_eq!(ticket.tier_index, 0);
    assert!(!ticket.redeemed);

    let event = client.get_event(&event_id);
    assert_eq!(event.balance, 10_000_000_i128);

    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&buyer), 90_000_000_i128);

    let tiers = client.get_tiers(&event_id);
    assert_eq!(tiers.get(0).unwrap().tickets_sold, 1);
}

#[test]
fn test_buy_multiple_tickets_different_tiers() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer_a = Address::generate(&env);
    let buyer_b = Address::generate(&env);

    token_admin.mint(&buyer_a, &100_000_000_i128);
    token_admin.mint(&buyer_b, &500_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);

    let ticket_a = client.buy_ticket(&buyer_a, &event_id, &0); // General: 1 USDC
    let ticket_b = client.buy_ticket(&buyer_b, &event_id, &1); // VIP: 5 USDC

    assert_eq!(ticket_a, 0);
    assert_eq!(ticket_b, 1);

    let event = client.get_event(&event_id);
    assert_eq!(event.balance, 60_000_000_i128);

    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&buyer_a), 90_000_000_i128);
    assert_eq!(token.balance(&buyer_b), 450_000_000_i128);
}

#[test]
fn test_redeem_ticket() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    assert!(!client.get_ticket(&event_id, &ticket_id).redeemed);

    client.redeem_ticket(&organizer, &event_id, &ticket_id);

    assert!(client.get_ticket(&event_id, &ticket_id).redeemed);
}

#[test]
fn test_end_event_changes_status_to_ended() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    assert_eq!(client.get_event(&event_id).status, EventStatus::Active);

    client.end_event(&organizer, &event_id);

    assert_eq!(client.get_event(&event_id).status, EventStatus::Ended);
}

#[test]
fn test_end_event_by_non_organizer_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let impostor = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    let result = client.try_end_event(&impostor, &event_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    // Status must be unchanged.
    assert_eq!(client.get_event(&event_id).status, EventStatus::Active);
}

#[test]
fn test_end_already_ended_event_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    client.end_event(&organizer, &event_id);

    let result = client.try_end_event(&organizer, &event_id);
    assert_eq!(result, Err(Ok(Error::EventNotActive)));
}

#[test]
fn test_sponsor_event_blocked_after_end_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor = Address::generate(&env);
    token_admin.mint(&sponsor, &100_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.end_event(&organizer, &event_id);

    let result = client.try_sponsor_event(&sponsor, &event_id, &10_000_000_i128);
    assert_eq!(result, Err(Ok(Error::EventNotActive)));
}

#[test]
fn test_sponsorship_is_publicly_recorded() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);

    token_admin.mint(&sponsor_a, &1_000_000_000_i128);
    token_admin.mint(&sponsor_b, &1_000_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);

    client.sponsor_event(&sponsor_a, &event_id, &200_000_000_i128);
    client.sponsor_event(&sponsor_b, &event_id, &300_000_000_i128);

    let sponsorships = client.get_sponsorships(&event_id);
    assert_eq!(sponsorships.len(), 2);
    assert_eq!(sponsorships.get(0).unwrap().sponsor, sponsor_a);
    assert_eq!(sponsorships.get(0).unwrap().amount, 200_000_000_i128);
    assert_eq!(sponsorships.get(1).unwrap().sponsor, sponsor_b);
    assert_eq!(sponsorships.get(1).unwrap().amount, 300_000_000_i128);

    let event = client.get_event(&event_id);
    assert_eq!(event.balance, 500_000_000_i128);

    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&sponsor_a), 800_000_000_i128);
    assert_eq!(token.balance(&sponsor_b), 700_000_000_i128);
}

#[test]
fn test_sold_out_tier_blocks_purchase() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let tiers = vec![
        &env,
        TierInput {
            name: String::from_str(&env, "Limited"),
            price: 10_000_000_i128,
            supply_cap: 1,
        },
    ];
    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Exclusive"),
        &String::from_str(&env, "One ticket only"),
        &String::from_str(&env, "Secret venue"),
        &1_750_000_000_u64,
        &10_000_000_i128,
        &tiers,
    );

    let buyer_a = Address::generate(&env);
    let buyer_b = Address::generate(&env);
    token_admin.mint(&buyer_a, &100_000_000_i128);
    token_admin.mint(&buyer_b, &100_000_000_i128);

    client.buy_ticket(&buyer_a, &event_id, &0);

    let result = client.try_buy_ticket(&buyer_b, &event_id, &0);
    assert_eq!(result, Err(Ok(Error::TierSoldOut)));
}

#[test]
fn test_double_redeem_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    client.redeem_ticket(&organizer, &event_id, &ticket_id);

    let result = client.try_redeem_ticket(&organizer, &event_id, &ticket_id);
    assert_eq!(result, Err(Ok(Error::AlreadyRedeemed)));
}

#[test]
fn test_double_initialize_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, _, _, client) = setup(&env);

    // setup() already called initialize once; a second call must fail
    let admin = Address::generate(&env);
    let result = client.try_initialize(&admin, &token_addr);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
#[should_panic]
fn test_initialize_requires_admin_auth() {
    let env = Env::default();
    // Deliberately not mocking auths — admin.require_auth() must reject this call.

    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = token_contract.address();

    let contract_id = env.register(NovaEventsContract, ());
    let client = NovaEventsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &token_addr);
}

#[test]
fn test_sponsor_nonexistent_event_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let sponsor = Address::generate(&env);
    token_admin.mint(&sponsor, &500_000_000_i128);

    // No event created — event_id 99 does not exist
    let result = client.try_sponsor_event(&sponsor, &99, &100_000_000_i128);
    assert_eq!(result, Err(Ok(Error::EventNotFound)));
}

#[test]
fn test_non_organizer_cannot_redeem_ticket() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let impostor = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    let result = client.try_redeem_ticket(&impostor, &event_id, &ticket_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_get_token_returns_configured_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (token_addr, _, _, client) = setup(&env);
    assert_eq!(client.get_token(), token_addr);
}

#[test]
fn test_get_admin_returns_configured_address() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = token_contract.address();

    let contract_id = env.register(NovaEventsContract, ());
    let client = NovaEventsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &token_addr);

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_get_balance_reflects_ticket_and_sponsor_payments() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let sponsor = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128); // 10 USDC
    token_admin.mint(&sponsor, &500_000_000_i128); // 50 USDC

    let event_id = create_test_event(&env, &client, &organizer);

    assert_eq!(client.get_balance(&event_id), 0);

    client.buy_ticket(&buyer, &event_id, &0); // 1 USDC
    assert_eq!(client.get_balance(&event_id), 10_000_000_i128);

    client.sponsor_event(&sponsor, &event_id, &200_000_000_i128); // 20 USDC
    assert_eq!(client.get_balance(&event_id), 210_000_000_i128);
}

#[test]
fn test_invalid_tier_index_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);

    // default_tiers has 2 tiers (index 0 and 1); index 99 is out of range
    let result = client.try_buy_ticket(&buyer, &event_id, &99);
    assert_eq!(result, Err(Ok(Error::InvalidTier)));
}

#[test]
fn test_create_event_with_no_tiers_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Empty"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &Vec::new(&env),
    );

    assert_eq!(result, Err(Ok(Error::NoTiers)));
}

#[test]
fn test_create_event_with_negative_funding_goal_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Bad Goal"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &-1_i128,
        &default_tiers(&env),
    );

    assert_eq!(result, Err(Ok(Error::InvalidFundingGoal)));
}

#[test]
fn test_zero_price_tier_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let bad_tiers = vec![
        &env,
        TierInput {
            name: String::from_str(&env, "Free"),
            price: 0,
            supply_cap: 50,
        },
    ];

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Bad Event"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &bad_tiers,
    );

    assert_eq!(result, Err(Ok(Error::InvalidTierPrice)));
}

#[test]
fn test_create_event_with_past_date_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 2_000_000_000);

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Time Traveler"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_000_000_000_u64, // before the ledger's current timestamp
        &100_000_000_i128,
        &default_tiers(&env),
    );

    assert_eq!(result, Err(Ok(Error::DateInPast)));
}

// ─── update_event_details tests ───────────────────────────────────────────────

#[test]
fn test_update_event_details_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    client.update_event_details(
        &organizer,
        &event_id,
        &String::from_str(&env, "Stellar Summit 2026"),
        &String::from_str(&env, "Updated description"),
        &String::from_str(&env, "New York"),
        &1_800_000_000_u64,
    );

    let event = client.get_event(&event_id);
    assert_eq!(event.name, String::from_str(&env, "Stellar Summit 2026"));
    assert_eq!(
        event.description,
        String::from_str(&env, "Updated description")
    );
    assert_eq!(event.venue, String::from_str(&env, "New York"));
    assert_eq!(event.date_unix, 1_800_000_000_u64);
}

#[test]
fn test_update_event_details_rejected_for_non_organizer() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let impostor = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    let result = client.try_update_event_details(
        &impostor,
        &event_id,
        &String::from_str(&env, "Hijacked"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_800_000_000_u64,
    );
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    // Event must be unchanged.
    let event = client.get_event(&event_id);
    assert_eq!(event.name, String::from_str(&env, "Stellar Summit"));
}

#[test]
fn test_update_event_details_reuses_create_event_validation() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    env.ledger().with_mut(|li| li.timestamp = 2_000_000_000);

    let empty_name = client.try_update_event_details(
        &organizer,
        &event_id,
        &String::from_str(&env, ""),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &2_500_000_000_u64,
    );
    assert_eq!(empty_name, Err(Ok(Error::EmptyName)));

    let empty_description = client.try_update_event_details(
        &organizer,
        &event_id,
        &String::from_str(&env, "name"),
        &String::from_str(&env, ""),
        &String::from_str(&env, "venue"),
        &2_500_000_000_u64,
    );
    assert_eq!(empty_description, Err(Ok(Error::EmptyDescription)));

    let empty_venue = client.try_update_event_details(
        &organizer,
        &event_id,
        &String::from_str(&env, "name"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, ""),
        &2_500_000_000_u64,
    );
    assert_eq!(empty_venue, Err(Ok(Error::EmptyVenue)));

    let past_date = client.try_update_event_details(
        &organizer,
        &event_id,
        &String::from_str(&env, "name"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_000_000_000_u64, // before the ledger's current timestamp
    );
    assert_eq!(past_date, Err(Ok(Error::DateInPast)));
}

#[test]
fn test_update_event_details_locked_once_tickets_sold() {
    // Once any tier has sold a ticket, details are locked so buyers can't be
    // surprised by a change to the thing they already paid for.
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.buy_ticket(&buyer, &event_id, &0);

    let result = client.try_update_event_details(
        &organizer,
        &event_id,
        &String::from_str(&env, "Renamed"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_800_000_000_u64,
    );
    assert_eq!(result, Err(Ok(Error::EventDetailsLocked)));

    // Event must be unchanged.
    let event = client.get_event(&event_id);
    assert_eq!(event.name, String::from_str(&env, "Stellar Summit"));
}

#[test]
fn test_create_event_with_empty_name_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, ""),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &default_tiers(&env),
    );

    assert_eq!(result, Err(Ok(Error::EmptyName)));
}

#[test]
fn test_create_event_with_empty_description_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "name"),
        &String::from_str(&env, ""),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &default_tiers(&env),
    );

    assert_eq!(result, Err(Ok(Error::EmptyDescription)));
}

#[test]
fn test_create_event_with_empty_venue_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "name"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, ""),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &default_tiers(&env),
    );

    assert_eq!(result, Err(Ok(Error::EmptyVenue)));
}

#[test]
fn test_sponsorships_at_cap_then_one_more_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor = Address::generate(&env);

    token_admin.mint(&sponsor, &(MAX_SPONSORSHIPS as i128 + 1));

    let event_id = create_test_event(&env, &client, &organizer);

    for _ in 0..MAX_SPONSORSHIPS {
        client.sponsor_event(&sponsor, &event_id, &1_i128);
    }
    assert_eq!(client.get_sponsorships(&event_id).len(), MAX_SPONSORSHIPS);

    // One more, past the cap, must be rejected with typed error.
    let result = client.try_sponsor_event(&sponsor, &event_id, &1_i128);
    assert_eq!(result, Err(Ok(Error::TooManySponsors)));
}

#[test]
fn test_create_event_with_too_many_tiers_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let mut too_many_tiers: Vec<TierInput> = Vec::new(&env);
    for _ in 0..(MAX_TIERS + 1) {
        too_many_tiers.push_back(TierInput {
            name: String::from_str(&env, "Tier"),
            price: 10_000_000_i128,
            supply_cap: 10,
        });
    }

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Overloaded"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &too_many_tiers,
    );

    assert_eq!(result, Err(Ok(Error::TooManyTiers)));
}

#[test]
fn test_zero_supply_cap_tier_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let bad_tiers = vec![
        &env,
        TierInput {
            name: String::from_str(&env, "Ghost"),
            price: 10_000_000_i128,
            supply_cap: 0,
        },
    ];

    let result = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Bad Event"),
        &String::from_str(&env, "desc"),
        &String::from_str(&env, "venue"),
        &1_750_000_000_u64,
        &100_000_000_i128,
        &bad_tiers,
    );

    assert_eq!(result, Err(Ok(Error::InvalidTierSupply)));
}

#[test]
fn test_sponsor_share_single_sponsor_is_100_percent() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor = Address::generate(&env);

    token_admin.mint(&sponsor, &500_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.sponsor_event(&sponsor, &event_id, &300_000_000_i128);

    // Single sponsor must own 100% = 10_000 basis points
    assert_eq!(client.get_sponsor_share(&event_id, &sponsor), 10_000);
}

#[test]
fn test_sponsor_share_multiple_sponsors_proportional() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);

    token_admin.mint(&sponsor_a, &1_000_000_000_i128);
    token_admin.mint(&sponsor_b, &1_000_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);

    // sponsor_a: 300, sponsor_b: 700 → total 1000
    client.sponsor_event(&sponsor_a, &event_id, &300_000_000_i128);
    client.sponsor_event(&sponsor_b, &event_id, &700_000_000_i128);

    // sponsor_a share = 300/1000 * 10_000 = 3_000 bp (30%)
    assert_eq!(client.get_sponsor_share(&event_id, &sponsor_a), 3_000);
    // sponsor_b share = 700/1000 * 10_000 = 7_000 bp (70%)
    assert_eq!(client.get_sponsor_share(&event_id, &sponsor_b), 7_000);
}

#[test]
fn test_sponsor_share_zero_sponsorship_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let anyone = Address::generate(&env);

    let event_id = create_test_event(&env, &client, &organizer);

    // No sponsorships at all — must return 0 without panicking
    assert_eq!(client.get_sponsor_share(&event_id, &anyone), 0);
}

#[test]
fn test_sponsor_share_non_sponsor_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let non_sponsor = Address::generate(&env);

    token_admin.mint(&sponsor, &500_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.sponsor_event(&sponsor, &event_id, &300_000_000_i128);

    // Total sponsorship is non-zero, but non_sponsor never contributed — must be 0, not garbage.
    assert_eq!(client.get_sponsor_share(&event_id, &non_sponsor), 0);
}

// ─── transfer_ticket tests ────────────────────────────────────────────────────

#[test]
fn test_transfer_ticket_basic() {
    // Happy path: owner transfers a valid, unredeemed ticket to another address.
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let new_owner = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    // Confirm buyer owns the ticket before transfer.
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, buyer);

    client.transfer_ticket(&buyer, &event_id, &ticket_id, &new_owner);

    // After transfer, new_owner should be the recorded owner.
    let ticket = client.get_ticket(&event_id, &ticket_id);
    assert_eq!(ticket.owner, new_owner);
    assert!(!ticket.redeemed);
}

#[test]
fn test_transfer_redeemed_ticket_fails() {
    // A ticket that has already been redeemed must not be transferable.
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let new_owner = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    // Organizer redeems the ticket at the door.
    client.redeem_ticket(&organizer, &event_id, &ticket_id);

    // Attempting to transfer a redeemed ticket must fail.
    let result = client.try_transfer_ticket(&buyer, &event_id, &ticket_id, &new_owner);
    assert_eq!(result, Err(Ok(Error::AlreadyRedeemed)));
}

#[test]
fn test_transfer_ticket_to_self_rejected() {
    // Transferring a ticket to its own current owner is a meaningless no-op
    // and must be rejected rather than silently succeeding.
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    let result = client.try_transfer_ticket(&buyer, &event_id, &ticket_id, &buyer);
    assert_eq!(result, Err(Ok(Error::InvalidRecipient)));

    // Ownership must be unchanged.
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, buyer);
}

#[test]
fn test_transfer_ticket_no_resale_rules() {
    // Without any resale-rule configuration the transfer is a pure ownership
    // reassignment: no USDC moves, only ticket.owner is updated.
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0); // costs 1 USDC

    let token = soroban_sdk::token::Client::new(&env, &token_addr);
    let buyer_balance_before = token.balance(&buyer);
    let recipient_balance_before = token.balance(&recipient);

    // Transfer with no resale rules — must succeed with no token movement.
    client.transfer_ticket(&buyer, &event_id, &ticket_id, &recipient);

    // Balances must be unchanged — this is purely an ownership record update.
    assert_eq!(token.balance(&buyer), buyer_balance_before);
    assert_eq!(token.balance(&recipient), recipient_balance_before);

    // Ownership must have been updated.
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, recipient);
}

// ─── resell_ticket tests ──────────────────────────────────────────────────────

#[test]
fn test_resell_ticket_no_rules_is_free() {
    // With no resale rules configured, resell_ticket behaves exactly like
    // transfer_ticket: a free ownership reassignment, regardless of `price`.
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    let token = soroban_sdk::token::Client::new(&env, &token_addr);
    let buyer_balance_before = token.balance(&buyer);
    let recipient_balance_before = token.balance(&recipient);

    client.resell_ticket(&buyer, &event_id, &ticket_id, &recipient, &20_000_000_i128);

    assert_eq!(token.balance(&buyer), buyer_balance_before);
    assert_eq!(token.balance(&recipient), recipient_balance_before);
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, recipient);
}

#[test]
fn test_resell_ticket_within_cap_splits_royalty() {
    // Resale within the configured cap must pay the seller (price - royalty)
    // and route the royalty share straight to the organizer.
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);
    token_admin.mint(&recipient, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0); // General tier: 1 USDC

    // Max resale price 2 USDC, 10% royalty.
    client.set_resale_rules(&organizer, &event_id, &20_000_000_i128, &1_000u32);

    let token = soroban_sdk::token::Client::new(&env, &token_addr);
    let seller_balance_before = token.balance(&buyer);
    let organizer_balance_before = token.balance(&organizer);
    let recipient_balance_before = token.balance(&recipient);

    let resale_price = 20_000_000_i128; // exactly at cap
    client.resell_ticket(&buyer, &event_id, &ticket_id, &recipient, &resale_price);

    let expected_royalty = 2_000_000_i128; // 10% of 20_000_000
    let expected_seller_amount = resale_price - expected_royalty;

    assert_eq!(
        token.balance(&buyer),
        seller_balance_before + expected_seller_amount
    );
    assert_eq!(
        token.balance(&organizer),
        organizer_balance_before + expected_royalty
    );
    assert_eq!(
        token.balance(&recipient),
        recipient_balance_before - resale_price
    );
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, recipient);
}

#[test]
fn test_resell_ticket_above_cap_rejected() {
    // A resale price above the configured max must be rejected outright.
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);
    token_admin.mint(&recipient, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    client.set_resale_rules(&organizer, &event_id, &20_000_000_i128, &1_000u32);

    let result =
        client.try_resell_ticket(&buyer, &event_id, &ticket_id, &recipient, &20_000_001_i128);
    assert_eq!(result, Err(Ok(Error::ResalePriceExceedsCap)));

    // Ownership must be unchanged.
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, buyer);
}

#[test]
fn test_resell_ticket_zero_or_negative_price_rejected_when_rules_set() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_admin.mint(&buyer, &50_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    client.set_resale_rules(&organizer, &event_id, &20_000_000_i128, &1_000u32);

    let result = client.try_resell_ticket(&buyer, &event_id, &ticket_id, &recipient, &0_i128);
    assert_eq!(result, Err(Ok(Error::ResalePriceExceedsCap)));
}

#[test]
fn test_set_resale_rules_rejected_for_non_organizer() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let stranger = Address::generate(&env);

    let event_id = create_test_event(&env, &client, &organizer);

    let result = client.try_set_resale_rules(&stranger, &event_id, &20_000_000_i128, &1_000u32);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_set_resale_rules_validates_inputs() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    let bad_price = client.try_set_resale_rules(&organizer, &event_id, &0_i128, &1_000u32);
    assert_eq!(bad_price, Err(Ok(Error::InvalidMaxResalePrice)));

    let bad_bps =
        client.try_set_resale_rules(&organizer, &event_id, &20_000_000_i128, &10_001u32);
    assert_eq!(bad_bps, Err(Ok(Error::InvalidRoyaltyBps)));
}

// ─── Payout tests ─────────────────────────────────────────────────────────────

/// Helper: create an event, sell a ticket to fund its balance, then end it.
/// Returns (event_id, organizer).
fn setup_ended_event(
    env: &Env,
    client: &NovaEventsContractClient,
    token_admin: &StellarAssetClient,
) -> (u32, Address) {
    let organizer = Address::generate(env);
    let buyer = Address::generate(env);

    token_admin.mint(&buyer, &100_000_000_i128); // 10 USDC

    let event_id = create_test_event(env, client, &organizer);
    client.buy_ticket(&buyer, &event_id, &0); // 1 USDC → balance = 10_000_000
    client.end_event(&organizer, &event_id);

    (event_id, organizer)
}

#[test]
fn test_payout_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, contract_id, client) = setup(&env);
    let _ = contract_id;
    let recipient = Address::generate(&env);

    let (event_id, organizer) = setup_ended_event(&env, &client, &token_admin);

    // Event balance is 10_000_000 (1 USDC from ticket sale).
    let payout_amount = 6_000_000_i128;
    client.payout(&organizer, &event_id, &recipient, &payout_amount);

    // Balance decremented.
    assert_eq!(client.get_balance(&event_id), 4_000_000_i128);

    // Disbursement recorded.
    let payouts = client.get_payouts(&event_id);
    assert_eq!(payouts.len(), 1);
    assert_eq!(payouts.get(0).unwrap().recipient, recipient);
    assert_eq!(payouts.get(0).unwrap().amount, payout_amount);

    // USDC transferred to recipient.
    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&recipient), payout_amount);
}

#[test]
fn test_payouts_at_cap_then_one_more_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let recipient = Address::generate(&env);

    let (event_id, organizer) = setup_ended_event(&env, &client, &token_admin);

    // Event balance is 10_000_000 — MAX_PAYOUTS payouts of 1 stroop each fits easily.
    for _ in 0..MAX_PAYOUTS {
        client.payout(&organizer, &event_id, &recipient, &1_i128);
    }
    assert_eq!(client.get_payouts(&event_id).len(), MAX_PAYOUTS);

    // One more, past the cap, must be rejected.
    let result = client.try_payout(&organizer, &event_id, &recipient, &1_i128);
    assert_eq!(result, Err(Ok(Error::TooManyPayouts)));
}

#[test]
fn test_payout_fails_when_amount_exceeds_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let recipient = Address::generate(&env);

    let (event_id, organizer) = setup_ended_event(&env, &client, &token_admin);

    // Event balance is 10_000_000; try to pay out more than that.
    let result = client.try_payout(&organizer, &event_id, &recipient, &99_000_000_i128);
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));
}

#[test]
fn test_payout_fails_on_active_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let event_id = create_test_event(&env, &client, &organizer);

    // Event is still Active — payout must be rejected.
    let result = client.try_payout(&organizer, &event_id, &recipient, &1_000_000_i128);
    assert_eq!(result, Err(Ok(Error::EventNotEnded)));
}

#[test]
fn test_payout_fails_on_cancelled_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let event_id = create_test_event(&env, &client, &organizer);

    // Flip status to Cancelled.
    env.as_contract(&client.address, || {
        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap();
        event.status = EventStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);
    });

    let result = client.try_payout(&organizer, &event_id, &recipient, &1_000_000_i128);
    assert_eq!(result, Err(Ok(Error::EventNotEnded)));
}

#[test]
fn test_payout_non_organizer_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let impostor = Address::generate(&env);
    let recipient = Address::generate(&env);

    let (event_id, _) = setup_ended_event(&env, &client, &token_admin);

    let result = client.try_payout(&impostor, &event_id, &recipient, &1_000_000_i128);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_multiple_payouts_accumulate_in_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let worker_a = Address::generate(&env);
    let worker_b = Address::generate(&env);

    let (event_id, organizer) = setup_ended_event(&env, &client, &token_admin);

    // Balance: 10_000_000; pay worker_a 3 USDC, worker_b 2 USDC.
    client.payout(&organizer, &event_id, &worker_a, &3_000_000_i128);
    client.payout(&organizer, &event_id, &worker_b, &2_000_000_i128);

    assert_eq!(client.get_balance(&event_id), 5_000_000_i128);

    let payouts = client.get_payouts(&event_id);
    assert_eq!(payouts.len(), 2);
    assert_eq!(payouts.get(0).unwrap().recipient, worker_a);
    assert_eq!(payouts.get(1).unwrap().recipient, worker_b);

    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&worker_a), 3_000_000_i128);
    assert_eq!(token.balance(&worker_b), 2_000_000_i128);
}

// ─── Event summary tests ─────────────────────────────────────────────────────

#[test]
fn test_get_event_summary_new_event_is_all_zeros() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    let summary = client.get_event_summary(&event_id);
    assert_eq!(summary.ticket_revenue, 0);
    assert_eq!(summary.sponsorship_total, 0);
    assert_eq!(summary.total_collected, 0);
    assert_eq!(summary.total_paid_out, 0);
    assert_eq!(summary.balance, 0);
}

/// The invariant that makes the summary an audit tool: across a realistic mix of
/// sales, sponsorships, and payouts, every unit collected is either still held
/// or recorded as disbursed.
#[test]
fn test_get_event_summary_invariant_across_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);

    let organizer = Address::generate(&env);
    let attendee_a = Address::generate(&env);
    let attendee_b = Address::generate(&env);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    let worker = Address::generate(&env);
    let vendor = Address::generate(&env);

    token_admin.mint(&attendee_a, &100_000_000_i128);
    token_admin.mint(&attendee_b, &100_000_000_i128);
    token_admin.mint(&sponsor_a, &200_000_000_i128);
    token_admin.mint(&sponsor_b, &100_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);

    // Two General tickets (1 USDC each) and one VIP (5 USDC) → 7 USDC.
    client.buy_ticket(&attendee_a, &event_id, &0);
    client.buy_ticket(&attendee_a, &event_id, &0);
    client.buy_ticket(&attendee_b, &event_id, &1);
    let expected_ticket_revenue = 70_000_000_i128;

    // Two sponsorships of different sizes → 13 USDC.
    client.sponsor_event(&sponsor_a, &event_id, &100_000_000_i128);
    client.sponsor_event(&sponsor_b, &event_id, &30_000_000_i128);
    let expected_sponsorship = 130_000_000_i128;

    // Everything collected is still held while the event is running.
    let mid = client.get_event_summary(&event_id);
    assert_eq!(mid.ticket_revenue, expected_ticket_revenue);
    assert_eq!(mid.sponsorship_total, expected_sponsorship);
    assert_eq!(mid.total_collected, 200_000_000_i128);
    assert_eq!(mid.total_paid_out, 0);
    assert_eq!(mid.balance, 200_000_000_i128);
    assert_eq!(mid.total_collected, mid.total_paid_out + mid.balance);

    // End the event so payouts are permitted.
    env.as_contract(&client.address, || {
        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap();
        event.status = EventStatus::Ended;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);
    });

    // Two disbursements to different recipients → 10 USDC out.
    client.payout(&organizer, &event_id, &worker, &45_000_000_i128);
    client.payout(&organizer, &event_id, &vendor, &55_000_000_i128);

    let final_summary = client.get_event_summary(&event_id);
    assert_eq!(final_summary.ticket_revenue, expected_ticket_revenue);
    assert_eq!(final_summary.sponsorship_total, expected_sponsorship);
    assert_eq!(final_summary.total_collected, 200_000_000_i128);
    assert_eq!(final_summary.total_paid_out, 100_000_000_i128);
    assert_eq!(final_summary.balance, 100_000_000_i128);

    // The audit guarantee.
    assert_eq!(
        final_summary.total_collected,
        final_summary.total_paid_out + final_summary.balance
    );
}

/// The aggregate must never drift from the itemized records it summarizes.
#[test]
fn test_get_event_summary_agrees_with_itemized_queries() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let attendee = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let worker = Address::generate(&env);

    token_admin.mint(&attendee, &100_000_000_i128);
    token_admin.mint(&sponsor, &100_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.buy_ticket(&attendee, &event_id, &1);
    client.sponsor_event(&sponsor, &event_id, &20_000_000_i128);

    env.as_contract(&client.address, || {
        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap();
        event.status = EventStatus::Ended;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);
    });
    client.payout(&organizer, &event_id, &worker, &15_000_000_i128);

    let summary = client.get_event_summary(&event_id);

    let sponsorships = client.get_sponsorships(&event_id);
    let mut sponsorship_sum = 0_i128;
    for i in 0..sponsorships.len() {
        sponsorship_sum += sponsorships.get(i).unwrap().amount;
    }
    assert_eq!(summary.sponsorship_total, sponsorship_sum);

    let payouts = client.get_payouts(&event_id);
    let mut payout_sum = 0_i128;
    for i in 0..payouts.len() {
        payout_sum += payouts.get(i).unwrap().amount;
    }
    assert_eq!(summary.total_paid_out, payout_sum);

    let tiers = client.get_tiers(&event_id);
    let mut tier_revenue = 0_i128;
    for i in 0..tiers.len() {
        let t = tiers.get(i).unwrap();
        tier_revenue += t.price * i128::from(t.tickets_sold);
    }
    assert_eq!(summary.ticket_revenue, tier_revenue);

    assert_eq!(summary.balance, client.get_balance(&event_id));
}

#[test]
fn test_get_event_summary_nonexistent_event_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);

    let result = client.try_get_event_summary(&999);
    assert_eq!(result, Err(Ok(Error::EventNotFound)));
}

// ─── buy_tickets tests ────────────────────────────────────────────────────────

#[test]
fn test_buy_tickets_happy_path_and_supply_check() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    token_admin.mint(&buyer, &1_000_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);

    // Tier 0 has 100 supply at 1 USDC (10_000_000). Buy 3 tickets.
    let ticket_ids = client.buy_tickets(&buyer, &event_id, &0, &3);
    assert_eq!(ticket_ids.len(), 3);
    assert_eq!(ticket_ids.get(0).unwrap(), 0);
    assert_eq!(ticket_ids.get(1).unwrap(), 1);
    assert_eq!(ticket_ids.get(2).unwrap(), 2);

    // All tickets owned by buyer
    assert_eq!(client.get_ticket(&event_id, &0).owner, buyer);
    assert_eq!(client.get_ticket(&event_id, &1).owner, buyer);
    assert_eq!(client.get_ticket(&event_id, &2).owner, buyer);

    // Single token transfer for 3 * 10 = 30 USDC (30_000_000)
    assert_eq!(client.get_balance(&event_id), 30_000_000_i128);
    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&buyer), 970_000_000_i128);

    // Tiers reflects 3 tickets sold
    let tiers = client.get_tiers(&event_id);
    assert_eq!(tiers.get(0).unwrap().tickets_sold, 3);

    // Buying 0 tickets fails
    let zero_res = client.try_buy_tickets(&buyer, &event_id, &0, &0);
    assert_eq!(zero_res, Err(Ok(Error::InvalidAmount)));

    // Exceeding supply (97 remaining, asking 98) fails atomically
    let overflow_res = client.try_buy_tickets(&buyer, &event_id, &0, &98);
    assert_eq!(overflow_res, Err(Ok(Error::TierSoldOut)));

    // Single buy_ticket still works seamlessly
    let single_id = client.buy_ticket(&buyer, &event_id, &0);
    assert_eq!(single_id, 3);
    assert_eq!(client.get_tiers(&event_id).get(0).unwrap().tickets_sold, 4);
}

// ─── Admin Rotation Tests ────────────────────────────────────────────────────

#[test]
fn test_set_admin_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let admin = client.get_admin();
    let new_admin = Address::generate(&env);

    client.set_admin(&admin, &new_admin);
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
fn test_set_admin_rejects_non_admin_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let admin = client.get_admin();
    let impostor = Address::generate(&env);
    let new_admin = Address::generate(&env);

    let res = client.try_set_admin(&impostor, &new_admin);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    assert_eq!(client.get_admin(), admin);
}

// ─── Emergency Pause Tests ───────────────────────────────────────────────────

#[test]
fn test_pause_unpause_access_control() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let admin = client.get_admin();
    let impostor = Address::generate(&env);

    assert!(!client.is_paused());

    // Non-admin cannot pause
    let res = client.try_pause(&impostor);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    assert!(!client.is_paused());

    // Admin can pause
    client.pause(&admin);
    assert!(client.is_paused());

    // Non-admin cannot unpause
    let res = client.try_unpause(&impostor);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    assert!(client.is_paused());

    // Admin can unpause
    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_paused_contract_rejects_state_changing_calls_and_unpause_restores() {
    let env = Env::default();
    env.mock_all_auths();

    let (_token_addr, token_admin, _, client) = setup(&env);
    let admin = client.get_admin();
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128);

    // Create an event while active
    let event_id = create_test_event(&env, &client, &organizer);
    let ticket_id = client.buy_ticket(&buyer, &event_id, &0);

    // Emergency pause the contract
    client.pause(&admin);
    assert!(client.is_paused());

    // 1. Creating events is rejected when paused
    let create_res = client.try_create_event(
        &organizer,
        &String::from_str(&env, "Paused Summit"),
        &String::from_str(&env, "Desc"),
        &String::from_str(&env, "Venue"),
        &1_750_000_000_u64,
        &500_000_000_i128,
        &default_tiers(&env),
    );
    assert_eq!(create_res, Err(Ok(Error::ContractPaused)));

    // 2. Buying tickets is rejected when paused
    let buy_res = client.try_buy_ticket(&buyer, &event_id, &0);
    assert_eq!(buy_res, Err(Ok(Error::ContractPaused)));

    // 3. Sponsoring events is rejected when paused
    let sponsor_res = client.try_sponsor_event(&buyer, &event_id, &10_000_000_i128);
    assert_eq!(sponsor_res, Err(Ok(Error::ContractPaused)));

    // 4. Redeeming tickets is rejected when paused
    let redeem_res = client.try_redeem_ticket(&organizer, &event_id, &ticket_id);
    assert_eq!(redeem_res, Err(Ok(Error::ContractPaused)));

    // 5. Transferring tickets is rejected when paused
    let transfer_res = client.try_transfer_ticket(&buyer, &event_id, &ticket_id, &recipient);
    assert_eq!(transfer_res, Err(Ok(Error::ContractPaused)));

    // 6. Read-only queries continue to work normally
    assert_eq!(client.event_count(), 1);
    let event = client.get_event(&event_id);
    assert_eq!(event.organizer, organizer);
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, buyer);

    // Unpause restores operations
    client.unpause(&admin);
    assert!(!client.is_paused());

    // Normal ticket transfer succeeds after unpause
    client.transfer_ticket(&buyer, &event_id, &ticket_id, &recipient);
    assert_eq!(client.get_ticket(&event_id, &ticket_id).owner, recipient);
}

// ─── get_events_by_organizer tests (issue #23) ───────────────────────────────

#[test]
fn test_get_events_by_organizer_empty_for_unknown_address() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let never_organized = Address::generate(&env);

    // Organizer creates an event, but a stranger who never organized gets an
    // empty Vec (not an error).
    create_test_event(&env, &client, &organizer);
    assert_eq!(client.get_events_by_organizer(&never_organized).len(), 0);
}

#[test]
fn test_get_events_by_organizer_tracks_multiple_events_per_organizer() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let id0 = create_test_event(&env, &client, &organizer);
    let id1 = create_test_event(&env, &client, &organizer);

    let events = client.get_events_by_organizer(&organizer);
    assert_eq!(events.len(), 2);
    assert_eq!(events.get(0).unwrap(), id0);
    assert_eq!(events.get(1).unwrap(), id1);
}

#[test]
fn test_get_events_by_organizer_isolation_between_organizers() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer_a = Address::generate(&env);
    let organizer_b = Address::generate(&env);

    let a_id = create_test_event(&env, &client, &organizer_a);
    create_test_event(&env, &client, &organizer_b);
    let b_second = create_test_event(&env, &client, &organizer_b);

    // A only sees its own event; B sees both of its own, and neither sees the other's.
    let a_events = client.get_events_by_organizer(&organizer_a);
    assert_eq!(a_events.len(), 1);
    assert_eq!(a_events.get(0).unwrap(), a_id);

    let b_events = client.get_events_by_organizer(&organizer_b);
    assert_eq!(b_events.len(), 2);
    assert_eq!(b_events.get(1).unwrap(), b_second);

    // Neither list contains the other's event IDs.
    assert!(!a_events.contains(b_second));
}

// ─── cancel_event tests (issue #24) ──────────────────────────────────────────

#[test]
fn test_cancel_event_with_no_tickets_or_sponsors_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);

    let event_id = create_test_event(&env, &client, &organizer);
    assert_eq!(client.get_event(&event_id).status, EventStatus::Active);

    client.cancel_event(&organizer, &event_id);

    assert_eq!(client.get_event(&event_id).status, EventStatus::Cancelled);
    assert_eq!(client.get_balance(&event_id), 0);
}

#[test]
fn test_cancel_event_refunds_sponsors() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);

    token_admin.mint(&sponsor_a, &500_000_000_i128);
    token_admin.mint(&sponsor_b, &500_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.sponsor_event(&sponsor_a, &event_id, &200_000_000_i128);
    client.sponsor_event(&sponsor_b, &event_id, &300_000_000_i128);
    assert_eq!(client.get_balance(&event_id), 500_000_000_i128);

    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&sponsor_a), 300_000_000_i128);
    assert_eq!(token.balance(&sponsor_b), 200_000_000_i128);

    client.cancel_event(&organizer, &event_id);

    // Each sponsor gets their contributed amount back.
    assert_eq!(token.balance(&sponsor_a), 500_000_000_i128);
    assert_eq!(token.balance(&sponsor_b), 500_000_000_i128);
    // Event balance zeroed after refunds.
    assert_eq!(client.get_balance(&event_id), 0);
    assert_eq!(client.get_event(&event_id).status, EventStatus::Cancelled);
}

#[test]
fn test_cancel_event_refunds_tickets_and_sponsors() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_addr, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let sponsor = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128);
    token_admin.mint(&sponsor, &500_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.buy_ticket(&buyer, &event_id, &0); // 1 USDC
    client.sponsor_event(&sponsor, &event_id, &100_000_000_i128);

    let token = TokenClient::new(&env, &token_addr);
    assert_eq!(token.balance(&buyer), 90_000_000_i128);
    assert_eq!(token.balance(&sponsor), 400_000_000_i128);

    client.cancel_event(&organizer, &event_id);

    assert_eq!(token.balance(&buyer), 100_000_000_i128);
    assert_eq!(token.balance(&sponsor), 500_000_000_i128);
    assert_eq!(client.get_balance(&event_id), 0);
    assert_eq!(client.get_event(&event_id).status, EventStatus::Cancelled);
}

#[test]
fn test_cancel_event_by_non_organizer_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let impostor = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    let result = client.try_cancel_event(&impostor, &event_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
    assert_eq!(client.get_event(&event_id).status, EventStatus::Active);
}

#[test]
fn test_cancel_event_rejects_non_active_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    client.end_event(&organizer, &event_id);
    let result = client.try_cancel_event(&organizer, &event_id);
    assert_eq!(result, Err(Ok(Error::EventNotActive)));

    // Double cancellation also rejected.
    let id2 = create_test_event(&env, &client, &organizer);
    client.cancel_event(&organizer, &id2);
    let second = client.try_cancel_event(&organizer, &id2);
    assert_eq!(second, Err(Ok(Error::EventNotActive)));
}

#[test]
fn test_cancel_event_blocks_buying_and_sponsoring() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let sponsor = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128);
    token_admin.mint(&sponsor, &100_000_000_i128);

    let event_id = create_test_event(&env, &client, &organizer);
    client.cancel_event(&organizer, &event_id);

    assert_eq!(
        client.try_buy_ticket(&buyer, &event_id, &0),
        Err(Ok(Error::EventNotActive))
    );
    assert_eq!(
        client.try_sponsor_event(&sponsor, &event_id, &10_000_000_i128),
        Err(Ok(Error::EventNotActive))
    );
}

#[test]
fn test_cancel_event_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let admin = client.get_admin();
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    client.pause(&admin);

    let result = client.try_cancel_event(&organizer, &event_id);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    assert_eq!(client.get_event(&event_id).status, EventStatus::Active);
}

#[test]
fn test_end_event_blocked_while_paused() {
    // end_event predates the pause feature and was never wired to check it —
    // confirm it now respects the same emergency halt as every other
    // state-changing function.
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, client) = setup(&env);
    let admin = client.get_admin();
    let organizer = Address::generate(&env);
    let event_id = create_test_event(&env, &client, &organizer);

    client.pause(&admin);

    let result = client.try_end_event(&organizer, &event_id);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    assert_eq!(client.get_event(&event_id).status, EventStatus::Active);

    client.unpause(&admin);
    client.end_event(&organizer, &event_id);
    assert_eq!(client.get_event(&event_id).status, EventStatus::Ended);
}
