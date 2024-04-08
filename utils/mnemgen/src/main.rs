fn main() {
    let mnemonic = bip39::Mnemonic::generate(24).unwrap();
    println!("{}", mnemonic.to_string());
}
