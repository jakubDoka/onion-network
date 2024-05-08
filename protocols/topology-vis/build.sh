#!/usr/bin/env bash

creq() { [ -x "$(command -v $1)" ] || cargo install $1; }
creq wasm-bindgen-cli
sod() { export "$1"="${!1:-$2}"; }

OUT_DIR="$2"

TARGET_DIR="debug"
if [ "$1" = "release" ]; then
    FLAGS='--release'
    RELEASE=yes
    TARGET_DIR="release"
fi


export PROJECT_NAME="topology-vis"
export WASM_PATH="../../target/wasm32-unknown-unknown/$TARGET_DIR/$PROJECT_NAME.wasm"
export HTML=$(cat index.html | sed "s/{{PROJECT_NAME}}/$PROJECT_NAME/g")


mkdir -p $OUT_DIR
cargo build --target wasm32-unknown-unknown $FLAGS
wasm-bindgen "$WASM_PATH" --out-dir $OUT_DIR --target web --no-typescript
if [ "$RELEASE" = "yes" ]; then
    creq wasm-opt
    wasm-opt -Oz -o $OUT_DIR/topology-vis_bg.wasm $OUT_DIR/topology-vis_bg.wasm 
fi

patch() { sed -i "$1" "$OUT_DIR/$PROJECT_NAME.js";}

patch "s/import \* as __wbg_star0 from 'env';//"
patch "s/let wasm;/let wasm; export const set_wasm = (w) => wasm = w;/"
patch "s/imports\['env'\] = __wbg_star0;/return imports.wbg\;/"
patch "s/const imports = __wbg_get_imports();/return __wbg_get_imports();/"

echo "$HTML" > $OUT_DIR/index.html
