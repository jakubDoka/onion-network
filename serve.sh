#!/bin/bash

creq() { [ -x "$(command -v $1)" ] || cargo install $1; }
sod() { export "$1"="${!1:-$2}"; }
is_running() { pgrep -f "$1" > /dev/null; }

creq trunk
creq live-server
creq subxt

# args
sod PROFILE         ""
sod RELOAD_TOPOLOGY "false"
sod REBUILD_CHAIN   "false"
sod REBUILD_NATIVE  "false"
sod REBUILD_CLIENT  "false"

# config
sod CHAIN_NODES       "ws://localhost:9944"
sod NODE_COUNT        15
sod IDLE_TIMEOUT      2000
sod FRONTEND_PORT     7777
sod TOPOLOGY_PORT     8888
sod RUST_LOG          "info"
sod RUST_BACKTRACE    1
sod NODE_START        8800
sod NETWORK_BOOT_NODE "/ip4/127.0.0.1/tcp/$((NODE_START + 100))/ws"
sod MIN_NODES         5
sod BALANCE           100000000000000
sod TEST_WALLETS      "5CwfgYUrq24dTpfh2sQ2st1FNCR2fM2JFSn3EtdWyrGdEaER,5E7YrzVdg1ovRYfWLQG1bJV7FvZWJpnVnQ3nVCKEwpFzkX8s,5CveKLTBDy6vFbgE1DwXHwPRcswa1kRSLY8rL3Yx5qUhsSCo"
sod EXPOSED_ADDRESS   "127.0.0.1"

# constst
CHAIN_NAME="node-template"
CHAIN_PATH="chain/substrate-tests/target/release/$CHAIN_NAME"
TOPOLOGY_ROOT="protocols/topology-vis"
WALLET_INTEGRATION="chat/client/wallet-integration"
FALCON_ROOT="crypto/falcon"
TARGET_DIR="target/debug"
if [ "$PROFILE" = "release" ]; then
	FLAGS="--profile native-optimized"
	WASM_FLAGS="--release"
	TARGET_DIR="target/native-optimized"
fi

cleanup_files() {
	rm -rf node_keys node_logs
	mkdir node_keys node_logs
}
generate_falcon() { (cd $FALCON_ROOT && sh transpile.sh || exit 1); }
init_npm() { (cd $WALLET_INTEGRATION && npm i || exit 1); }

rebuild_native() {
		cargo build $FLAGS --workspace \
			--exclude chat-client \
			--exclude chat-client-node \
			--exclude topology-vis \
			|| exit 1
}
rebuild_topology() { (cd $TOPOLOGY_ROOT && ./build.sh "$PROFILE" || exit 1); }
rebuild_chain() { (cd chain/substrate-tests && cargo build --release); }
rebuild_client() { (cd chat/client && trunk build $WASM_FLAGS || exit 1); }

run_wasm() {
	(cd $TOPOLOGY_ROOT/dist && live-server --host localhost --port $TOPOLOGY_PORT &)
	(cd chat/client/dist && live-server --host localhost --port $FRONTEND_PORT &)
}
run_chat_servers() {
	killall chat-server
	for i in $(seq $NODE_COUNT); do
		echo "Starting node $i"

		export PORT=$((NODE_START + i))
		export WS_PORT=$((PORT + 100))
		export NODE_ACCOUNT="//Alice"
		export KEY_PATH="node_keys/node$i.keys"
		export NONCE=$i
		export RPC_TIMEOUT=1000

		$TARGET_DIR/chat-server > "node_logs/node$i.log" 2>&1 &
	done
}
run_chain() {
	killall $CHAIN_NAME
	$CHAIN_PATH --dev > /dev/null 2>&1 &

	sleep 3

	METADATA_FILE="chain/types/metadata.scale"
	test -e $METADATA_FILE || subxt metadata > $METADATA_FILE
	$TARGET_DIR/init-transfer || exit 1
}

test -e $CHAIN_PATH && ! $REBUILD_CHAIN || rebuild_chain

test -d $FALCON_ROOT/falcon              || generate_falcon
test -d $WALLET_INTEGRATION/node_modules || init_npm
is_running $CHAIN_NAME                   || run_chain

test -e $TARGET_DIR/chat-server && ! $REBUILD_SERVERS  || rebuild_native
test -d $TOPOLOGY_ROOT/dist     && ! $REBUILD_TOPOLOGY || rebuild_topology
test -d chat/client/dist        && ! $REBUILD_CLIENT   || rebuild_client

run_chat_servers
run_wasm

while read -r line; do
	case "$line" in
		"exec")
			echo -n "Enter command: "
			read -r cmd
			eval "$cmd"
			;;
		"chat-servers")
			rebuild_native
			run_chat_servers
			;;
		"topology")
			rebuild_topology
			;;
		"client")
			rebuild_client
			;;
		"chain")
			rebuild_chain
			run_chain
			;;
		"killall")
			killall chat-server $CHAIN_NAME live-server
			;;
		"exit")
			exit 0
			;;
		*)
			echo "Unknown command: $line"
			echo "Available commands:"
			echo "        exec: execute command"
			echo "chat-servers: rebuild and restart chat servers"
			echo "    topology: rebuild topology (hotreload)"
			echo "      client: rebuild client (hotreload)"
			echo "       chain: rebuild and restart chain"
			echo "        exit: exit"
			;;
	esac
done
