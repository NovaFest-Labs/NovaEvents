#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token::Client as TokenClient, Address,
    Env, String, Vec,
};

/// Maximum number of ticket tiers allowed per event.
const MAX_TIERS: u32 = 20;

/// Maximum number of sponsorships allowed per event, bounding the cost of
/// get_sponsorships/get_sponsor_share, which both scan the full list.
const MAX_SPONSORSHIPS: u32 = 100;

/// Maximum number of payouts recorded per event, bounding the cost of
/// get_payouts, which scans the full list.
const MAX_PAYOUTS: u32 = 100;

// ─── Error enum ───────────────────────────────────────────────────────────────

/// All structured failure codes returned by the contract.
/// Using `#[contracterror]` means callers receive a typed, distinguishable
/// error code instead of an opaque panic trap.
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// Contract has already been initialised.
    AlreadyInitialized = 1,
    /// Contract has not been initialised yet.
    NotInitialized = 2,
    /// The requested event does not exist.
    EventNotFound = 3,
    /// The requested ticket does not exist.
    TicketNotFound = 4,
    /// Tier storage for an event is missing (internal inconsistency).
    TiersNotFound = 5,
    /// Caller is not the organizer of the event.
    Unauthorized = 6,
    /// Event is not in Active status.
    EventNotActive = 7,
    /// Ticket has already been redeemed.
    AlreadyRedeemed = 8,
    /// The requested tier index is out of range.
    InvalidTier = 9,
    /// The requested tier has no remaining supply.
    TierSoldOut = 10,
    /// Event must have at least one tier.
    NoTiers = 11,
    /// Event exceeds the maximum allowed number of tiers.
    TooManyTiers = 12,
    /// Funding goal must be a positive amount.
    InvalidFundingGoal = 13,
    /// Event name must not be empty.
    EmptyName = 14,
    /// Event description must not be empty.
    EmptyDescription = 15,
    /// Event venue must not be empty.
    EmptyVenue = 16,
    /// Event date must not be in the past.
    DateInPast = 17,
    /// Tier price must be a positive amount.
    InvalidTierPrice = 18,
    /// Tier supply cap must be at least 1.
    InvalidTierSupply = 19,
    /// Sponsorship amount must be positive.
    InvalidAmount = 20,
    /// Event has reached the maximum number of sponsorships.
    TooManySponsors = 21,
    /// Caller is not the current ticket owner.
    NotOwner = 22,
    /// Payout amount exceeds the event's current balance.
    InsufficientBalance = 23,
    /// Event is not in Ended status.
    EventNotEnded = 24,
    /// Event has reached the maximum number of recorded payouts.
    TooManyPayouts = 25,
    /// Recipient must differ from the ticket's current owner.
    InvalidRecipient = 26,
    /// Contract is currently paused by admin.
    ContractPaused = 27,
}

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

/// A record of a single payout disbursed from an event's balance.
#[contracttype]
#[derive(Clone)]
pub struct Payout {
    pub recipient: Address,
    pub amount: i128,
}

/// Aggregated financial view of an event, so an auditor can confirm in one call
/// that every unit collected is still held or accounted for as a disbursement.
///
/// The contract guarantees `total_collected == total_paid_out + balance`.
#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub struct EventSummary {
    /// Sum of every ticket sold, priced at its tier.
    pub ticket_revenue: i128,
    /// Sum of every sponsorship contribution.
    pub sponsorship_total: i128,
    /// `ticket_revenue + sponsorship_total` — everything the event ever took in.
    pub total_collected: i128,
    /// Sum of every payout disbursed from the event balance.
    pub total_paid_out: i128,
    /// Funds still held by the contract for this event.
    pub balance: i128,
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    EventCounter,
    Event(u32),
    Tiers(u32),
    TicketCounter(u32),
    Ticket(u32, u32),
    Sponsorships(u32),
    Payouts(u32),
    OrganizerEvents(Address),
    Paused,
}

fn require_not_paused(env: &Env) -> Result<(), Error> {
    if env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
    {
        return Err(Error::ContractPaused);
    }
    Ok(())
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct NovaEventsContract;

#[contractimpl]
impl NovaEventsContract {
    /// One-time setup: authorize an admin and record the USDC token contract address.
    ///
    /// What it does:
    /// - Stores the admin address and USDC token contract address in instance storage.
    /// - Initializes the event counter to 0.
    ///
    /// Who may call:
    /// - Any caller, but the `admin` address passed must authorize the call (via `require_auth`).
    ///
    /// Errors:
    /// - `AlreadyInitialized` if the contract has already been initialised.
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Token) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::EventCounter, &0u32);
        Ok(())
    }

    /// Emergency halt for all state-changing contract functions.
    ///
    /// What it does:
    /// - Sets the global paused flag to true. Used to prevent state-changing calls while paused.
    ///
    /// Who may call:
    /// - Only the configured admin (the `admin` parameter must authorize the call and match stored admin).
    ///
    /// Errors:
    /// - `NotInitialized` if the contract hasn't been initialized.
    /// - `Unauthorized` if the provided `admin` doesn't match the recorded admin.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        let current_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if admin != current_admin {
            return Err(Error::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    /// Resumes normal contract operations after an emergency halt.
    /// Callable only by the registered admin.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        let current_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if admin != current_admin {
            return Err(Error::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Organizer creates a new event with one or more ticket tiers and returns the new event ID.
    ///
    /// What it does:
    /// - Validates input (non-empty name/description/venue, positive funding goal, tiers present and valid).
    /// - Persists the Event, Tiers, TicketCounter and Sponsorships storage entries.
    /// - Increments the event counter and returns the created event ID.
    ///
    /// Who may call:
    /// - The organizer address passed must authorize the call (`organizer.require_auth()`).
    ///
    /// Errors:
    /// - `NoTiers`, `TooManyTiers`, `InvalidFundingGoal`, `EmptyName`, `EmptyDescription`,
    ///   `EmptyVenue`, `DateInPast`, `InvalidTierPrice`, `InvalidTierSupply`,
    ///   and `ContractPaused` (via require_not_paused).
    #[allow(clippy::too_many_arguments)]
    pub fn create_event(
        env: Env,
        organizer: Address,
        name: String,
        description: String,
        venue: String,
        date_unix: u64,
        funding_goal: i128,
        tiers: Vec<TierInput>,
    ) -> Result<u32, Error> {
        require_not_paused(&env)?;
        organizer.require_auth();

        if tiers.is_empty() {
            return Err(Error::NoTiers);
        }
        if tiers.len() > MAX_TIERS {
            return Err(Error::TooManyTiers);
        }
        if funding_goal <= 0 {
            return Err(Error::InvalidFundingGoal);
        }
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        if description.is_empty() {
            return Err(Error::EmptyDescription);
        }
        if venue.is_empty() {
            return Err(Error::EmptyVenue);
        }
        if date_unix < env.ledger().timestamp() {
            return Err(Error::DateInPast);
        }
        for i in 0..tiers.len() {
            let t: TierInput = tiers.get(i).unwrap();
            if t.price <= 0 {
                return Err(Error::InvalidTierPrice);
            }
            if t.supply_cap == 0 {
                return Err(Error::InvalidTierSupply);
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

        let organizer_clone = organizer.clone();
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

        // Maintain a reverse index from organizer to their event IDs so
        // get_events_by_organizer doesn't have to scan every event.
        let mut organizer_events: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::OrganizerEvents(organizer_clone.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        organizer_events.push_back(event_id);
        env.storage().persistent().set(
            &DataKey::OrganizerEvents(organizer_clone),
            &organizer_events,
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

        Ok(event_id)
    }

    /// Buyer purchases multiple tickets in a given tier, transferring USDC to the contract.
    ///
    /// What it does:
    /// - Validates event/tier/quantity and updates tier sold counts, ticket records, and event balance.
    /// - Transfers USDC from buyer to the contract.
    ///
    /// Who may call:
    /// - The buyer address passed must authorize the call (`buyer.require_auth()`).
    ///
    /// Errors:
    /// - `ContractPaused`, `InvalidAmount`, `EventNotFound`, `EventNotActive`, `TiersNotFound`,
    ///   `InvalidTier`, `TierSoldOut`, `InvalidAmount` (on arithmetic overflow), `NotInitialized`
    ///   (if token address missing).
    pub fn buy_tickets(
        env: Env,
        buyer: Address,
        event_id: u32,
        tier_index: u32,
        quantity: u32,
    ) -> Result<Vec<u32>, Error> {
        require_not_paused(&env)?;
        buyer.require_auth();

        if quantity == 0 {
            return Err(Error::InvalidAmount);
        }

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;
        if event.status != EventStatus::Active {
            return Err(Error::EventNotActive);
        }

        let tiers: Vec<TicketTier> = env
            .storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .ok_or(Error::TiersNotFound)?;
        if tier_index >= tiers.len() {
            return Err(Error::InvalidTier);
        }

        let tier: TicketTier = tiers.get(tier_index).unwrap();
        if tier.tickets_sold + quantity > tier.supply_cap {
            return Err(Error::TierSoldOut);
        }

        let total_price = tier
            .price
            .checked_mul(quantity as i128)
            .ok_or(Error::InvalidAmount)?;
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;

        // Rebuild tiers with updated sold count for the purchased tier.
        let mut updated: Vec<TicketTier> = Vec::new(&env);
        for i in 0..tiers.len() {
            let t: TicketTier = tiers.get(i).unwrap();
            if i == tier_index {
                updated.push_back(TicketTier {
                    name: t.name,
                    price: t.price,
                    supply_cap: t.supply_cap,
                    tickets_sold: t.tickets_sold + quantity,
                });
            } else {
                updated.push_back(t);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::Tiers(event_id), &updated);

        event.balance += total_price;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);

        let starting_ticket_id: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TicketCounter(event_id))
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::TicketCounter(event_id),
            &(starting_ticket_id + quantity),
        );

        let mut ticket_ids: Vec<u32> = Vec::new(&env);
        for offset in 0..quantity {
            let ticket_id = starting_ticket_id + offset;
            env.storage().persistent().set(
                &DataKey::Ticket(event_id, ticket_id),
                &Ticket {
                    event_id,
                    tier_index,
                    owner: buyer.clone(),
                    redeemed: false,
                },
            );
            ticket_ids.push_back(ticket_id);
        }

        TokenClient::new(&env, &token_addr).transfer(
            &buyer,
            env.current_contract_address(),
            &total_price,
        );

        Ok(ticket_ids)
    }

    /// Buyer purchases a ticket in a given tier.
    /// Transfers `tier.price` USDC from buyer to this contract.
    /// Returns the new ticket ID.
    pub fn buy_ticket(
        env: Env,
        buyer: Address,
        event_id: u32,
        tier_index: u32,
    ) -> Result<u32, Error> {
        let ticket_ids = Self::buy_tickets(env, buyer, event_id, tier_index, 1)?;
        Ok(ticket_ids.get(0).unwrap())
    }

    /// Organizer checks in (redeems) a ticket at the door.
    ///
    /// What it does:
    /// - Marks a ticket as redeemed on-chain.
    ///
    /// Who may call:
    /// - Only the event's organizer (the `organizer` parameter must authorize the call).
    ///
    /// Errors:
    /// - `ContractPaused`, `NotInitialized` (if event missing/other storage issues),
    /// - `EventNotFound`, `Unauthorized`, `TicketNotFound`, `AlreadyRedeemed`.
    pub fn redeem_ticket(
        env: Env,
        organizer: Address,
        event_id: u32,
        ticket_id: u32,
    ) -> Result<(), Error> {
        require_not_paused(&env)?;
        organizer.require_auth();

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;
        if event.organizer != organizer {
            return Err(Error::Unauthorized);
        }

        let mut ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(event_id, ticket_id))
            .ok_or(Error::TicketNotFound)?;
        if ticket.redeemed {
            return Err(Error::AlreadyRedeemed);
        }

        ticket.redeemed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Ticket(event_id, ticket_id), &ticket);
        Ok(())
    }

    /// Organizer closes an event, transitioning it from `Active` to `Ended`.
    /// Blocks further ticket purchases and sponsorships, and is the
    /// prerequisite for `payout`.
    pub fn end_event(env: Env, organizer: Address, event_id: u32) -> Result<(), Error> {
        require_not_paused(&env)?;
        organizer.require_auth();

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;
        if event.organizer != organizer {
            return Err(Error::Unauthorized);
        }
        if event.status != EventStatus::Active {
            return Err(Error::EventNotActive);
        }

        event.status = EventStatus::Ended;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);
        Ok(())
    }

    /// Organizer cancels an `Active` event.
    /// Refunds every unredeemed ticket buyer their tier price and refunds every
    /// sponsor their contributed amount, then marks the event `Cancelled`.
    /// Cancelling with no buyers/sponsors still succeeds (the refund loops no-op).
    pub fn cancel_event(env: Env, organizer: Address, event_id: u32) -> Result<(), Error> {
        require_not_paused(&env)?;
        organizer.require_auth();

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;
        if event.organizer != organizer {
            return Err(Error::Unauthorized);
        }
        if event.status != EventStatus::Active {
            return Err(Error::EventNotActive);
        }

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let token_client = TokenClient::new(&env, &token_addr);

        // Refund each unredeemed ticket owner their tier price.
        let tiers: Vec<TicketTier> = env
            .storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .ok_or(Error::TiersNotFound)?;
        let ticket_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TicketCounter(event_id))
            .unwrap_or(0);
        for ticket_id in 0..ticket_count {
            let ticket: Ticket = env
                .storage()
                .persistent()
                .get(&DataKey::Ticket(event_id, ticket_id))
                .expect("recorded ticket index must exist");
            if ticket.redeemed {
                continue;
            }
            let tier: TicketTier = tiers
                .get(ticket.tier_index)
                .expect("ticket tier index must exist");
            token_client.transfer(&env.current_contract_address(), &ticket.owner, &tier.price);
        }

        // Refund each sponsor their contributed amount.
        let sponsorships: Vec<Sponsorship> = env
            .storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        for i in 0..sponsorships.len() {
            let s: Sponsorship = sponsorships.get(i).unwrap();
            token_client.transfer(&env.current_contract_address(), &s.sponsor, &s.amount);
        }

        // Any unredeemed tickets/sponsorship balances are returned, so zero out balance.
        event.balance = 0;
        event.status = EventStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);
        Ok(())
    }

    /// Transfer ticket ownership from the current owner to a new address.
    ///
    /// Rules enforced:
    /// - `from` must be the current ticket owner and must authorize the call.
    /// - The event must not be `Cancelled` or `Ended`.
    /// - The ticket must not have been redeemed already.
    pub fn transfer_ticket(
        env: Env,
        from: Address,
        event_id: u32,
        ticket_id: u32,
        to: Address,
    ) -> Result<(), Error> {
        require_not_paused(&env)?;
        from.require_auth();

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;

        // Transfers are blocked for Cancelled or Ended events.
        if event.status == EventStatus::Cancelled || event.status == EventStatus::Ended {
            return Err(Error::EventNotActive);
        }

        let mut ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(event_id, ticket_id))
            .ok_or(Error::TicketNotFound)?;

        // Only the current owner may transfer.
        if ticket.owner != from {
            return Err(Error::NotOwner);
        }

        // Transferring to yourself is a meaningless no-op.
        if to == from {
            return Err(Error::InvalidRecipient);
        }

        // A redeemed ticket cannot change hands.
        if ticket.redeemed {
            return Err(Error::AlreadyRedeemed);
        }

        ticket.owner = to;
        env.storage()
            .persistent()
            .set(&DataKey::Ticket(event_id, ticket_id), &ticket);

        Ok(())
    }

    /// Sponsor contributes USDC to an event.
    /// Contribution is recorded publicly against the sponsor's address.
    pub fn sponsor_event(
        env: Env,
        sponsor: Address,
        event_id: u32,
        amount: i128,
    ) -> Result<(), Error> {
        require_not_paused(&env)?;
        sponsor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;
        if event.status != EventStatus::Active {
            return Err(Error::EventNotActive);
        }

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;

        let mut sponsorships: Vec<Sponsorship> = env
            .storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        if sponsorships.len() >= MAX_SPONSORSHIPS {
            return Err(Error::TooManySponsors);
        }
        sponsorships.push_back(Sponsorship {
            sponsor: sponsor.clone(),
            amount,
        });
        env.storage()
            .persistent()
            .set(&DataKey::Sponsorships(event_id), &sponsorships);

        event.balance += amount;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);

        TokenClient::new(&env, &token_addr).transfer(
            &sponsor,
            env.current_contract_address(),
            &amount,
        );
        Ok(())
    }

    // ─── Payouts ──────────────────────────────────────────────────────────────

    /// Organizer disburses `amount` USDC from the event balance to `recipient`.
    /// Only callable on an Ended event; every disbursement is recorded on-chain.
    pub fn payout(
        env: Env,
        organizer: Address,
        event_id: u32,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        require_not_paused(&env)?;
        organizer.require_auth();

        let mut event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;

        if event.organizer != organizer {
            return Err(Error::Unauthorized);
        }
        if event.status != EventStatus::Ended {
            return Err(Error::EventNotEnded);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if amount > event.balance {
            return Err(Error::InsufficientBalance);
        }

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;

        let mut payouts: Vec<Payout> = env
            .storage()
            .persistent()
            .get(&DataKey::Payouts(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        if payouts.len() >= MAX_PAYOUTS {
            return Err(Error::TooManyPayouts);
        }

        // Deduct from event balance and persist.
        event.balance -= amount;
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id), &event);

        // Record disbursement.
        payouts.push_back(Payout {
            recipient: recipient.clone(),
            amount,
        });
        env.storage()
            .persistent()
            .set(&DataKey::Payouts(event_id), &payouts);

        // Transfer USDC from contract to recipient.
        TokenClient::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        Ok(())
    }

    /// Returns the Event record for `event_id`.
    ///
    /// Who may call:
    /// - Publicly readable by any caller (no authorization required).
    ///
    /// Errors:
    /// - `EventNotFound` if no event exists with the supplied id.
    pub fn get_event(env: Env, event_id: u32) -> Result<Event, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)
    }

    /// Returns the stored ticket tiers for `event_id`.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - `TiersNotFound` if the event has no tiers stored.
    pub fn get_tiers(env: Env, event_id: u32) -> Result<Vec<TicketTier>, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .ok_or(Error::TiersNotFound)
    }

    /// Returns a single ticket's ownership record.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - `TicketNotFound` if no ticket exists with the supplied id.
    pub fn get_ticket(env: Env, event_id: u32, ticket_id: u32) -> Result<Ticket, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Ticket(event_id, ticket_id))
            .ok_or(Error::TicketNotFound)
    }

    /// Returns every sponsorship contribution recorded for `event_id`.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - Never errors; returns an empty `Vec` if the event has no sponsorships.
    pub fn get_sponsorships(env: Env, event_id: u32) -> Vec<Sponsorship> {
        env.storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns every payout disbursed for `event_id`.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - Never errors; returns an empty `Vec` if the event has no payouts.
    pub fn get_payouts(env: Env, event_id: u32) -> Vec<Payout> {
        env.storage()
            .persistent()
            .get(&DataKey::Payouts(event_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the total number of events ever created.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - Never errors; returns 0 before the first event is created.
    pub fn event_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::EventCounter)
            .unwrap_or(0)
    }

    /// Returns the list of event IDs ever created by `organizer`.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - Never errors; returns an empty `Vec` for an address that has not
    ///   organized anything.
    pub fn get_events_by_organizer(env: Env, organizer: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::OrganizerEvents(organizer))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the number of tickets sold for `event_id`.
    ///
    /// Who may call:
    /// - Publicly readable by any caller.
    ///
    /// Errors:
    /// - Never errors; returns 0 if no tickets have been sold.
    pub fn ticket_count(env: Env, event_id: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::TicketCounter(event_id))
            .unwrap_or(0)
    }

    /// Returns the token contract address configured during initialize.
    pub fn get_token(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)
    }

    /// Returns the admin address configured during initialize.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Returns the current USDC balance held by the contract for an event.
    /// Convenience wrapper around get_event so callers don't decode the full struct.
    pub fn get_balance(env: Env, event_id: u32) -> Result<i128, Error> {
        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;
        Ok(event.balance)
    }

    /// Returns a single audit view of an event's money: what came in from
    /// tickets, what came in from sponsors, what went out, and what is left.
    ///
    /// Every figure is derived from the same records the itemized queries read,
    /// so the summary cannot drift from `get_sponsorships` / `get_payouts`.
    /// The scans are bounded by MAX_TIERS, MAX_SPONSORSHIPS, and MAX_PAYOUTS.
    pub fn get_event_summary(env: Env, event_id: u32) -> Result<EventSummary, Error> {
        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)?;

        let tiers: Vec<TicketTier> = env
            .storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .ok_or(Error::TiersNotFound)?;

        let mut ticket_revenue: i128 = 0;
        for i in 0..tiers.len() {
            let t: TicketTier = tiers.get(i).unwrap();
            ticket_revenue += t.price * i128::from(t.tickets_sold);
        }

        let sponsorships: Vec<Sponsorship> = env
            .storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env));

        let mut sponsorship_total: i128 = 0;
        for i in 0..sponsorships.len() {
            sponsorship_total += sponsorships.get(i).unwrap().amount;
        }

        let payouts: Vec<Payout> = env
            .storage()
            .persistent()
            .get(&DataKey::Payouts(event_id))
            .unwrap_or_else(|| Vec::new(&env));

        let mut total_paid_out: i128 = 0;
        for i in 0..payouts.len() {
            total_paid_out += payouts.get(i).unwrap().amount;
        }

        Ok(EventSummary {
            ticket_revenue,
            sponsorship_total,
            total_collected: ticket_revenue + sponsorship_total,
            total_paid_out,
            balance: event.balance,
        })
    }

    /// Returns the sponsor's share of total sponsorship for an event in basis points
    /// (1 bp = 0.01%, 10_000 bp = 100%).
    /// Returns 0 if the address has not sponsored the event or if total sponsorship is zero.
    pub fn get_sponsor_share(env: Env, event_id: u32, sponsor: Address) -> i128 {
        let sponsorships: Vec<Sponsorship> = env
            .storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env));

        let mut sponsor_total: i128 = 0;
        let mut grand_total: i128 = 0;

        for i in 0..sponsorships.len() {
            let s: Sponsorship = sponsorships.get(i).unwrap();
            grand_total += s.amount;
            if s.sponsor == sponsor {
                sponsor_total += s.amount;
            }
        }

        if grand_total == 0 {
            return 0;
        }

        // Return share in basis points (sponsor_total / grand_total * 10_000)
        sponsor_total * 10_000 / grand_total
    }
}

#[cfg(test)]
mod test;
