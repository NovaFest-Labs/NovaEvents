#!/usr/bin/env bash
#
# NovaEvents — automated Stellar testnet deployment & initialization.
#
# Builds the contract WASM, deploys it to the Stellar testnet, initializes it
# with the configured admin and token addresses, and prints the deployed contract ID.
#
# Usage:
#   scripts/deploy-testnet.sh [--source <source-key-or-alias>] [--admin <admin-address>] [--token <token-address>]
#   scripts/deploy-testnet.sh --help
#
set -euo pipefail

# ─── Default Configuration ───────────────────────────────────────────────────

NETWORK="testnet"
SOURCE_KEY="${STELLAR_SOURCE_KEY:-default}"
ADMIN_ADDR="${ADMIN_ADDRESS:-}"
TOKEN_ADDR="${TOKEN_ADDRESS:-CAUJTFVKA5WCN4ZPUDBRDAS3DT5HVKNQTLFT32KDAFVGJRTB7VPRVNRT}" # Default testnet USDC
WASM_PATH="target/wasm32v1-none/release/nova_events.wasm"
SKIP_BUILD=0

# ─── Formatting ──────────────────────────────────────────────────────────────

if [[ -t 1 ]]; then
    BOLD=$'\033[1m'; RED=$'\033[31m'; GREEN=$'\033[32m'
    YELLOW=$'\033[33m'; BLUE=$'\033[34m'; CYAN=$'\033[36m'; RESET=$'\033[0m'
else
    BOLD=""; RED=""; GREEN=""; YELLOW=""; BLUE=""; CYAN=""; RESET=""
fi

step() { printf '\n%s▸ %s%s\n' "$BOLD$BLUE" "$*" "$RESET"; }
info() { printf '  %s\n' "$*"; }
success() { printf '%s✔ %s%s\n' "$BOLD$GREEN" "$*" "$RESET"; }
error() { printf '%s✖ %s%s\n' "$BOLD$RED" "$*" "$RESET" >&2; }

# ─── Parse arguments ─────────────────────────────────────────────────────────

usage() {
    cat << USAGE_EOF
NovaEvents Testnet Deployment Script

Usage:
  scripts/deploy-testnet.sh [OPTIONS]

Options:
  -s, --source <name>     Stellar identity / key name to sign transactions (default: 'default')
  -a, --admin <address>   Admin address for the contract (default: public address of --source)
  -t, --token <address>   USDC token contract address (default: testnet USDC)
  --skip-build            Skip building WASM and deploy existing binary
  -h, --help              Show this help message

Environment variables:
  STELLAR_SOURCE_KEY      Fallback for --source
  ADMIN_ADDRESS           Fallback for --admin
  TOKEN_ADDRESS           Fallback for --token

Example:
  scripts/deploy-testnet.sh --source alice
USAGE_EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -s|--source)
            SOURCE_KEY="$2"; shift 2 ;;
        -a|--admin)
            ADMIN_ADDR="$2"; shift 2 ;;
        -t|--token)
            TOKEN_ADDR="$2"; shift 2 ;;
        --skip-build)
            SKIP_BUILD=1; shift ;;
        -h|--help)
            usage ;;
        *)
            error "Unknown argument: $1"
            usage ;;
    esac
done

# ─── Prerequisites Check ─────────────────────────────────────────────────────

step "Checking prerequisites"
if ! command -v stellar &>/dev/null && ! command -v soroban &>/dev/null; then
    error "Stellar CLI is required but not found in PATH."
    error "Install via: cargo install --locked stellar-cli --features opt"
    exit 1
fi
CLI_BIN="$(command -v stellar || command -v soroban)"
info "Using CLI binary: $CLI_BIN"

# Resolve Admin address if omitted
if [[ -z "$ADMIN_ADDR" ]]; then
    if "$CLI_BIN" keys address "$SOURCE_KEY" &>/dev/null; then
        ADMIN_ADDR="$("$CLI_BIN" keys address "$SOURCE_KEY")"
    else
        error "Could not resolve public address for source key '$SOURCE_KEY'."
        error "Please specify --admin <address> or create the key: $CLI_BIN keys generate --network testnet $SOURCE_KEY"
        exit 1
    fi
fi

info "Network:      $NETWORK"
info "Source Key:   $SOURCE_KEY"
info "Admin Addr:   $ADMIN_ADDR"
info "Token Addr:   $TOKEN_ADDR"

# ─── 1. Build WASM ────────────────────────────────────────────────────────────

if [[ $SKIP_BUILD -eq 0 ]]; then
    step "Building contract WASM (wasm32v1-none)"
    cargo build --target wasm32v1-none --release
    success "Build complete: $WASM_PATH"
else
    step "Skipping build (using existing $WASM_PATH)"
fi

if [[ ! -f "$WASM_PATH" ]]; then
    error "Contract WASM file not found at $WASM_PATH"
    exit 1
fi

# ─── 2. Deploy Contract ───────────────────────────────────────────────────────

step "Deploying NovaEvents to Stellar testnet"
CONTRACT_ID="$("$CLI_BIN" contract deploy \
    --wasm "$WASM_PATH" \
    --network "$NETWORK" \
    --source "$SOURCE_KEY")"

success "Contract deployed successfully!"

# ─── 3. Initialize Contract ───────────────────────────────────────────────────

step "Initializing contract (admin: $ADMIN_ADDR, token: $TOKEN_ADDR)"
"$CLI_BIN" contract invoke \
    --id "$CONTRACT_ID" \
    --network "$NETWORK" \
    --source "$SOURCE_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDR" \
    --token "$TOKEN_ADDR"

success "Contract initialized successfully!"

# ─── 4. Output Summary ────────────────────────────────────────────────────────

printf '\n%s==========================================================%s\n' "$BOLD$CYAN" "$RESET"
printf '%s🎉 NovaEvents Deployment Summary%s\n' "$BOLD$GREEN" "$RESET"
printf '%s==========================================================%s\n' "$BOLD$CYAN" "$RESET"
printf '  %sNetwork:%s       %s\n' "$BOLD" "$RESET" "$NETWORK"
printf '  %sContract ID:%s   %s%s%s\n' "$BOLD" "$RESET" "$BOLD$YELLOW" "$CONTRACT_ID" "$RESET"
printf '  %sAdmin Address:%s %s\n' "$BOLD" "$RESET" "$ADMIN_ADDR"
printf '  %sToken Address:%s %s\n' "$BOLD" "$RESET" "$TOKEN_ADDR"
printf '%s==========================================================%s\n\n' "$BOLD$CYAN" "$RESET"
