/**
 * NovaEvents TypeScript SDK
 *
 * Auto-generated bindings for the NovaEvents Soroban contract.
 * Contract ID: CABTSQOXHOOAFFWBPDIXAPAL7KKV76WFL3WLGBUH6SLJ7R2BO5YNWKFU
 * Network: testnet
 *
 * Generated via:
 *   stellar contract bindings typescript \
 *     --contract-id CABTSQOXHOOAFFWBPDIXAPAL7KKV76WFL3WLGBUH6SLJ7R2BO5YNWKFU \
 *     --network testnet \
 *     --output-dir ./sdk
 */

import {
  Account,
  Address,
  Contract,
  Keypair,
  nativeToScVal,
  Networks,
  rpc,
  scValToNative,
  TransactionBuilder,
  xdr,
} from "@stellar/stellar-sdk";

// ─── Constants ────────────────────────────────────────────────────────────────

export const CONTRACT_ID =
  "CABTSQOXHOOAFFWBPDIXAPAL7KKV76WFL3WLGBUH6SLJ7R2BO5YNWKFU";

export const TESTNET_RPC_URL = "https://soroban-testnet.stellar.org";
export const TESTNET_NETWORK_PASSPHRASE = Networks.TESTNET;

// ─── Types ────────────────────────────────────────────────────────────────────

/** Input shape for a ticket tier when creating an event. */
export interface TierInput {
  name: string;
  /** Price in USDC stroops (1 USDC = 10_000_000). */
  price: bigint;
  supply_cap: number;
}

/** Stored state for a ticket tier, extended with live sales count. */
export interface TicketTier {
  name: string;
  price: bigint;
  supply_cap: number;
  tickets_sold: number;
}

export type EventStatus = "Active" | "Ended" | "Cancelled";

export interface NovaEvent {
  organizer: string;
  name: string;
  description: string;
  venue: string;
  /** Unix timestamp of the event date. */
  date_unix: bigint;
  /** Funding goal in USDC stroops. */
  funding_goal: bigint;
  /** Current USDC balance held by the contract for this event. */
  balance: bigint;
  status: EventStatus;
}

/** On-chain proof of ticket ownership. */
export interface Ticket {
  event_id: number;
  tier_index: number;
  owner: string;
  redeemed: boolean;
}

/** A public sponsorship contribution record. */
export interface Sponsorship {
  sponsor: string;
  amount: bigint;
}

/** A record of a single payout disbursed from an event's balance. */
export interface Payout {
  recipient: string;
  amount: bigint;
}

/**
 * Per-event resale rules for paid ticket transfers.
 * `royalty_bps` is in basis points (1 bp = 0.01%, 10_000 bp = 100%).
 */
export interface ResaleRules {
  max_price: bigint;
  royalty_bps: number;
}

// ─── XDR helpers ─────────────────────────────────────────────────────────────

function addressToScVal(address: string): xdr.ScVal {
  return nativeToScVal(Address.fromString(address), { type: "address" });
}

function tierInputToScVal(tier: TierInput): xdr.ScVal {
  return xdr.ScVal.scvMap([
    new xdr.ScMapEntry({
      key: xdr.ScVal.scvSymbol("name"),
      val: nativeToScVal(tier.name, { type: "string" }),
    }),
    new xdr.ScMapEntry({
      key: xdr.ScVal.scvSymbol("price"),
      val: nativeToScVal(tier.price, { type: "i128" }),
    }),
    new xdr.ScMapEntry({
      key: xdr.ScVal.scvSymbol("supply_cap"),
      val: nativeToScVal(tier.supply_cap, { type: "u32" }),
    }),
  ]);
}

function tierInputsToScVal(tiers: TierInput[]): xdr.ScVal {
  return xdr.ScVal.scvVec(tiers.map(tierInputToScVal));
}

function scValToEvent(val: xdr.ScVal): NovaEvent {
  const native = scValToNative(val) as Record<string, unknown>;
  return {
    organizer: (native["organizer"] as Address).toString(),
    name: native["name"] as string,
    description: native["description"] as string,
    venue: native["venue"] as string,
    date_unix: native["date_unix"] as bigint,
    funding_goal: native["funding_goal"] as bigint,
    balance: native["balance"] as bigint,
    status: native["status"] as EventStatus,
  };
}

function scValToTicketTier(val: xdr.ScVal): TicketTier {
  const native = scValToNative(val) as Record<string, unknown>;
  return {
    name: native["name"] as string,
    price: native["price"] as bigint,
    supply_cap: native["supply_cap"] as number,
    tickets_sold: native["tickets_sold"] as number,
  };
}

function scValToTicket(val: xdr.ScVal): Ticket {
  const native = scValToNative(val) as Record<string, unknown>;
  return {
    event_id: native["event_id"] as number,
    tier_index: native["tier_index"] as number,
    owner: (native["owner"] as Address).toString(),
    redeemed: native["redeemed"] as boolean,
  };
}

function scValToSponsorship(val: xdr.ScVal): Sponsorship {
  const native = scValToNative(val) as Record<string, unknown>;
  return {
    sponsor: (native["sponsor"] as Address).toString(),
    amount: native["amount"] as bigint,
  };
}

function scValToPayout(val: xdr.ScVal): Payout {
  const native = scValToNative(val) as Record<string, unknown>;
  return {
    recipient: (native["recipient"] as Address).toString(),
    amount: native["amount"] as bigint,
  };
}

function scValToResaleRules(val: xdr.ScVal): ResaleRules {
  const native = scValToNative(val) as Record<string, unknown>;
  return {
    max_price: native["max_price"] as bigint,
    royalty_bps: native["royalty_bps"] as number,
  };
}

// ─── Client options ───────────────────────────────────────────────────────────

export interface NovaEventsClientOptions {
  /** Soroban RPC URL. Defaults to testnet. */
  rpcUrl?: string;
  /** Network passphrase. Defaults to testnet. */
  networkPassphrase?: string;
  /** Contract ID. Defaults to the deployed testnet contract. */
  contractId?: string;
}

export interface SignerOptions {
  /** Keypair used to sign the transaction. Required for write operations. */
  signer: Keypair;
  /** Fee in stroops. Defaults to 100. */
  fee?: string;
}

// ─── NovaEventsClient ────────────────────────────────────────────────────────

/**
 * Typed client for the NovaEvents Soroban contract.
 *
 * @example
 * ```ts
 * import { NovaEventsClient } from "novaevents-sdk";
 *
 * const client = new NovaEventsClient();
 * const event = await client.get_event(0);
 * console.log(event.name);
 * ```
 */
export class NovaEventsClient {
  private readonly server: rpc.Server;
  private readonly contractId: string;
  private readonly networkPassphrase: string;
  private readonly contract: Contract;

  constructor(opts: NovaEventsClientOptions = {}) {
    this.contractId = opts.contractId ?? CONTRACT_ID;
    this.networkPassphrase =
      opts.networkPassphrase ?? TESTNET_NETWORK_PASSPHRASE;
    this.server = new rpc.Server(opts.rpcUrl ?? TESTNET_RPC_URL, {
      allowHttp: false,
    });
    this.contract = new Contract(this.contractId);
  }

  // ─── Helpers ───────────────────────────────────────────────────────────────

  /** Build, simulate, sign, and submit a transaction. */
  private async invoke(
    operation: xdr.Operation,
    opts: SignerOptions
  ): Promise<xdr.ScVal> {
    const source = await this.server.getAccount(
      opts.signer.publicKey()
    );
    const fee = opts.fee ?? "100";
    const tx = new TransactionBuilder(source, {
      fee,
      networkPassphrase: this.networkPassphrase,
    })
      .addOperation(operation)
      .setTimeout(30)
      .build();

    const simResult = await this.server.simulateTransaction(tx);
    if (rpc.Api.isSimulationError(simResult)) {
      throw new Error(`Simulation failed: ${simResult.error}`);
    }

    const prepared = rpc.assembleTransaction(tx, simResult).build();
    prepared.sign(opts.signer);

    const sendResult = await this.server.sendTransaction(prepared);
    if (sendResult.status === "ERROR") {
      throw new Error(
        `Transaction failed: ${JSON.stringify(sendResult.errorResult)}`
      );
    }

    // Poll until confirmed
    let getResult = await this.server.getTransaction(sendResult.hash);
    while (
      getResult.status === rpc.Api.GetTransactionStatus.NOT_FOUND
    ) {
      await new Promise((r) => setTimeout(r, 1000));
      getResult = await this.server.getTransaction(sendResult.hash);
    }

    if (getResult.status === rpc.Api.GetTransactionStatus.FAILED) {
      throw new Error("Transaction failed on-chain");
    }

    return (getResult as rpc.Api.GetSuccessfulTransactionResponse)
      .returnValue ?? xdr.ScVal.scvVoid();
  }

  /** Simulate a read-only call and return the raw ScVal result. */
  private async query(operation: xdr.Operation): Promise<xdr.ScVal> {
    // For read-only calls, simulation doesn't require a funded account —
    // a fresh keypair with sequence "0" is enough to build a valid transaction.
    const dummySource = new Account(Keypair.random().publicKey(), "0");
    const simResult = await this.server.simulateTransaction(
      new TransactionBuilder(dummySource, {
        fee: "100",
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build()
    );

    if (rpc.Api.isSimulationError(simResult)) {
      throw new Error(`Query failed: ${simResult.error}`);
    }

    const success = simResult as rpc.Api.SimulateTransactionSuccessResponse;
    return success.result?.retval ?? xdr.ScVal.scvVoid();
  }

  // ─── Write functions ───────────────────────────────────────────────────────

  /**
   * One-time setup: authorize an admin and record the USDC token contract address.
   */
  async initialize(
    admin: string,
    token: string,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "initialize",
      addressToScVal(admin),
      addressToScVal(token)
    );
    await this.invoke(op, opts);
  }

  /**
   * Organizer creates a new event with one or more ticket tiers.
   * @returns The new event ID.
   */
  async create_event(
    organizer: string,
    name: string,
    description: string,
    venue: string,
    date_unix: bigint,
    funding_goal: bigint,
    tiers: TierInput[],
    opts: SignerOptions
  ): Promise<number> {
    const op = this.contract.call(
      "create_event",
      addressToScVal(organizer),
      nativeToScVal(name, { type: "string" }),
      nativeToScVal(description, { type: "string" }),
      nativeToScVal(venue, { type: "string" }),
      nativeToScVal(date_unix, { type: "u64" }),
      nativeToScVal(funding_goal, { type: "i128" }),
      tierInputsToScVal(tiers)
    );
    const result = await this.invoke(op, opts);
    return scValToNative(result) as number;
  }

  /**
   * Buyer purchases a ticket in a given tier.
   * Transfers `tier.price` USDC from buyer to the contract.
   * @returns The new ticket ID.
   */
  async buy_ticket(
    buyer: string,
    event_id: number,
    tier_index: number,
    opts: SignerOptions
  ): Promise<number> {
    const op = this.contract.call(
      "buy_ticket",
      addressToScVal(buyer),
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(tier_index, { type: "u32" })
    );
    const result = await this.invoke(op, opts);
    return scValToNative(result) as number;
  }

  /**
   * Organizer checks in (redeems) a ticket at the door.
   */
  async redeem_ticket(
    organizer: string,
    event_id: number,
    ticket_id: number,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "redeem_ticket",
      addressToScVal(organizer),
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(ticket_id, { type: "u32" })
    );
    await this.invoke(op, opts);
  }

  /**
   * Sponsor contributes USDC to an event.
   * Contribution is recorded publicly against the sponsor's address.
   */
  async sponsor_event(
    sponsor: string,
    event_id: number,
    amount: bigint,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "sponsor_event",
      addressToScVal(sponsor),
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(amount, { type: "i128" })
    );
    await this.invoke(op, opts);
  }

  /**
   * Transfers ticket ownership from the current owner to a new address.
   */
  async transfer_ticket(
    from: string,
    event_id: number,
    ticket_id: number,
    to: string,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "transfer_ticket",
      addressToScVal(from),
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(ticket_id, { type: "u32" }),
      addressToScVal(to)
    );
    await this.invoke(op, opts);
  }

  /**
   * Organizer sets (or replaces) the resale rules for an event: a maximum
   * resale price and a royalty percentage (in basis points) taken from
   * every paid resale via `resell_ticket`.
   */
  async set_resale_rules(
    organizer: string,
    event_id: number,
    max_price: bigint,
    royalty_bps: number,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "set_resale_rules",
      addressToScVal(organizer),
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(max_price, { type: "i128" }),
      nativeToScVal(royalty_bps, { type: "u32" })
    );
    await this.invoke(op, opts);
  }

  /**
   * Transfers ticket ownership from `from` to `to`, paid according to the
   * event's resale rules. With no resale rules configured, this behaves
   * exactly like `transfer_ticket` (a free reassignment) and `price` is
   * ignored. With resale rules set, `price` must be positive and capped at
   * the configured max price; both `from` and `to` must sign.
   */
  async resell_ticket(
    from: string,
    event_id: number,
    ticket_id: number,
    to: string,
    price: bigint,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "resell_ticket",
      addressToScVal(from),
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(ticket_id, { type: "u32" }),
      addressToScVal(to),
      nativeToScVal(price, { type: "i128" })
    );
    await this.invoke(op, opts);
  }

  /**
   * Organizer disburses `amount` USDC from the event balance to `recipient`.
   * Only callable on an Ended event.
   */
  async payout(
    organizer: string,
    event_id: number,
    recipient: string,
    amount: bigint,
    opts: SignerOptions
  ): Promise<void> {
    const op = this.contract.call(
      "payout",
      addressToScVal(organizer),
      nativeToScVal(event_id, { type: "u32" }),
      addressToScVal(recipient),
      nativeToScVal(amount, { type: "i128" })
    );
    await this.invoke(op, opts);
  }

  // ─── Read-only queries ─────────────────────────────────────────────────────

  /** Fetch full event details by ID. */
  async get_event(event_id: number): Promise<NovaEvent> {
    const op = this.contract.call(
      "get_event",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    return scValToEvent(result);
  }

  /** Fetch all ticket tiers for an event. */
  async get_tiers(event_id: number): Promise<TicketTier[]> {
    const op = this.contract.call(
      "get_tiers",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    const vec = result.vec() ?? [];
    return vec.map(scValToTicketTier);
  }

  /** Fetch a specific ticket by event ID and ticket ID. */
  async get_ticket(event_id: number, ticket_id: number): Promise<Ticket> {
    const op = this.contract.call(
      "get_ticket",
      nativeToScVal(event_id, { type: "u32" }),
      nativeToScVal(ticket_id, { type: "u32" })
    );
    const result = await this.query(op);
    return scValToTicket(result);
  }

  /** Fetch all sponsorships for an event. */
  async get_sponsorships(event_id: number): Promise<Sponsorship[]> {
    const op = this.contract.call(
      "get_sponsorships",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    const vec = result.vec() ?? [];
    return vec.map(scValToSponsorship);
  }

  /** Returns the total number of events created. */
  async event_count(): Promise<number> {
    const op = this.contract.call("event_count");
    const result = await this.query(op);
    return scValToNative(result) as number;
  }

  /** Returns the total number of tickets sold for an event. */
  async ticket_count(event_id: number): Promise<number> {
    const op = this.contract.call(
      "ticket_count",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    return scValToNative(result) as number;
  }

  /** Returns the USDC token contract address configured during initialize. */
  async get_token(): Promise<string> {
    const op = this.contract.call("get_token");
    const result = await this.query(op);
    return Address.fromScVal(result).toString();
  }

  /** Returns the admin address configured during initialize. */
  async get_admin(): Promise<string> {
    const op = this.contract.call("get_admin");
    const result = await this.query(op);
    return Address.fromScVal(result).toString();
  }

  /** Fetch every payout disbursed for an event. */
  async get_payouts(event_id: number): Promise<Payout[]> {
    const op = this.contract.call(
      "get_payouts",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    const vec = result.vec() ?? [];
    return vec.map(scValToPayout);
  }

  /**
   * Returns the current USDC balance (in stroops) held by the contract for an event.
   */
  async get_balance(event_id: number): Promise<bigint> {
    const op = this.contract.call(
      "get_balance",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    return scValToNative(result) as bigint;
  }

  /** Returns the resale rules configured for an event, or null if none are set. */
  async get_resale_rules(event_id: number): Promise<ResaleRules | null> {
    const op = this.contract.call(
      "get_resale_rules",
      nativeToScVal(event_id, { type: "u32" })
    );
    const result = await this.query(op);
    if (result.switch().name === "scvVoid") {
      return null;
    }
    return scValToResaleRules(result);
  }

  /**
   * Returns the sponsor's share of total sponsorship for an event in basis points
   * (1 bp = 0.01%, 10_000 bp = 100%).
   */
  async get_sponsor_share(event_id: number, sponsor: string): Promise<bigint> {
    const op = this.contract.call(
      "get_sponsor_share",
      nativeToScVal(event_id, { type: "u32" }),
      addressToScVal(sponsor)
    );
    const result = await this.query(op);
    return scValToNative(result) as bigint;
  }
}
