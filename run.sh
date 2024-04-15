#!/bin/bash

creq() { command -v $1 > /dev/null || cargo install $1; }
local_creq() { command -v $(basename $1) > /dev/null || cargo install --path $1; }
sod() { export "$1"="${!1:-$2}"; }
is_running() { pgrep -f "$1" > /dev/null; }
ensure_dir() { test -d $1 || mkdir -p $1; }

ensure_dir tmp
echo 42069 > tmp/port
echo 0 > tmp/nonce

alloc_port() {
	PORT=$(cat tmp/port)
	PORT=$((PORT + 1))
	echo $PORT > tmp/port
	echo $((PORT - 1))
}

alloc_nonce() {
	NONCE=$(cat tmp/nonce)
	NONCE=$((NONCE + 1))
	echo $NONCE > tmp/nonce
	echo $((NONCE - 1))
}

creq trunk
creq live-server
creq subxt

local_creq utils/mnemgen

# args
sod PROFILE          ""
sod REBUILD_TOPOLOGY "false"
sod REBUILD_CHAIN    "false"
sod REBUILD_NATIVE   "false"
sod REBUILD_CLIENT   "false"

# config
sod CHAIN_NODES        "ws://localhost:9944"
sod NODE_COUNT         15
sod SATELITE_COUNT     3
sod STORAGE_NODE_COUNT 20
sod IDLE_TIMEOUT       2000
sod FRONTEND_PORT      7777
sod TOPOLOGY_PORT      8888
sod RUST_LOG           "info"
sod RUST_BACKTRACE     1
sod MIN_NODES          5
sod BALANCE            100000000000000
sod TEST_WALLETS       "5CwfgYUrq24dTpfh2sQ2st1FNCR2fM2JFSn3EtdWyrGdEaER,5E7YrzVdg1ovRYfWLQG1bJV7FvZWJpnVnQ3nVCKEwpFzkX8s,5CveKLTBDy6vFbgE1DwXHwPRcswa1kRSLY8rL3Yx5qUhsSCo"
sod EXPOSED_ADDRESS    "127.0.0.1"
sod RPC_TIMEOUT        1000
sod NODE_ACCOUNT       "//Alice"
sod STORAGE_SIZE_GB    1

# constst
TMP_DIR="tmp"
CHAIN_NAME="node-template"
CHAIN_PATH="chain/substrate-tests/target/release/$CHAIN_NAME"
TOPOLOGY_ROOT="protocols/topology-vis"
WALLET_INTEGRATION="chat/client/wallet-integration"
FALCON_ROOT="crypto/falcon"
TARGET_DIR="target/debug"
CLIENT_ROOT="chat/client"
SILENCE=""
if [ "$PROFILE" = "release" ]; then
	FLAGS="--profile native-optimized"
	WASM_FLAGS="--release"
	TARGET_DIR="target/native-optimized"
fi

load_mnemonic() {
	ensure_dir $TMP_DIR/node_mnemonics
	FILE_NAME="$TMP_DIR/node_mnemonics/$1.mnem"
	test -f $FILE_NAME || mnemgen > $FILE_NAME
	echo $(cat $FILE_NAME)
}

cleanup_files() {
	rm -rf tmp
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
rebuild_client() { (cd chat/client && trunk build $WASM_FLAGS --features building || exit 1); }

rebuild_all() {
	rebuild_native
	rebuild_topology
	rebuild_client
}

run_wasm() {
	killall live-server
	(cd $TOPOLOGY_ROOT/dist && live-server --host 127.0.0.1 --port $TOPOLOGY_PORT > /dev/null 2>&1 &)
	(cd $CLIENT_ROOT/dist && live-server --host 127.0.0.1 --port $FRONTEND_PORT > /dev/null 2>&1 &)
}
run_nodes() {
	EXE=$1
	COUNT=$2

	killall $EXE
	ensure_dir $TMP_DIR/logs/$EXE
	ensure_dir $TMP_DIR/storage/$EXE

	for i in $(seq $COUNT); do
		echo "Starting node $EXE-$i"
		export PORT=$(alloc_port)
		export WS_PORT=$(alloc_port)
		export MNEMONIC=$(load_mnemonic $EXE-$i)
		export NONCE=$(alloc_nonce)
		echo "PORT=$PORT WS_PORT=$WS_PORT"
		export STORAGE_DIR="$TMP_DIR/storage/$EXE-$i"
		$TARGET_DIR/$EXE > "$TMP_DIR/logs/$EXE/$i.log" 2>&1 &
	done
}
run_chat_servers() { run_nodes chat-server $NODE_COUNT; }
run_satelites() { run_nodes storage-satelite $SATELITE_COUNT; }
run_storage_nodes() { run_nodes storage-node $STORAGE_NODE_COUNT; }
run_chain() {
	killall $CHAIN_NAME
	$CHAIN_PATH --dev > /dev/null 2>&1 &

	sleep 3

	METADATA_FILE="chain/types/metadata.scale"
	subxt metadata > $METADATA_FILE
	$TARGET_DIR/init-transfer || exit 1 &
}

test -e $CHAIN_PATH && ! $REBUILD_CHAIN || rebuild_chain

test -d $FALCON_ROOT/falcon              || generate_falcon
test -d $WALLET_INTEGRATION/node_modules || init_npm

is_running $CHAIN_NAME || run_chain

test -e $TARGET_DIR/chat-server && ! $REBUILD_NATIVE   || rebuild_native
test -d $TOPOLOGY_ROOT/dist     && ! $REBUILD_TOPOLOGY || rebuild_topology
test -d $CLIENT_ROOT/dist       && ! $REBUILD_CLIENT   || rebuild_client

is_running chat-server      || run_chat_servers
#is_running storage-satelite || run_satelites
#is_running storage-node     || run_storage_nodes
is_running live-server      || run_wasm

echo "begins shell from the scope of this script:"
while read -p '$ ' -r line; do $line; done
