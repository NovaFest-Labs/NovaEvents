#!/usr/bin/env bash
#
# NovaEvents — end-to-end lifecycle verification.
#
# Drives the deployed contract through a full, realistic event lifecycle using
# only the Stellar CLI and RPC:
#
#   deploy -> initialize -> create_event -> buy_ticket -> transfer_ticket
#          -> redeem_ticket -> sponsor_event -> end_event -> payout
#
# Unlike `cargo test`, which runs against an in-process `Env::default()`, this
# exercises the real serialization, auth, and cross-contract token path, so it
# catches deployment- and ABI-level breakage that unit tests cannot see.
#
# Usage:
#   scripts/e2e_lifecycle.sh                      # local sandbox (needs Docker)
#   scripts/e2e_lifecycle.sh --network testnet    # public testnet (needs internet)
#   scripts/e2e_lifecycle.sh --help
#
set -euo pipefail

# ─── Configuration ────────────────────────────────────────────────────────────

NETWORK="local"
SKIP_BUILD=0
KEEP_SANDBOX=0
CONTAINER_NAME="nova-events-e2e"

WASM_PATH="target/wasm32v1-none/release/nova_events.wasm"

# Ticket tier prices, in stroops (7 decimals — see CONTRIBUTING.md).
readonly TIER0_PRICE=10000000   # 1.0
readonly TIER1_PRICE=50000000   # 5.0
readonly SPONSOR_AMOUNT=250000000 # 25.0
readonly PAYOUT_AMOUNT=30000000  # 3.0
readonly FUNDING_GOAL=1000000000 # 100.0

# Contract error codes (must stay in sync with `Error` in src/lib.rs).
readonly ERR_EVENT_NOT_FOUND=3
readonly ERR_UNAUTHORIZED=6
readonly ERR_EVENT_NOT_ACTIVE=7
readonly ERR_ALREADY_REDEEMED=8
readonly ERR_INVALID_RECIPIENT=26
readonly ERR_EVENT_NOT_CANCELLED=35
readonly ERR_ALREADY_REFUNDED=36

# ─── Output helpers ───────────────────────────────────────────────────────────

if [[ -t 1 ]]; then
    BOLD=$'\033[1m'; RED=$'\033[31m'; GREEN=$'\033[32m'
    YELLOW=$'\033[33m'; BLUE=$'\033[34m'; RESET=$'\033[0m'
else
    BOLD=""; RED=""; GREEN=""; YELLOW=""; BLUE=""; RESET=""
fi

CHECKS_PASSED=0
CHECKS_SKIPPED=0

step()  { printf '\n%s▸ %s%s\n' "$BOLD$BLUE" "$*" "$RESET"; }
info()  { printf '  %s\n' "$*"; }
pass()  { CHECKS_PASSED=$((CHECKS_PASSED + 1)); printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
skip()  { CHECKS_SKIPPED=$((CHECKS_SKIPPED + 1)); printf '  %s—%s %s\n' "$YELLOW" "$RESET" "$*"; }
warn()  { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$*"; }
die()   { printf '\n%s✗ %s%s\n\n' "$RED$BOLD" "$*" "$RESET" >&2; exit 1; }

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
    cat <<'USAGE'

Options:
  --network <local|testnet>  Target network. Default: local.
  --skip-build               Reuse the existing WASM instead of rebuilding.
  --keep-sandbox             Leave the local container running when done.
  -h, --help                 Show this help.
USAGE
}

# ─── Argument parsing ─────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --network)            NETWORK="${2:-}"; shift 2 ;;
        --network=*)          NETWORK="${1#*=}"; shift ;;
        --skip-build)         SKIP_BUILD=1; shift ;;
        --keep-sandbox)       KEEP_SANDBOX=1; shift ;;
        -h|--help)            usage; exit 0 ;;
        *)                    usage >&2; die "Unknown argument: $1" ;;
    esac
done

case "$NETWORK" in
    local|testnet) ;;
    *) die "--network must be 'local' or 'testnet' (got '$NETWORK')" ;;
esac

# Run from the repository root regardless of the caller's working directory.
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ─── Cleanup ──────────────────────────────────────────────────────────────────

RUN_ID="$$-$(date +%s)"
ORGANIZER_KEY="nova-e2e-organizer-$RUN_ID"
ATTENDEE_KEY="nova-e2e-attendee-$RUN_ID"
SPONSOR_KEY="nova-e2e-sponsor-$RUN_ID"
WORKER_KEY="nova-e2e-worker-$RUN_ID"

STDERR_LOG="$(mktemp)"
SANDBOX_STARTED=0

cleanup() {
    local code=$?
    rm -f "$STDERR_LOG"
    for key in "$ORGANIZER_KEY" "$ATTENDEE_KEY" "$SPONSOR_KEY" "$WORKER_KEY"; do
        stellar keys rm --force "$key" >/dev/null 2>&1 || true
    done
    if (( SANDBOX_STARTED == 1 && KEEP_SANDBOX == 0 )); then
        printf '\n  stopping local sandbox…\n'
        stellar container stop "$CONTAINER_NAME" >/dev/null 2>&1 || true
    fi
    exit "$code"
}
trap cleanup EXIT INT TERM

# ─── CLI wrappers ─────────────────────────────────────────────────────────────

# Invoke a state-changing contract function. Echoes the JSON return value.
tx() {
    local source="$1"; shift
    local out rc=0
    out=$(stellar contract invoke --id "$CONTRACT_ID" --source "$source" \
        --network "$NETWORK" -- "$@" 2>"$STDERR_LOG") || rc=$?
    if (( rc != 0 )); then
        cat "$STDERR_LOG" >&2
        die "invoke $1 failed (exit $rc)"
    fi
    printf '%s' "$out"
}

# Invoke a read-only contract function via simulation (no transaction submitted).
call() {
    local out rc=0
    out=$(stellar contract invoke --id "$CONTRACT_ID" --source "$ORGANIZER_KEY" \
        --network "$NETWORK" --send=no -- "$@" 2>"$STDERR_LOG") || rc=$?
    if (( rc != 0 )); then
        cat "$STDERR_LOG" >&2
        die "query $1 failed (exit $rc)"
    fi
    printf '%s' "$out"
}

# Read a token balance through the token contract itself, not contract state.
token_balance() {
    stellar contract invoke --id "$TOKEN_ID" --source "$ORGANIZER_KEY" \
        --network "$NETWORK" --send=no -- balance --id "$1" 2>/dev/null | tr -d '"'
}

# ─── Assertions ───────────────────────────────────────────────────────────────

assert_eq() {
    local expected="$1" actual="$2" label="$3"
    if [[ "$expected" == "$actual" ]]; then
        pass "$label"
    else
        die "$label
    expected: $expected
    actual:   $actual"
    fi
}

assert_json_eq() {
    local field="$1" expected="$2" json="$3" label="$4"
    assert_eq "$expected" "$(printf '%s' "$json" | jq -r "$field")" "$label"
}

# Assert that a contract call fails with a specific `#[contracterror]` code.
# Usage: assert_error <code> <label> <source-key> <fn> [args...]
assert_error() {
    local expected="$1" label="$2" source="$3"; shift 3
    local rc=0
    stellar contract invoke --id "$CONTRACT_ID" --source "$source" \
        --network "$NETWORK" -- "$@" >/dev/null 2>"$STDERR_LOG" || rc=$?
    if (( rc == 0 )); then
        die "$label: call unexpectedly succeeded (expected Error #$expected)"
    fi
    if grep -q "Error(Contract, #$expected)" "$STDERR_LOG"; then
        pass "$label (Error #$expected)"
    else
        cat "$STDERR_LOG" >&2
        die "$label: expected Error(Contract, #$expected), got the output above"
    fi
}

# ─── 1. Preflight ─────────────────────────────────────────────────────────────

step "Preflight"

for bin in stellar jq; do
    command -v "$bin" >/dev/null 2>&1 || die "'$bin' is required but not installed."
done
info "stellar CLI: $(stellar --version 2>/dev/null | head -1)"

if (( SKIP_BUILD == 0 )); then
    command -v cargo >/dev/null 2>&1 || die "'cargo' is required but not installed."
    if ! rustup target list --installed 2>/dev/null | grep -q '^wasm32v1-none$'; then
        die "The wasm32v1-none target is missing. Run: rustup target add wasm32v1-none"
    fi
    info "building contract…"
    cargo build --target wasm32v1-none --release >/dev/null
fi
[[ -f "$WASM_PATH" ]] || die "WASM not found at $WASM_PATH (drop --skip-build to build it)."
pass "contract WASM ready ($(wc -c < "$WASM_PATH") bytes)"

# ─── 2. Network ───────────────────────────────────────────────────────────────

step "Network: $NETWORK"

if [[ "$NETWORK" == "local" ]]; then
    if ! docker info >/dev/null 2>&1; then
        die "The local sandbox needs a running Docker daemon that your user can reach.
    Start Docker, or run against the public network instead:
        scripts/e2e_lifecycle.sh --network testnet"
    fi
    if curl -fsS -m 5 -X POST -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
        http://localhost:8000/rpc >/dev/null 2>&1; then
        info "reusing the sandbox already listening on :8000"
    else
        info "starting the local Soroban sandbox (first run pulls the image)…"
        stellar container start local --name "$CONTAINER_NAME" >/dev/null
        SANDBOX_STARTED=1
        info "waiting for RPC to become healthy…"
        for _ in $(seq 1 120); do
            if curl -fsS -m 5 -X POST -H 'Content-Type: application/json' \
                -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
                http://localhost:8000/rpc 2>/dev/null | grep -q healthy; then
                break
            fi
            sleep 2
        done
        curl -fsS -m 5 -X POST -H 'Content-Type: application/json' \
            -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
            http://localhost:8000/rpc 2>/dev/null | grep -q healthy \
            || die "The local sandbox did not become healthy within 4 minutes."
    fi
fi
pass "RPC reachable"

# ─── 3. Identities ────────────────────────────────────────────────────────────

step "Identities"

for key in "$ORGANIZER_KEY" "$ATTENDEE_KEY" "$SPONSOR_KEY" "$WORKER_KEY"; do
    stellar keys generate --fund --network "$NETWORK" --overwrite "$key" >/dev/null 2>&1 \
        || die "Could not generate and fund '$key' on $NETWORK."
done

ORGANIZER=$(stellar keys address "$ORGANIZER_KEY")
ATTENDEE=$(stellar keys address "$ATTENDEE_KEY")
SPONSOR=$(stellar keys address "$SPONSOR_KEY")
WORKER=$(stellar keys address "$WORKER_KEY")

info "organizer $ORGANIZER"
info "attendee  $ATTENDEE"
info "sponsor   $SPONSOR"
info "worker    $WORKER"
pass "four roles funded"

# ─── 4. Settlement token ──────────────────────────────────────────────────────

step "Settlement token"

# The contract is token-agnostic: it stores whatever token address `initialize`
# is given. Production uses USDC; here we use the native Stellar Asset Contract
# so the script needs no issuer, trustlines, or minting to run.
if [[ "$NETWORK" == "local" ]]; then
    stellar contract asset deploy --asset native --network "$NETWORK" \
        --source "$ORGANIZER_KEY" >/dev/null 2>&1 || true
fi
TOKEN_ID=$(stellar contract id asset --asset native --network "$NETWORK")
info "native SAC $TOKEN_ID"
pass "settlement token resolved"

# ─── 5. Deploy & initialize ───────────────────────────────────────────────────

step "Deploy & initialize"

CONTRACT_ID=$(stellar contract deploy --wasm "$WASM_PATH" --network "$NETWORK" \
    --source "$ORGANIZER_KEY" 2>"$STDERR_LOG" | tail -1) || true
[[ "$CONTRACT_ID" == C* ]] || { cat "$STDERR_LOG" >&2; die "Deploy failed."; }
info "contract $CONTRACT_ID"
pass "contract deployed"

tx "$ORGANIZER_KEY" initialize --admin "$ORGANIZER" --token "$TOKEN_ID" >/dev/null
pass "initialize accepted"

assert_eq "$ORGANIZER" "$(call get_admin | tr -d '"')" "get_admin returns the admin"
assert_eq "$TOKEN_ID"  "$(call get_token | tr -d '"')" "get_token returns the token"

# A second initialize must be rejected — the one-time-setup guard survives the
# real deploy path, not just the unit-test env.
assert_error 1 "initialize is not repeatable" "$ORGANIZER_KEY" \
    initialize --admin "$ORGANIZER" --token "$TOKEN_ID"

# ─── 6. Create event ──────────────────────────────────────────────────────────

step "Create event"

EVENT_DATE=$(( $(date +%s) + 2592000 )) # 30 days out
TIERS=$(jq -nc \
    --arg p0 "$TIER0_PRICE" --arg p1 "$TIER1_PRICE" \
    '[{name:"General",price:$p0,supply_cap:100},{name:"VIP",price:$p1,supply_cap:10}]')

EVENT_ID=$(tx "$ORGANIZER_KEY" create_event \
    --organizer "$ORGANIZER" \
    --name "NovaFest Lagos" \
    --description "End-to-end lifecycle verification run" \
    --venue "Landmark Centre, Lagos" \
    --date_unix "$EVENT_DATE" \
    --funding_goal "$FUNDING_GOAL" \
    --tiers "$TIERS")

assert_eq "0" "$EVENT_ID" "create_event returns event id 0"
assert_eq "1" "$(call event_count)" "event_count is 1"

EVENT_JSON=$(call get_event --event_id "$EVENT_ID")
assert_json_eq '.status'    "Active"     "$EVENT_JSON" "event status is Active"
assert_json_eq '.organizer' "$ORGANIZER" "$EVENT_JSON" "organizer recorded"
assert_json_eq '.balance'   "0"          "$EVENT_JSON" "event balance starts at 0"

TIERS_JSON=$(call get_tiers --event_id "$EVENT_ID")
assert_json_eq 'length'            "2"             "$TIERS_JSON" "two tiers stored"
assert_json_eq '.[1].name'         "VIP"           "$TIERS_JSON" "tier 1 is VIP"
assert_json_eq '.[1].price'        "$TIER1_PRICE"  "$TIERS_JSON" "tier 1 price round-trips"
assert_json_eq '.[0].tickets_sold' "0"             "$TIERS_JSON" "tier 0 has no sales yet"

# ─── 7. Ticket sales ──────────────────────────────────────────────────────────

step "Ticket sales"

CONTRACT_BAL_BEFORE=$(token_balance "$CONTRACT_ID")

TICKET0=$(tx "$ATTENDEE_KEY" buy_ticket --buyer "$ATTENDEE" --event_id "$EVENT_ID" --tier_index 0)
assert_eq "0" "$TICKET0" "buy_ticket (General) returns ticket 0"

TICKET1=$(tx "$ATTENDEE_KEY" buy_ticket --buyer "$ATTENDEE" --event_id "$EVENT_ID" --tier_index 1)
assert_eq "1" "$TICKET1" "buy_ticket (VIP) returns ticket 1"

assert_eq "2" "$(call ticket_count --event_id "$EVENT_ID")" "ticket_count is 2"

TICKET_JSON=$(call get_ticket --event_id "$EVENT_ID" --ticket_id "$TICKET0")
assert_json_eq '.owner'      "$ATTENDEE" "$TICKET_JSON" "ticket 0 is owned by the attendee"
assert_json_eq '.redeemed'   "false"     "$TICKET_JSON" "ticket 0 is unredeemed"
assert_json_eq '.tier_index' "0"         "$TICKET_JSON" "ticket 0 is in tier 0"

SALES_TOTAL=$((TIER0_PRICE + TIER1_PRICE))
assert_eq "$SALES_TOTAL" "$(call get_balance --event_id "$EVENT_ID" | tr -d '"')" \
    "event balance equals ticket revenue"

# The real cross-contract transfer actually moved funds — an Env::default() test
# with a mock token cannot prove this.
CONTRACT_BAL_AFTER=$(token_balance "$CONTRACT_ID")
assert_eq "$SALES_TOTAL" "$((CONTRACT_BAL_AFTER - CONTRACT_BAL_BEFORE))" \
    "contract token balance moved by the ticket revenue"

assert_error "$ERR_EVENT_NOT_FOUND" "buy_ticket on a missing event is rejected" "$ATTENDEE_KEY" \
    buy_ticket --buyer "$ATTENDEE" --event_id 999 --tier_index 0

# ─── 8. Transfer & check-in ───────────────────────────────────────────────────

step "Transfer & check-in"

assert_error "$ERR_INVALID_RECIPIENT" "transfer to self is rejected" "$ATTENDEE_KEY" \
    transfer_ticket --from "$ATTENDEE" --event_id "$EVENT_ID" --ticket_id "$TICKET1" --to "$ATTENDEE"

tx "$ATTENDEE_KEY" transfer_ticket --from "$ATTENDEE" --event_id "$EVENT_ID" \
    --ticket_id "$TICKET1" --to "$WORKER" >/dev/null
assert_json_eq '.owner' "$WORKER" \
    "$(call get_ticket --event_id "$EVENT_ID" --ticket_id "$TICKET1")" \
    "ticket 1 transferred to the new owner"

assert_error "$ERR_UNAUTHORIZED" "non-organizer cannot check in a ticket" "$SPONSOR_KEY" \
    redeem_ticket --organizer "$SPONSOR" --event_id "$EVENT_ID" --ticket_id "$TICKET0"

tx "$ORGANIZER_KEY" redeem_ticket --organizer "$ORGANIZER" --event_id "$EVENT_ID" \
    --ticket_id "$TICKET0" >/dev/null
assert_json_eq '.redeemed' "true" \
    "$(call get_ticket --event_id "$EVENT_ID" --ticket_id "$TICKET0")" \
    "ticket 0 is marked redeemed"

assert_error "$ERR_ALREADY_REDEEMED" "double check-in is rejected" "$ORGANIZER_KEY" \
    redeem_ticket --organizer "$ORGANIZER" --event_id "$EVENT_ID" --ticket_id "$TICKET0"

# ─── 9. Sponsorship ───────────────────────────────────────────────────────────

step "Sponsorship"

tx "$SPONSOR_KEY" sponsor_event --sponsor "$SPONSOR" --event_id "$EVENT_ID" \
    --amount "$SPONSOR_AMOUNT" >/dev/null

SPONSORSHIPS=$(call get_sponsorships --event_id "$EVENT_ID")
assert_json_eq 'length'       "1"               "$SPONSORSHIPS" "one sponsorship recorded"
assert_json_eq '.[0].sponsor' "$SPONSOR"        "$SPONSORSHIPS" "sponsor address is public"
assert_json_eq '.[0].amount'  "$SPONSOR_AMOUNT" "$SPONSORSHIPS" "sponsorship amount round-trips"

assert_eq "10000" "$(call get_sponsor_share --event_id "$EVENT_ID" --sponsor "$SPONSOR" | tr -d '"')" \
    "sole sponsor holds 100% (10000 bp)"

COLLECTED=$((SALES_TOTAL + SPONSOR_AMOUNT))
assert_eq "$COLLECTED" "$(call get_balance --event_id "$EVENT_ID" | tr -d '"')" \
    "event balance equals sales plus sponsorship"
CONTRACT_BAL_NOW=$(token_balance "$CONTRACT_ID")
assert_eq "$COLLECTED" "$((CONTRACT_BAL_NOW - CONTRACT_BAL_BEFORE))" \
    "contract token balance matches the recorded event balance"

# ─── 10. End event & payout ───────────────────────────────────────────────────

step "End event & payout"

tx "$ORGANIZER_KEY" end_event --organizer "$ORGANIZER" --event_id "$EVENT_ID" >/dev/null
assert_json_eq '.status' "Ended" "$(call get_event --event_id "$EVENT_ID")" \
    "event status is Ended"

assert_error "$ERR_EVENT_NOT_ACTIVE" "sales are locked once ended" "$ATTENDEE_KEY" \
    buy_ticket --buyer "$ATTENDEE" --event_id "$EVENT_ID" --tier_index 0
assert_error "$ERR_EVENT_NOT_ACTIVE" "sponsorship is locked once ended" "$SPONSOR_KEY" \
    sponsor_event --sponsor "$SPONSOR" --event_id "$EVENT_ID" --amount "$SPONSOR_AMOUNT"

WORKER_BAL_BEFORE=$(token_balance "$WORKER")
tx "$ORGANIZER_KEY" payout --organizer "$ORGANIZER" --event_id "$EVENT_ID" \
    --recipient "$WORKER" --amount "$PAYOUT_AMOUNT" >/dev/null

PAYOUTS=$(call get_payouts --event_id "$EVENT_ID")
assert_json_eq 'length'         "1"              "$PAYOUTS" "one payout recorded"
assert_json_eq '.[0].recipient' "$WORKER"        "$PAYOUTS" "payout recipient is the worker"
assert_json_eq '.[0].amount'    "$PAYOUT_AMOUNT" "$PAYOUTS" "payout amount round-trips"

assert_eq "$((COLLECTED - PAYOUT_AMOUNT))" \
    "$(call get_balance --event_id "$EVENT_ID" | tr -d '"')" \
    "event balance is reduced by the payout"
WORKER_BAL_AFTER=$(token_balance "$WORKER")
assert_eq "$PAYOUT_AMOUNT" "$((WORKER_BAL_AFTER - WORKER_BAL_BEFORE))" \
    "worker actually received the funds"

# ─── 11. Cancellation & refunds ───────────────────────────────────────────────

step "Cancellation & refunds"

# A second, independent event: cancel_event only works on an Active event, and
# EVENT_ID is already Ended above.
TIERS2=$(jq -nc --arg p0 "$TIER0_PRICE" '[{name:"General",price:$p0,supply_cap:100}]')
EVENT_ID2=$(tx "$ORGANIZER_KEY" create_event \
    --organizer "$ORGANIZER" \
    --name "NovaFest Abuja (cancelled)" \
    --description "Exercises cancel_event's pull-based refund path" \
    --venue "Transcorp Hilton, Abuja" \
    --date_unix "$EVENT_DATE" \
    --funding_goal "$FUNDING_GOAL" \
    --tiers "$TIERS2")

CTICKET_LIVE=$(tx "$ATTENDEE_KEY" buy_ticket --buyer "$ATTENDEE" --event_id "$EVENT_ID2" --tier_index 0)
CTICKET_REDEEMED=$(tx "$WORKER_KEY" buy_ticket --buyer "$WORKER" --event_id "$EVENT_ID2" --tier_index 0)
tx "$ORGANIZER_KEY" redeem_ticket --organizer "$ORGANIZER" --event_id "$EVENT_ID2" \
    --ticket_id "$CTICKET_REDEEMED" >/dev/null

tx "$SPONSOR_KEY" sponsor_event --sponsor "$SPONSOR" --event_id "$EVENT_ID2" \
    --amount "$SPONSOR_AMOUNT" >/dev/null

PRE_CANCEL_COLLECTED=$((TIER0_PRICE * 2 + SPONSOR_AMOUNT))
assert_eq "$PRE_CANCEL_COLLECTED" "$(call get_balance --event_id "$EVENT_ID2" | tr -d '"')" \
    "event 2 balance holds both ticket sales and the sponsorship"

assert_error "$ERR_EVENT_NOT_CANCELLED" "claim_ticket_refund rejected before cancellation" \
    "$ATTENDEE_KEY" claim_ticket_refund --owner "$ATTENDEE" --event_id "$EVENT_ID2" \
    --ticket_id "$CTICKET_LIVE"

tx "$ORGANIZER_KEY" cancel_event --organizer "$ORGANIZER" --event_id "$EVENT_ID2" >/dev/null
assert_json_eq '.status' "Cancelled" "$(call get_event --event_id "$EVENT_ID2")" \
    "event 2 status is Cancelled"

# cancel_event is pull-based: it moves nothing. The full pre-cancellation
# balance must still be sitting in the contract until each refund is claimed.
assert_eq "$PRE_CANCEL_COLLECTED" "$(call get_balance --event_id "$EVENT_ID2" | tr -d '"')" \
    "cancelling alone does not move any funds"

assert_eq "false" "$(call is_ticket_refunded --event_id "$EVENT_ID2" --ticket_id "$CTICKET_LIVE" | tr -d '"')" \
    "is_ticket_refunded is false before the claim"

# Measured on the contract's own balance, not the attendee's: the attendee
# submits this transaction themselves, so their native-token balance also
# absorbs their own network fee, same reason sponsor_event's balance check
# above reads the contract's side rather than the sponsor's.
CONTRACT_BAL_BEFORE_CLAIM=$(token_balance "$CONTRACT_ID")
tx "$ATTENDEE_KEY" claim_ticket_refund --owner "$ATTENDEE" --event_id "$EVENT_ID2" \
    --ticket_id "$CTICKET_LIVE" >/dev/null
CONTRACT_BAL_AFTER_CLAIM=$(token_balance "$CONTRACT_ID")
assert_eq "$TIER0_PRICE" "$((CONTRACT_BAL_BEFORE_CLAIM - CONTRACT_BAL_AFTER_CLAIM))" \
    "the ticket refund actually left the contract"

assert_eq "true" "$(call is_ticket_refunded --event_id "$EVENT_ID2" --ticket_id "$CTICKET_LIVE" | tr -d '"')" \
    "is_ticket_refunded is true after the claim"
assert_error "$ERR_ALREADY_REFUNDED" "claiming the same ticket refund twice is rejected" \
    "$ATTENDEE_KEY" claim_ticket_refund --owner "$ATTENDEE" --event_id "$EVENT_ID2" \
    --ticket_id "$CTICKET_LIVE"

assert_error "$ERR_ALREADY_REDEEMED" "a ticket redeemed before cancellation is not refunded" \
    "$WORKER_KEY" claim_ticket_refund --owner "$WORKER" --event_id "$EVENT_ID2" \
    --ticket_id "$CTICKET_REDEEMED"

CONTRACT_BAL_BEFORE_CLAIM=$(token_balance "$CONTRACT_ID")
tx "$SPONSOR_KEY" claim_sponsorship_refund --sponsor "$SPONSOR" --event_id "$EVENT_ID2" >/dev/null
CONTRACT_BAL_AFTER_CLAIM=$(token_balance "$CONTRACT_ID")
assert_eq "$SPONSOR_AMOUNT" "$((CONTRACT_BAL_BEFORE_CLAIM - CONTRACT_BAL_AFTER_CLAIM))" \
    "the sponsorship refund actually left the contract"
assert_eq "true" "$(call is_sponsor_refunded --event_id "$EVENT_ID2" --sponsor "$SPONSOR" | tr -d '"')" \
    "is_sponsor_refunded is true after the claim"

# Only the redeemed ticket's payment (never refundable) remains in the
# contract: cancel_event itself moved nothing, and nobody can claim it back.
assert_eq "$TIER0_PRICE" "$(call get_balance --event_id "$EVENT_ID2" | tr -d '"')" \
    "event 2 balance is exactly the one un-refundable redeemed ticket"

SUMMARY2=$(call get_event_summary --event_id "$EVENT_ID2")
TOTAL_COLLECTED=$(printf '%s' "$SUMMARY2" | jq -r '.total_collected')
TOTAL_PAID_OUT=$(printf '%s' "$SUMMARY2" | jq -r '.total_paid_out')
REFUNDED_TOTAL=$(printf '%s' "$SUMMARY2" | jq -r '.refunded_total')
BALANCE2=$(printf '%s' "$SUMMARY2" | jq -r '.balance')
assert_eq "$TOTAL_COLLECTED" "$((TOTAL_PAID_OUT + REFUNDED_TOTAL + BALANCE2))" \
    "get_event_summary's invariant holds after cancellation and refunds"

# ─── Summary ──────────────────────────────────────────────────────────────────

printf '\n%s%s%s\n' "$BOLD" "────────────────────────────────────────────────────────" "$RESET"
printf '%s✓ end-to-end lifecycle passed%s — %d checks' "$GREEN$BOLD" "$RESET" "$CHECKS_PASSED"
if (( CHECKS_SKIPPED > 0 )); then
    printf ', %d skipped' "$CHECKS_SKIPPED"
fi
printf '\n  network:  %s\n  contract: %s\n\n' "$NETWORK" "$CONTRACT_ID"
