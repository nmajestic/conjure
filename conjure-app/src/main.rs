fn main() {
    let app = rune::App::new(conjure_app::spec());
    let handle = app.handle();
    let tx = conjure_app::r#gen::spawn(handle.clone());
    let initial = tx.clone();
    handle.update(move |s, _| s.gen_tx = Some(initial));
    let _ = tx.send(conjure_app::r#gen::GenCommand::Discover);
    app.run();
}
