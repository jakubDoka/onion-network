wasm-pack build --target web --release --out-name onion_client --out-dir ./pkg
(cd pkg && sed -i 's/Array<any>/Array<Message>/g' onion_client.d.ts)
