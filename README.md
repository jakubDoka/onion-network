# Orion network

## Setup

```sh
git clone --recursive '<this-repo>'
cd orion-network
./run.sh
```

## Build Chat Server

```sh
cargo build --release --bin chat-server
mv target/release/chat-server .
```
