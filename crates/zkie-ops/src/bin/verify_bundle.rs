//! B2 independent verifier process.
//!
//! Usage: verify_bundle <bundle-dir>
//! Reads only <bundle-dir>/bundle.bin and <bundle-dir>/proof.bin and exits 0
//! on acceptance, 1 on rejection. No prover state, weights, or store is
//! loaded.

use std::path::Path;

fn main() {
    let dir = match std::env::args().nth(1) {
        Some(d) => d,
        None => {
            eprintln!("usage: verify_bundle <bundle-dir>");
            std::process::exit(2);
        }
    };
    let ok = zkie_ops::bundle::verify_bundle_dir(Path::new(&dir));
    println!("{}", if ok { "ACCEPT" } else { "REJECT" });
    std::process::exit(if ok { 0 } else { 1 });
}
