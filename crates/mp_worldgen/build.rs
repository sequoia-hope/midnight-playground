//! Turns `assets/fonts/fonts.json` into the bundled font table
//! (`fonts::BUNDLED`, `BUNDLED_GENERIC`, `BUNDLED_FALLBACK` and the files'
//! bytes), so the manifest is the one place the mapping is written
//! (DECISIONS D373). A small JSON reader of its own keeps the crate free of
//! build dependencies.

use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Debug)]
enum Json {
    Null,
    Bool(#[allow(dead_code)] bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            v => panic!("fonts.json: expected a string, found {v:?}"),
        }
    }
    fn num(&self) -> f64 {
        match self {
            Json::Num(n) => *n,
            v => panic!("fonts.json: expected a number, found {v:?}"),
        }
    }
    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            v => panic!("fonts.json: expected an array, found {v:?}"),
        }
    }
    fn obj(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(kv) => kv,
            v => panic!("fonts.json: expected an object, found {v:?}"),
        }
    }
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(
            self.s.get(self.i),
            Some(&c),
            "fonts.json: expected '{}' at byte {}",
            c as char,
            self.i
        );
        self.i += 1;
    }
    fn peek(&mut self) -> u8 {
        self.ws();
        *self.s.get(self.i).expect("fonts.json: unexpected end")
    }
    fn value(&mut self) -> Json {
        match self.peek() {
            b'{' => {
                self.eat(b'{');
                let mut kv = Vec::new();
                if self.peek() != b'}' {
                    loop {
                        let k = self.string();
                        self.eat(b':');
                        kv.push((k, self.value()));
                        if self.peek() == b',' {
                            self.eat(b',');
                        } else {
                            break;
                        }
                    }
                }
                self.eat(b'}');
                Json::Obj(kv)
            }
            b'[' => {
                self.eat(b'[');
                let mut a = Vec::new();
                if self.peek() != b']' {
                    loop {
                        a.push(self.value());
                        if self.peek() == b',' {
                            self.eat(b',');
                        } else {
                            break;
                        }
                    }
                }
                self.eat(b']');
                Json::Arr(a)
            }
            b'"' => Json::Str(self.string()),
            _ => {
                let start = self.i;
                while self.i < self.s.len() && !b",]} \t\r\n".contains(&self.s[self.i]) {
                    self.i += 1;
                }
                let tok = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                match tok {
                    "null" => Json::Null,
                    "true" => Json::Bool(true),
                    "false" => Json::Bool(false),
                    _ => Json::Num(
                        tok.parse()
                            .unwrap_or_else(|_| panic!("fonts.json: bad token {tok:?}")),
                    ),
                }
            }
        }
    }
    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut out = Vec::new();
        loop {
            let c = self.s[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
                            self.i += 4;
                            let ch = char::from_u32(u32::from_str_radix(hex, 16).unwrap())
                                .expect("fonts.json: \\u escape outside the BMP");
                            out.extend_from_slice(ch.to_string().as_bytes());
                        }
                        _ => out.push(e),
                    }
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).unwrap()
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/fonts/fonts.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).expect("assets/fonts/fonts.json");
    let json = Reader {
        s: text.as_bytes(),
        i: 0,
    }
    .value();

    let mut out = String::new();
    let mut files: Vec<String> = Vec::new();
    out += "/// The bundled faces: `(family, file, weight range, style)`, as\n";
    out += "/// `assets/fonts/fonts.json` lists them (generated by `build.rs`).\n";
    out += "pub const BUNDLED: &[(&str, &str, (f32, f32), &str)] = &[\n";
    for f in json.get("faces").expect("fonts.json: faces").arr() {
        let file = f.get("file").unwrap().str();
        let w = f.get("weight").unwrap().arr();
        let style = f.get("style").map_or("normal", Json::str);
        assert!(
            style == "normal" || style == "italic",
            "fonts.json: style {style:?}"
        );
        writeln!(
            out,
            "    ({:?}, {:?}, ({:?}, {:?}), {:?}),",
            f.get("family").unwrap().str(),
            file,
            w[0].num() as f32,
            w[1].num() as f32,
            style
        )
        .unwrap();
        if !files.iter().any(|x| x == file) {
            files.push(file.to_string());
        }
    }
    out += "];\n\n/// The generic families, as `fonts.json` maps them.\n";
    out += "pub const BUNDLED_GENERIC: &[(&str, &str)] = &[\n";
    for (g, f) in json.get("generic").map_or(&[][..], Json::obj) {
        writeln!(out, "    ({g:?}, {:?}),", f.str()).unwrap();
    }
    out += "];\n\n/// The families tried for a character no family of the font has,\n";
    out += "/// as `fonts.json` lists them.\n";
    out += "pub const BUNDLED_FALLBACK: &[&str] = &[\n";
    for f in json.get("fallback").map_or(&[][..], Json::arr) {
        writeln!(out, "    {:?},", f.str()).unwrap();
    }
    out += "];\n\n/// Each bundled file once, with its bytes.\n";
    out += "const BUNDLED_FILES: &[(&str, &[u8])] = &[\n";
    for file in &files {
        writeln!(
            out,
            "    ({file:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../assets/fonts/\", {file:?}))),"
        )
        .unwrap();
    }
    out += "];\n";
    let dst = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bundled_fonts.rs");
    std::fs::write(dst, out).unwrap();
}
