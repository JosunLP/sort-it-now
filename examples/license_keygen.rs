//! License key generator for sort-it-now (maintainer tool, not part of the shipped binary).
//!
//! ```text
//! cargo run --example license_keygen -- generate [COUNT]   # new keys + their hashes
//! cargo run --example license_keygen -- hash KEY [KEY...]   # hash existing keys (e.g. to revoke)
//! ```
//!
//! `generate` prints a tab-separated `KEY<TAB>HASH` line per key on stdout. Hand the keys to
//! licensees over a private channel and append only the hashes to `src/license/hashes.rs`.
//! Never commit the plain keys.

use sort_it_now::license::{KEY_PREFIX, hash_key, normalize_key};

fn usage() -> ! {
    eprintln!("Usage:\n  license_keygen generate [COUNT]\n  license_keygen hash KEY [KEY...]");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("generate") => {
            let count = match args.get(1) {
                None => 1,
                Some(raw) => raw.parse::<usize>().unwrap_or_else(|_| usage()),
            };
            for _ in 0..count {
                let raw = format!("{KEY_PREFIX}{}", uuid::Uuid::new_v4());
                let key = normalize_key(&raw).expect("generated key must be well-formed");
                println!("{key}\t{}", hash_key(&key));
            }
        }
        Some("hash") if args.len() > 1 => {
            for raw in &args[1..] {
                match normalize_key(raw) {
                    Some(key) => println!("{key}\t{}", hash_key(&key)),
                    None => {
                        eprintln!("❌ Invalid key format: {raw}");
                        std::process::exit(1);
                    }
                }
            }
        }
        _ => usage(),
    }
}
