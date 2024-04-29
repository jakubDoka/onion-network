#!/usr/bin/env bash

creq() { [ -x "$(command -v $1)" ] || cargo install $1; }

creq wasm-bindgen-cli

sod() { export "$1"="${!1:-$2}"; }

sod BOOT_NODE "/ip4/127.0.0.1/tcp/8900/ws"

OUT_DIR="$2"

TARGET_DIR="debug"
if [ "$1" = "release" ]; then
    FLAGS='--release'
    RELEASE=yes
    TARGET_DIR="release"
fi


export PROJECT_NAME="topology-vis"
export WASM_PATH="../../target/wasm32-unknown-unknown/$TARGET_DIR/$PROJECT_NAME.wasm"
export HTML=$(cat <<- END
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>${PROJECT_NAME}</title>
    <style>
        html,
        body,
        canvas {
            margin: 0px;
            padding: 0px;
            width: 100%;
            height: 100%;
            overflow: hidden;
            position: absolute;
            z-index: 0;
        }
    </style>
</head>
<body style="margin: 0; padding: 0; height: 100vh; width: 100vw;">
    <canvas id="glcanvas" tabindex='1' hidden></canvas>
    <script src="https://not-fl3.github.io/miniquad-samples/mq_js_bundle.js"></script>
    <script type="module">
        import init, { set_wasm } from "./${PROJECT_NAME}.js";
        async function impl_run() {
            let wbg = await init();
            miniquad_add_plugin({
                register_plugin: (a) => (a.wbg = wbg),
                on_init: () => set_wasm(wasm_exports),
                version: "0.0.1",
                name: "wbg",
            });
            load("./${PROJECT_NAME}_bg.wasm");
        }
        window.run = function() {
            document.getElementById("run-container").remove();
            document.getElementById("glcanvas").removeAttribute("hidden");
            document.getElementById("glcanvas").focus();
            impl_run();
        }
    </script>
    <div id="run-container" style="display: flex; justify-content: center; align-items: center; height: 100%; flex-direction: column;">
        <p>Game can't play audio unless a button has been clicked.</p>
        <button onclick="run()">Run Game</button>
    </div>
</body>
</html>
END
)


mkdir -p $OUT_DIR
cargo build --target wasm32-unknown-unknown $FLAGS --features building
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
