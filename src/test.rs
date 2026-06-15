use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    vec, Address, Env, String,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (Address, StellarAssetClient, Address, NovaEventsContractClient) {
    let token_admin = Address::generate(env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = token_contract.address();
    let token_admin_client = StellarAssetClient::new(env, &token_addr);

    let contract_id = env.register(NovaEventsContract, ());
    let client = NovaEventsContractClient::new(env, &contract_id);
    client.initialize(&token_addr);

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

fn create_test_event(
    env: &Env,
    client: &NovaEventsContractClient,
    organizer: &Address,
) -> u32 {
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
    assert!(result.is_err());
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
    assert!(result.is_err());
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
    assert!(result.is_err());
}

#[test]
fn test_get_token_returns_configured_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (token_addr, _, _, client) = setup(&env);
    assert_eq!(client.get_token(), token_addr);
}

#[test]
fn test_get_balance_reflects_ticket_and_sponsor_payments() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, token_admin, _, client) = setup(&env);
    let organizer = Address::generate(&env);
    let buyer = Address::generate(&env);
    let sponsor = Address::generate(&env);

    token_admin.mint(&buyer, &100_000_000_i128);   // 10 USDC
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
    assert!(result.is_err());
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

    assert!(result.is_err());
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

    assert!(result.is_err());
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

    assert!(result.is_err());
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

    assert!(result.is_err());
}
