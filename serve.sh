#!/bin/bash

creq() { [ -x "$(command -v $1)" ] || cargo install $1; }

creq trunk
creq cargo-contract
creq live-server
creq subxt

sod() { export "$1"="${!1:-$2}"; }

sod CHAIN_NODES "ws://localhost:9944"
sod NODE_COUNT 15
sod IDLE_TIMEOUT 2000
sod FRONTEND_PORT 7777
sod TOPOLOGY_PORT 8888
sod RUST_LOG "info"
sod RUST_BACKTRACE 1
sod NODE_START 8800
sod NETWORK_BOOT_NODE "/ip4/127.0.0.1/tcp/$((NODE_START + 100))/ws"
sod MIN_NODES 5
sod BALANCE 100000000000000
sod TEST_WALLETS 5CwfgYUrq24dTpfh2sQ2st1FNCR2fM2JFSn3EtdWyrGdEaER,5E7YrzVdg1ovRYfWLQG1bJV7FvZWJpnVnQ3nVCKEwpFzkX8s,5CveKLTBDy6vFbgE1DwXHwPRcswa1kRSLY8rL3Yx5qUhsSCo
sod EXPOSED_ADDRESS 127.0.0.1

TARGET_DIR="target/debug"
if [ "$1" = "release" ]; then
  FLAGS="--profile native-optimized"
  WASM_FLAGS="--release"
  TARGET_DIR="target/native-optimized"
fi

on_exit() { killall node-template chat-server runner trunk live-server; }
trap on_exit EXIT

rm -rf node_keys node_logs
mkdir node_keys node_logs

# build
rebuild_workspace() {
	cargo build $FLAGS --workspace \
		--exclude chat-client \
		--exclude chat-client-node \
		--exclude topology-vis \
		|| exit 1
}

test -d crypto/falcon/falcon || (cd crypto/falcon && sh transpile.sh || exit 1)


CHAIN_PATH="chain/substrate-tests/target/release/node-template"
test -e $CHAIN_PATH || (cd chain/substrate-tests && cargo build --release)
$CHAIN_PATH --dev > /dev/null 2>&1 &
sleep 2
METADATA_FILE="chain/types/metadata.scale"
test -e $METADATA_FILE || subxt metadata > $METADATA_FILE

(cd chat/client/wallet-integration && npm i || exit 1)
rebuild_workspace

$TARGET_DIR/init-transfer || exit 1

# run
run_miners() { $TARGET_DIR/runner --node-count $NODE_COUNT --first-port $NODE_START --miner $TARGET_DIR/chat-server $1 & }


(cd protocols/topology-vis && ./build.sh "$1" || exit 1)
(cd protocols/topology-vis/dist && live-server --host localhost --port $TOPOLOGY_PORT &)
(cd chat/client && trunk serve $WASM_FLAGS --port $FRONTEND_PORT --features building &)
run_miners --first-run

while read -r line; do
	case "$line" in
		"chat-servers")
			killall runner chat-server
			rebuild_workspace
			run_miners
			;;
		"topology")
			(cd protocols/topology-vis && ./build.sh "$1" || exit 1)
			;;
		"exit")
			exit 0
			;;
	esac
done
