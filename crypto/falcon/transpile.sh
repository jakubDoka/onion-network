creq() { [ -x "$(command -v $2)" ] || cargo install $1; }

compiledb() {
	FILENAME=$(echo "$1" | rev | cut -d" " -f1 | rev)
	DIRECTORY=$(pwd)
	ARGUMENTS=$(echo "$1" | sed -E "s/ +/\",\"/g")
	echo -n "{\"file\":\"$FILENAME\",\"directory\":\"$DIRECTORY\",\"arguments\":[\"$ARGUMENTS\"]}"
}
export -f compiledb

patch_shake() {
	sed -i 's/pub ctx: \*mut uint64_t/pub ctx: crate::shake::Ctx/g' $1
	sed -i 's/ctx: 0 as \*mut uint64_t/ctx: crate::shake::Ctx { uninit: (), }/g' $1
	sed -i 's/Copy, Clone//g' $1
}
export -f patch_shake

[ -x "$(command -v $2)" ] || cargo install --git https://github.com/immunant/c2rust c2rust
creq ripgrep rg

RESOURCES=$(pwd)
rm -rf falcon
mkdir falcon
cd falcon
ALG_PATH=./PQClean/crypto_sign/falcon-512/clean
PROOT=$(pwd)
CDB=$PROOT/compile_commands.json
CMAKE_EXPORT_COMPILE_COMMANDS=1

RM_STAR_SILENT=true
rm -rf src lib.rs build.rs Cargo.toml Cargo.lock rust-toolchain.toml
test -d PQClean || git clone https://github.com/PQClean/PQClean.git

(cd $ALG_PATH \
	&& make --dry-run | grep ^cc | xargs -I{} sh -c 'compiledb "{}"' \
	|  sed "s/}{/},{/g" | sed "s/^{/[{/" | sed "s/}$/}]/" > $CDB \
	&& c2rust transpile -e --emit-no-std -o $PROOT $CDB -- -I/usr/lib/clang/16/include \
	|| rm $CDB && exit 1)

rm rust-toolchain.toml $CDB build.rs

echo -e "#![allow(clippy::all)]#![allow(warnings)]#![no_std]$(cat lib.rs)" > lib.rs
cat $RESOURCES/injected_code.rs >> lib.rs

rg --files-with-matches 'use ::libc' |\
	rg -v 'transpile.sh' |\
	xargs -l1 sed -i 's/use ::libc/use crate::libc/g'
rg --files-with-matches 'shake256' |\
	rg -v 'transpile.sh' |\
	xargs -I{} sh -c 'patch_shake "{}"' {}

RANDOM_BYTES_USERS="fn \(PQCLEAN_FALCON512_CLEAN_crypto_sign\(_signature\|_keypair\|\)\|do_sign\)("
RANDOM_BYTES_SIGNATURE='impl FnMut(*mut uint8_t, size_t) -> libc::c_int,'
RANDOM_BYTES_USERS_REPLACEMENT="&mut randombytes: $RANDOM_BYTES_SIGNATURE"
sed -i "s/$RANDOM_BYTES_USERS/$RANDOM_BYTES_USERS_REPLACEMENT/g" src/pqclean.rs
sed -i "s/PQCLEAN_randombytes/randombytes/g" src/pqclean.rs
sed -i "s/pub unsafe extern \"C\" fn/pub unsafe fn/g" src/pqclean.rs
sed -i "s/#\[no_mangle]//g" src/pqclean.rs
sed -i "s/if do_sign(/&randombytes,/g" src/pqclean.rs

sed -i "s/extern crate libc;//" lib.rs

sed -i 's/\(memset\|memmove\|memcpy\)/rust_\1/g' src/*.rs
sed -i 's/pub type uint64_t =.*//g' src/*.rs
sed -i 's/uint64_t/u64/g' src/*.rs
sed -i 's/pub type int64_t =.*//g' src/*.rs
sed -i 's/int64_t/i64/g' src/*.rs
sed -i 's/pub type uint32_t =.*//g' src/*.rs
sed -i 's/uint32_t/u32/g' src/*.rs
sed -i -E 's/([[:digit:]]+|0x[0-9a-f]+) as libc::(c_ulong|c_long|c_int|fpr)/\1/g' src/*.rs
sed -i 's/-(1) as u32/u32::MAX/g' src/*.rs

sed -i "s/\[workspace\]//" Cargo.toml
sed -i "s/members = \[//" Cargo.toml
sed -i "s/^\]$//" Cargo.toml
sed -i 's/crate-type = \["staticlib", "rlib"]//' Cargo.toml

cargo add sha3 --no-default-features
cargo add rand_core --no-default-features
cargo remove libc
cargo build --target wasm32-unknown-unknown
cargo test --release
