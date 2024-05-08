#!/bin/bash

creq() { command -v $1 > /dev/null || cargo install $1; }
sod() { export "$1"="${!1:-$2}"; }
is_running() { pgrep -f "$1" > /dev/null; }
ensure_dir() { test -d $1 || mkdir -p $1; }


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
sod CLIENT_PORT        7777
sod TOPOLOGY_PORT      8888
sod RUST_LOG           "info"
sod RUST_BACKTRACE     1
sod MIN_NODES          5
sod BALANCE            100000000000000
sod TEST_WALLETS       "5CwfgYUrq24dTpfh2sQ2st1FNCR2fM2JFSn3EtdWyrGdEaER 5E7YrzVdg1ovRYfWLQG1bJV7FvZWJpnVnQ3nVCKEwpFzkX8s 5CveKLTBDy6vFbgE1DwXHwPRcswa1kRSLY8rL3Yx5qUhsSCo"
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
TOPOLOGY_DIST="$(pwd)/target/dist/topology"
CLIENT_DIST="$(pwd)/target/dist/client"
SILENCE=""
if [ "$PROFILE" = "release" ]; then
	FLAGS="--profile native-optimized"
	WASM_FLAGS="--release"
	TARGET_DIR="target/native-optimized"
fi

ensure_dir $TMP_DIR

reset_state() { # reset the port and nonce counters
	echo 42069 > $TMP_DIR/port
	echo 0 > $TMP_DIR/nonce
}

alloc_port() { # allocate a port for a node
	PORT=$(cat $TMP_DIR/port)
	PORT=$((PORT + 1))
	echo $PORT > $TMP_DIR/port
	echo $((PORT - 1))
}

alloc_nonce() { # allocate a nonce for a node
	NONCE=$(cat $TMP_DIR/nonce)
	NONCE=$((NONCE + 1))
	echo $NONCE > $TMP_DIR/nonce
	echo $((NONCE - 1))
}


load_mnemonic() { # <name> - load or generate a mnemonic for a node
	ensure_dir $TMP_DIR/node_mnemonics
	FILE_NAME="$TMP_DIR/node_mnemonics/$1.mnem"
	test -f $FILE_NAME || $TARGET_DIR/chain-helper gen-mnemonic 24 > $FILE_NAME
	echo $(cat $FILE_NAME)
}

cleanup_files() { rm -rf $TMP_DIR; }
generate_falcon() { (cd $FALCON_ROOT && sh transpile.sh || exit 1); }

build_native() { # rebuild the native part of the project (all executables)
		cargo build $FLAGS --workspace \
			--exclude chat-client \
			--exclude chat-client-node \
			--exclude topology-vis \
			--exclude websocket-websys \
			|| exit 1
}
build_topology() { # rebuild the topology, if `run_wasm` is called, it will trigger hotreload
	creq trunk
	ensure_dir $TOPOLOGY_DIST
	(cd $TOPOLOGY_ROOT && ./build.sh "$PROFILE" "$TOPOLOGY_DIST" || exit 1)
}
build_chain() { (cd chain/substrate-tests && cargo build --release); }
build_client() { # rebuild the client, if `run_wasm` is called, it will trigger hotreload
	creq trunk
	ensure_dir $CLIENT_DIST
	(cd chat/client && trunk build $WASM_FLAGS -d $CLIENT_DIST || exit 1)
}
build_all() { build_native; build_client; }

run_wasm() { # run the wasm frontends (topology and client)
	creq live-server
	killall live-server
	(cd $TOPOLOGY_DIST && live-server --host 127.0.0.1 --port $TOPOLOGY_PORT > /dev/null 2>&1 &)
	(cd $CLIENT_DIST && live-server --host 127.0.0.1 --port $CLIENT_PORT > /dev/null 2>&1 &)
}
run_nodes() { # <exe> <count> - run nodes of a certain type
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
		export STORAGE_DIR="$TMP_DIR/storage/$EXE-$i"

		IDENTITY=$($TARGET_DIR/chain-helper export-identity "$MNEMONIC" | jq -r '.sign')
		ENC_HAHS=$($TARGET_DIR/chain-helper export-identity "$MNEMONIC" | jq -r '.enc')
		$TARGET_DIR/chain-helper register-node //Alice $IDENTITY $ENC_HAHS 127.0.0.1:$PORT $CHAIN_NODES \
			&& $TARGET_DIR/$EXE $PORT $WS_PORT $IDLE_TIMEOUT $RPC_TIMEOUT $STORAGE_DIR "$MNEMONIC" \
				$CHAIN_NODES > "$TMP_DIR/logs/$EXE/$i.log" 2>&1 &
	done
}
run_chat_servers() { run_nodes chat-server $NODE_COUNT; }
run_servers() { reset_state; run_chat_servers; }
run_chain() { # run local chain node
	killall $CHAIN_NAME
	$CHAIN_PATH --dev > /dev/null 2>&1 &

	sleep 3

	METADATA_FILE="chain/types/metadata.scale"
	creq subxt
	subxt metadata > $METADATA_FILE
	$TARGET_DIR/chain-helper bulk-transfer $BALANCE $CHAIN_NODES //Bob $TEST_WALLETS || exit 1 &
}

repl() { # make a repl inside this script
	echo "begins shell from the scope of this script:"
	while read -p '$ ' -r line; do $line; done
}

boot_local() { # boot local chain nodes and frontends for testing
	test -e $CHAIN_PATH && ! $REBUILD_CHAIN || build_chain

	test -d $FALCON_ROOT/falcon              || generate_falcon

	is_running $CHAIN_NAME || run_chain

	test -e $TARGET_DIR/chat-server && ! $REBUILD_NATIVE   || build_native
	test -d $TOPOLOGY_DIST     && ! $REBUILD_TOPOLOGY || build_topology
	test -d $CLIENT_DIST       && ! $REBUILD_CLIENT   || build_client

	reset_state

	is_running chat-server      || run_chat_servers
	#is_running storage-satelite || run_satelites
	#is_running storage-node     || run_storage_nodes
	is_running live-server      || run_wasm

	repl
}

boot_born() { # boot frontend to connect to born deployment
	export CHAIN_NODES=wss://rpc-1.born.orionmessenger.io
	build_client
	build_topology
	is_running live-server      || run_wasm

	repl
}

help() { # print help
	echo "Usage: run.sh <command> [args...]"
	echo "when using repl, its recommended to call this with 'rlwrap'"
	echo "Commands:"
	grep -E '^[a-z_]+?\(\) \{' $0 | sed 's/() { #/ -/' | sort | sed 's/^/  /'
}

$@ || help
