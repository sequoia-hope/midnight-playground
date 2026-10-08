//! The kit table against the lab: the canonical JSON of the whole `KITS`
//! table, hashed, is the lab's (`node tools/music-lab/test/dump-tables.mjs`
//! prints it).

use mp_music::json::{Val, canon};
use mp_music::kits::{KITS, KITS_SHA256};
use sha2::{Digest, Sha256};

#[test]
fn the_table_is_the_labs() {
    let obj = Val::Obj(
        KITS.iter()
            .map(|(kit, lanes)| {
                (
                    (*kit).to_owned(),
                    Val::Obj(
                        lanes
                            .iter()
                            .map(|(lane, v)| ((*lane).to_owned(), v.to_val()))
                            .collect(),
                    ),
                )
            })
            .collect(),
    );
    let text = canon(&obj);
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    assert_eq!(hash, KITS_SHA256, "{text}");
}
