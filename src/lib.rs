#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    token::Client as TokenClient,
    Address, Env, String, Vec,
};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Input shape for a ticket tier when creating an event.
#[contracttype]
#[derive(Clone)]
pub struct TierInput {
    pub name: String,
    /// Price in USDC stroops (1 USDC = 10_000_000).
    pub price: i128,
    pub supply_cap: u32,
}

/// Stored state for a ticket tier, extended with live sales count.
#[contracttype]
#[derive(Clone)]
pub struct TicketTier {
    pub name: String,
    pub price: i128,
    pub supply_cap: u32,
    pub tickets_sold: u32,
}

#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum EventStatus {
    Active,
    Ended,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct Event {
    pub organizer: Address,
    pub name: String,
    pub description: String,
    pub venue: String,
    /// Unix timestamp of the event date.
    pub date_unix: u64,
    /// Funding goal in USDC stroops.
    pub funding_goal: i128,
    /// Current USDC balance held by the contract for this event.
    pub balance: i128,
    pub status: EventStatus,
}

/// On-chain proof of ticket ownership.
#[contracttype]
#[derive(Clone)]
pub struct Ticket {
    pub event_id: u32,
    pub tier_index: u32,
    pub owner: Address,
    pub redeemed: bool,
}

/// A public sponsorship contribution record.
#[contracttype]
#[derive(Clone)]
pub struct Sponsorship {
    pub sponsor: Address,
    pub amount: i128,
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Token,
    EventCounter,
    Event(u32),
    Tiers(u32),
    TicketCounter(u32),
    Ticket(u32, u32),
    Sponsorships(u32),
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct NovaEventsContract;

#[contractimpl]
impl NovaEventsContract {
    /// One-time setup: record the USDC token contract address.
    pub fn initialize(env: Env, token: Address) {
        if env.storage().instance().has(&DataKey::Token) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::EventCounter, &0u32);
    }

    /// Organizer creates a new event with one or more ticket tiers.
    /// Returns the new event ID.
    pub fn create_event(
        env: Env,
        organizer: Address,
        name: String,
        description: String,
        venue: String,
        date_unix: u64,
        funding_goal: i128,
        tiers: Vec<TierInput>,
    ) -> u32 {
        organizer.require_auth();

        if tiers.is_empty() {
            panic!("at least one tier required");
        }
        if funding_goal <= 0 {
            panic!("funding goal must be positive");
        }
        for i in 0..tiers.len() {
            let t: TierInput = tiers.get(i).unwrap();
            if t.price <= 0 {
                panic!("tier price must be positive");
            }
            if t.supply_cap == 0 {
                panic!("tier supply cap must be at least 1");
            }
        }

        let event_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::EventCounter)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::EventCounter, &(event_id + 1));

        env.storage().persistent().set(
            &DataKey::Event(event_id),
            &Event {
                organizer,
                name,
                description,
                venue,
                date_unix,
                funding_goal,
                balance: 0,
                status: EventStatus::Active,
            },
        );

        let mut tier_list: Vec<TicketTier> = Vec::new(&env);
        for i in 0..tiers.len() {
            let t: TierInput = tiers.get(i).unwrap();
            tier_list.push_back(TicketTier {
                name: t.name,
                price: t.price,
                supply_cap: t.supply_cap,
                tickets_sold: 0,
            });
        }
        env.storage()
            .persistent()
            .set(&DataKey::Tiers(event_id), &tier_list);
        env.storage()
            .persistent()
            .set(&DataKey::TicketCounter(event_id), &0u32);

        let empty_s: Vec<Sponsorship> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Sponsorships(event_id), &empty_s);

        event_id
    }

    /// Buyer purchases a ticket in a given tier.
    /// Transfers `tier.price` USDC from buyer to this contract.
    /// Returns the new ticket ID.
    pub fn buy_ticket(env: Env, buyer: Address, event_id: u32, tier_index: u32) -> u32 {
        buyer.require_auth();

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .expect("event not found");
        if event.status != EventStatus::Active {
            panic!("event not active");
        }

        let tiers: Vec<TicketTier> = env
            .storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .expect("tiers not found");
        if tier_index >= tiers.len() {
            panic!("invalid tier");
        }

        let tier: TicketTier = tiers.get(tier_index).unwrap();
        if tier.tickets_sold >= tier.supply_cap {
            panic!("tier sold out");
        }

        let price = tier.price;
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        TokenClient::new(&env, &token_addr).transfer(
            &buyer,
            &env.current_contract_address(),
            &price,
        );

        // Rebuild tiers with updated sold count for the purchased tier.
        let mut updated: Vec<TicketTier> = Vec::new(&env);
        for i in 0..tiers.len() {
            let t: TicketTier = tiers.get(i).unwrap();
            if i == tier_index {
                updated.push_back(TicketTier {
                    name: t.name,
                    price: t.price,
                    supply_cap: t.supply_cap,
                    tickets_sold: t.tickets_sold + 1,
                });
            } else {
                updated.push_back(t);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::Tiers(event_id), &updated);

        event.balance += price;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);

        let ticket_id: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TicketCounter(event_id))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::TicketCounter(event_id), &(ticket_id + 1));

        env.storage().persistent().set(
            &DataKey::Ticket(event_id, ticket_id),
            &Ticket {
                event_id,
                tier_index,
                owner: buyer,
                redeemed: false,
            },
        );

        ticket_id
    }

    /// Organizer checks in (redeems) a ticket at the door.
    pub fn redeem_ticket(env: Env, organizer: Address, event_id: u32, ticket_id: u32) {
        organizer.require_auth();

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .expect("event not found");
        if event.organizer != organizer {
            panic!("not the organizer");
        }

        let mut ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(event_id, ticket_id))
            .expect("ticket not found");
        if ticket.redeemed {
            panic!("already redeemed");
        }

        ticket.redeemed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Ticket(event_id, ticket_id), &ticket);
    }

    /// Sponsor contributes USDC to an event.
    /// Contribution is recorded publicly against the sponsor's address.
    pub fn sponsor_event(env: Env, sponsor: Address, event_id: u32, amount: i128) {
        sponsor.require_auth();

        if amount <= 0 {
            panic!("amount must be positive");
        }

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .expect("event not found");
        if event.status != EventStatus::Active {
            panic!("event not active");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        TokenClient::new(&env, &token_addr).transfer(
            &sponsor,
            &env.current_contract_address(),
            &amount,
        );

        let mut sponsorships: Vec<Sponsorship> = env
            .storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        sponsorships.push_back(Sponsorship { sponsor, amount });
        env.storage()
            .persistent()
            .set(&DataKey::Sponsorships(event_id), &sponsorships);

        event.balance += amount;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);
    }

    // ─── Queries — readable by anyone ────────────────────────────────────────

    pub fn get_event(env: Env, event_id: u32) -> Event {
        env.storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .expect("event not found")
    }

    pub fn get_tiers(env: Env, event_id: u32) -> Vec<TicketTier> {
        env.storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .expect("tiers not found")
    }

    pub fn get_ticket(env: Env, event_id: u32, ticket_id: u32) -> Ticket {
        env.storage()
            .persistent()
            .get(&DataKey::Ticket(event_id, ticket_id))
            .expect("ticket not found")
    }

    pub fn get_sponsorships(env: Env, event_id: u32) -> Vec<Sponsorship> {
        env.storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn event_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::EventCounter)
            .unwrap_or(0)
    }

    pub fn ticket_count(env: Env, event_id: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::TicketCounter(event_id))
            .unwrap_or(0)
    }

    /// Returns the current USDC balance held by the contract for an event.
    /// Convenience wrapper around get_event so callers don't decode the full struct.
    pub fn get_balance(env: Env, event_id: u32) -> i128 {
        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .expect("event not found");
        event.balance
    }
}

#[cfg(test)]
mod test;
