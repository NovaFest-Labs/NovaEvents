# NovaEvents SDK

TypeScript bindings for the [NovaEvents](https://github.com/CmxTop/NovaEvents) Soroban smart contract on Stellar.

**Contract ID (testnet):** `CABTSQOXHOOAFFWBPDIXAPAL7KKV76WFL3WLGBUH6SLJ7R2BO5YNWKFU`

> **Decision:** The SDK lives at `sdk/` inside the main repo. This keeps the contract source and its bindings together, simplifying versioning and making it easy for contributors to update types when the contract ABI changes.

---

## Installation

```bash
npm install novaevents-sdk
# or
yarn add novaevents-sdk
```

---

## Quick start

```ts
import { NovaEventsClient } from "novaevents-sdk";

// Read-only — no signer needed
const client = new NovaEventsClient();

const count = await client.event_count();
console.log("Total events:", count);

const event = await client.get_event(0);
console.log(event.name, event.status);

const tiers = await client.get_tiers(0);
tiers.forEach((t) => console.log(t.name, t.price, t.tickets_sold));
```

---

## Write operations

Write operations mutate on-chain state and require a signed transaction.  
Pass a `{ signer: Keypair }` object as the last argument.

```ts
import { Keypair } from "@stellar/stellar-sdk";
import { NovaEventsClient, TierInput } from "novaevents-sdk";

const client = new NovaEventsClient();
const signer = Keypair.fromSecret("S...");

// Create an event
const tiers: TierInput[] = [
  { name: "General", price: 10_000_000n, supply_cap: 100 }, // 1 USDC
  { name: "VIP",     price: 50_000_000n, supply_cap: 10  }, // 5 USDC
];

const eventId = await client.create_event(
  signer.publicKey(),       // organizer
  "My Concert",
  "An awesome show",
  "Madison Square Garden",
  BigInt(Math.floor(Date.now() / 1000) + 86400), // tomorrow
  100_000_000n,             // funding goal: 10 USDC
  tiers,
  { signer }
);
console.log("Created event ID:", eventId);

// Buy a ticket
const ticketId = await client.buy_ticket(
  signer.publicKey(), // buyer
  eventId,
  0,                  // General tier
  { signer }
);
console.log("Ticket ID:", ticketId);

// Redeem a ticket (organizer)
await client.redeem_ticket(signer.publicKey(), eventId, ticketId, { signer });

// Sponsor an event
await client.sponsor_event(signer.publicKey(), eventId, 20_000_000n, { signer });
```

---

## API reference

### Constructor

```ts
new NovaEventsClient(opts?: {
  rpcUrl?: string;           // default: Testnet RPC
  networkPassphrase?: string; // default: Testnet passphrase
  contractId?: string;        // default: deployed testnet contract ID
})
```

### Write methods

| Method | Description |
|--------|-------------|
| `initialize(admin, token, opts)` | One-time setup; sets admin and USDC token address |
| `create_event(organizer, name, description, venue, date_unix, funding_goal, tiers, opts)` → `number` | Create a new event; returns event ID |
| `update_event_details(organizer, event_id, name, description, venue, date_unix, opts)` | Correct an event's details before any tier has sold a ticket |
| `buy_ticket(buyer, event_id, tier_index, opts)` → `number` | Purchase a ticket; returns ticket ID |
| `redeem_ticket(organizer, event_id, ticket_id, opts)` | Mark a ticket as redeemed |
| `end_event(organizer, event_id, opts)` | Close an event, transitioning it from Active to Ended |
| `cancel_event(organizer, event_id, opts)` | Cancel an Active event, refunding tickets and sponsorships |
| `sponsor_event(sponsor, event_id, amount, opts)` | Contribute USDC to an event |
| `transfer_ticket(from, event_id, ticket_id, to, opts)` | Transfer ticket ownership (free reassignment) |
| `set_resale_rules(organizer, event_id, max_price, royalty_bps, opts)` | Configure resale price cap and organizer royalty for an event |
| `resell_ticket(from, event_id, ticket_id, to, price, opts)` | Transfer a ticket, paid according to the event's resale rules |
| `pause(admin, opts)` | Halt all state-changing contract functions in an emergency |
| `unpause(admin, opts)` | Resume normal operations after an emergency pause |
| `payout(organizer, event_id, recipient, amount, opts)` | Disburse USDC from an Ended event's balance |

### Read-only methods

| Method | Returns | Description |
|--------|---------|-------------|
| `get_event(event_id)` | `NovaEvent` | Full event details |
| `get_tiers(event_id)` | `TicketTier[]` | All ticket tiers for an event |
| `get_ticket(event_id, ticket_id)` | `Ticket` | Specific ticket details |
| `get_sponsorships(event_id)` | `Sponsorship[]` | All sponsorships for an event |
| `event_count()` | `number` | Total number of events |
| `ticket_count(event_id)` | `number` | Tickets sold for an event |
| `get_token()` | `string` | Configured USDC token contract address |
| `get_admin()` | `string` | Configured admin address |
| `is_paused()` | `boolean` | Whether the contract is currently paused |
| `get_payouts(event_id)` | `Payout[]` | All payouts disbursed for an event |
| `get_royalties(event_id)` | `Royalty[]` | All resale royalties recorded for an event |
| `get_resale_rules(event_id)` | `ResaleRules \| null` | Configured resale rules for an event, if any |
| `get_balance(event_id)` | `bigint` | Contract's USDC balance for an event (stroops) |
| `get_sponsor_share(event_id, sponsor)` | `bigint` | Sponsor's share in basis points (0–10 000) |
| `get_event_summary(event_id)` | `EventSummary` | Aggregated financial summary of an event |

---

## Types

```ts
interface TierInput   { name: string; price: bigint; supply_cap: number }
interface TicketTier  { name: string; price: bigint; supply_cap: number; tickets_sold: number }
interface NovaEvent   { organizer: string; name: string; description: string; venue: string;
                        date_unix: bigint; funding_goal: bigint; balance: bigint; status: EventStatus }
interface Ticket      { event_id: number; tier_index: number; owner: string; redeemed: boolean }
interface Sponsorship { sponsor: string; amount: bigint }
interface Payout      { recipient: string; amount: bigint }
interface Royalty     { ticket_id: number; amount: bigint }
interface ResaleRules { max_price: bigint; royalty_bps: number }
interface EventSummary { ticket_revenue: bigint; sponsorship_total: bigint; total_collected: bigint;
                        total_paid_out: bigint; balance: bigint; royalty_total: bigint }
type EventStatus = "Active" | "Ended" | "Cancelled"
```

> Prices and amounts are in **USDC stroops** — divide by `10_000_000` to get USDC.

---

## Building

```bash
cd sdk
npm install
npm run build
```

Output is written to `sdk/dist/`.

---

## Regenerating bindings

If the contract is redeployed or the ABI changes, regenerate with:

```bash
stellar contract bindings typescript \
  --contract-id CABTSQOXHOOAFFWBPDIXAPAL7KKV76WFL3WLGBUH6SLJ7R2BO5YNWKFU \
  --network testnet \
  --output-dir ./sdk
```
