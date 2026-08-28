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
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct NovaEventsContract;

#[contractimpl]
impl NovaEventsContract {
    /// One-time setup: authorize an admin and record the USDC token contract address.
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

    /// Organizer creates a new event with one or more ticket tiers.
    /// Returns the new event ID.
    // Each parameter is an independent required field on a Soroban entrypoint;
    // bundling them into a struct would be a breaking ABI change tracked separately.
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

        let mut organizer_events: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::OrganizerEvents(organizer.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        organizer_events.push_back(event_id);
        env.storage()
            .persistent()
            .set(&DataKey::OrganizerEvents(organizer), &organizer_events);

        Ok(event_id)
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
        buyer.require_auth();

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
        if tier.tickets_sold >= tier.supply_cap {
            return Err(Error::TierSoldOut);
        }

        let price = tier.price;
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
                owner: buyer.clone(),
                redeemed: false,
            },
        );

        TokenClient::new(&env, &token_addr).transfer(
            &buyer,
            env.current_contract_address(),
            &price,
        );

        Ok(ticket_id)
    }

    /// Organizer checks in (redeems) a ticket at the door.
    pub fn redeem_ticket(
        env: Env,
        organizer: Address,
        event_id: u32,
        ticket_id: u32,
    ) -> Result<(), Error> {
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

    // ─── Queries — readable by anyone ────────────────────────────────────────

    pub fn get_event(env: Env, event_id: u32) -> Result<Event, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(Error::EventNotFound)
    }

    pub fn get_tiers(env: Env, event_id: u32) -> Result<Vec<TicketTier>, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Tiers(event_id))
            .ok_or(Error::TiersNotFound)
    }

    pub fn get_ticket(env: Env, event_id: u32, ticket_id: u32) -> Result<Ticket, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Ticket(event_id, ticket_id))
            .ok_or(Error::TicketNotFound)
    }

    pub fn get_sponsorships(env: Env, event_id: u32) -> Vec<Sponsorship> {
        env.storage()
            .persistent()
            .get(&DataKey::Sponsorships(event_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_payouts(env: Env, event_id: u32) -> Vec<Payout> {
        env.storage()
            .persistent()
            .get(&DataKey::Payouts(event_id))
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

    /// Returns the list of event IDs created by a specific organizer address.
    /// Returns an empty list if the address has not created any events.
    pub fn get_events_by_organizer(env: Env, organizer: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::OrganizerEvents(organizer))
            .unwrap_or_else(|| Vec::new(&env))
    }
}

#[cfg(test)]
mod test;
