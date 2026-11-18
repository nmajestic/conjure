pub mod r#gen;
pub mod pixels;
pub mod state;
pub mod store_io;
pub mod ui;

pub use state::AppState;

pub fn spec() -> rune::AppSpec<AppState> {
    rune::AppSpec::new("Conjure", (1100.0, 760.0), AppState::init, ui::view)
}
