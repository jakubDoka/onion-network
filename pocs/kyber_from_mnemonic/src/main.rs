use std::str::FromStr;

fn main() {
    let mnemonic = std::env::args().nth(1).unwrap();
    let mnemonic = bip39::Mnemonic::from_str(&mnemonic).unwrap();
    let seed = mnemonic.to_seed("some password");
    let kp = kyber::Keypair::new(&seed);
    println!(
        "Public key: {:?}",
        kp.publickey().to_bytes().map(|x| format!("{:02x}", x)).into_iter().collect::<String>()
    );
}
