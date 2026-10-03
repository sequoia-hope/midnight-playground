//! The native client: `cargo run -p mr_game -- --level coast --query "s=300&v=30"`.
//! (On the web the library is the entry point; this binary is native only.)

#[cfg(not(target_arch = "wasm32"))]
fn main() -> bevy::app::AppExit {
    mr_game::native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
