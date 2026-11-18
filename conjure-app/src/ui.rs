//! The app's view: the NoLibrary screen, the Browse screen (library list, preview,
//! export panel) and the gen cockpit, all driven from [`crate::state::AppState`].

use conjure_core::style::Dimensions;
use conjure_core::{AssetKind, AssetRequest};
use rune::tok::{self, Theme, radius, type_};
use rune::{
    Color, Cx, El, FileDialogOptions, ScrollSpec, SurfaceFit, col, el, row, text, text_input,
};

use crate::r#gen::GenCommand;
use crate::state::{AppState, RunState, Screen};

/// The dark theme every screen draws on.
fn theme() -> Theme {
    tok::theme(true)
}

/// A press control: keyed, named for assistive technology, focusable.
fn button(
    key: String,
    label: String,
    active: bool,
    checked: bool,
    t: Theme,
    on_press: impl Fn(&mut AppState) + 'static,
) -> El {
    text(label.clone())
        .font(type_::LABEL)
        .color(if active { t.bg } else { t.text })
        .key(key)
        .role("button")
        .label(label)
        .checked(checked)
        .focusable(true)
        .px(10.0)
        .py(5.0)
        .radius(radius::SM)
        .fill(if active {
            tok::accent(0, true)
        } else {
            t.control_track
        })
        .on_press(on_press)
}

/// The first eight hex digits of a hash, the form every list key uses.
fn hash8(hash: &str) -> &str {
    &hash[..hash.len().min(8)]
}

/// Exact sRGB bytes to a linear [`Color`], through rune's documented hex form.
fn srgb(rgba: [u8; 4]) -> Color {
    let [r, g, b, a] = rgba;
    rune::color::hex(&format!("#{r:02x}{g:02x}{b:02x}{a:02x}")).unwrap_or(Color::TRANSPARENT)
}

/// Step a dimension within `1..=max`.
fn step_u32(value: u32, delta: i32, max: u32) -> u32 {
    (value as i32 + delta).clamp(1, max as i32) as u32
}

/// The one stepper pair adjusts whichever fields the active kind owns.
fn step_kind(delta: i32) -> impl Fn(&mut AppState) {
    move |s: &mut AppState| {
        let g = &mut s.r#gen;
        if g.is_sprite {
            g.sprite_w = step_u32(g.sprite_w, delta, 128);
            g.sprite_h = step_u32(g.sprite_h, delta, 128);
        } else {
            g.tile_w = step_u32(g.tile_w, delta, 128);
            g.tile_h = step_u32(g.tile_h, delta, 128);
            g.tile_count = step_u32(g.tile_count, delta, 32);
        }
    }
}

pub fn view(s: &AppState, _cx: &mut Cx<AppState>) -> El {
    let t = theme();
    match s.screen {
        Screen::NoLibrary => no_library(s, t),
        Screen::Browse => browse(s, t),
    }
}

/// No store in the working directory: explain, and offer to create one.
fn no_library(s: &AppState, t: Theme) -> El {
    let mut screen = col()
        .key("no-library")
        .w_full()
        .h_full()
        .center()
        .gap(12.0)
        .fill(t.bg)
        .child(
            text("No Conjure library found")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text("no .conjure/ found (run conjure init)")
                .font(type_::BODY)
                .color(t.text_secondary),
        )
        .child(button(
            "initialize".to_string(),
            "initialize library".to_string(),
            false,
            false,
            t,
            |s: &mut AppState| crate::store_io::initialize(s),
        ));
    if let Some(notice) = &s.notice {
        screen = screen.child(
            text(notice.clone())
                .font(type_::MONO_CAPTION)
                .color(t.text_secondary),
        );
    }
    screen
}

/// The main window: the library list on the left, workbench on the right.
fn browse(s: &AppState, t: Theme) -> El {
    let mut workbench = col()
        .key("workbench")
        .grow(1.0)
        .min_h(0.0)
        .px(12.0)
        .py(10.0)
        .gap(12.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(preview_pane(s, t));
    if !s.selection.is_empty() {
        workbench = workbench.child(export_panel(s, t));
    }
    workbench = workbench.child(gen_cockpit(s, t));
    row()
        .key("browse")
        .w_full()
        .h_full()
        .fill(t.bg)
        .child(library_panel(s, t))
        .child(workbench)
}

/// Cards, then (once one is chosen) the assets, in a scrolling column.
fn library_panel(s: &AppState, t: Theme) -> El {
    let mut panel = col()
        .key("library")
        .w(320.0)
        .h_full()
        .px(8.0)
        .py(8.0)
        .gap(4.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(text("Cards").font(type_::SECTION).color(t.text_secondary));
    for card in &s.cards {
        let hash8 = hash8(&card.hash).to_string();
        let card_hash = card.hash.clone();
        let active = s.selected_card.as_deref() == Some(card.hash.as_str());
        panel = panel.child(button(
            format!("browse-card-{hash8}"),
            format!("{} {} {hash8}", card.name, card.id),
            active,
            active,
            t,
            move |s: &mut AppState| crate::store_io::select_card(s, &card_hash),
        ));
    }
    if s.selected_card.is_some() {
        panel = panel.child(text("Assets").font(type_::SECTION).color(t.text_secondary));
        for asset in &s.assets {
            let hash8 = hash8(&asset.hash).to_string();
            let asset_hash = asset.hash.clone();
            let selected = s.selection.iter().any(|hash| hash == &asset.hash);
            let mut row_el = row()
                .key(format!("browse-asset-{hash8}"))
                .role("row")
                .label(format!(
                    "{} {} {}x{}",
                    asset.name, asset.kind, asset.w, asset.h
                ))
                .checked(selected)
                .focusable(true)
                .gap(8.0)
                .px(6.0)
                .py(4.0)
                .radius(radius::SM)
                .fill(if selected {
                    tok::accent(0, true)
                } else {
                    t.card
                })
                .on_press(move |s: &mut AppState| crate::store_io::select(s, &asset_hash))
                .child(
                    text(format!(
                        "{} {} {}x{}",
                        asset.name, asset.kind, asset.w, asset.h
                    ))
                    .font(type_::LABEL)
                    .color(if selected { t.bg } else { t.text }),
                );
            if !asset.tags.is_empty() {
                row_el = row_el.child(
                    text(asset.tags.join(" "))
                        .font(type_::MONO_CAPTION)
                        .color(if selected { t.bg } else { t.text_secondary }),
                );
            }
            panel = panel.child(row_el);
        }
    }
    panel
}

/// The selected asset's pixels, metadata, palette and zoom controls.
fn preview_pane(s: &AppState, t: Theme) -> El {
    let mut pane = col().key("viewer").gap(8.0);
    let Some(p) = &s.preview else {
        return pane.child(
            text("select an asset")
                .font(type_::BODY)
                .color(t.text_secondary),
        );
    };
    pane = pane
        .child(
            el().key("preview")
                .aspect_fit(p.w as f32 / p.h as f32)
                .max_h(280.0)
                .pixels_fit(&p.pixels, SurfaceFit::Fit),
        )
        .child(
            text(format!("{}x{} at zoom ×{}", p.w, p.h, p.zoom))
                .font(type_::MONO_CAPTION)
                .color(t.text_secondary),
        )
        .child(
            text(s.selection.first().cloned().unwrap_or_default())
                .font(type_::MONO_CAPTION)
                .color(t.text_secondary),
        );
    let mut strip = row().key("swatches").gap(6.0);
    for (i, rgba) in p.palette.iter().enumerate() {
        strip = strip.child(
            el().key(format!("palette-{i}"))
                .size(18.0, 18.0)
                .radius(4.0)
                .fill(srgb(*rgba))
                .border(1.0, t.separator),
        );
    }
    pane = pane.child(strip).child(
        row()
            .gap(8.0)
            .child(button(
                "zoom-out".to_string(),
                "zoom out".to_string(),
                false,
                false,
                t,
                |s: &mut AppState| crate::store_io::zoom(s, -1),
            ))
            .child(button(
                "zoom-in".to_string(),
                "zoom in".to_string(),
                false,
                false,
                t,
                |s: &mut AppState| crate::store_io::zoom(s, 1),
            )),
    );
    pane
}

/// Sheet layout, scale and the destination folder, once an asset is selected.
fn export_panel(s: &AppState, t: Theme) -> El {
    let mut controls = row()
        .gap(8.0)
        .child(button(
            "export-strip".to_string(),
            "strip".to_string(),
            !s.export.grid,
            !s.export.grid,
            t,
            |s: &mut AppState| s.export.grid = false,
        ))
        .child(button(
            "export-grid".to_string(),
            "grid".to_string(),
            s.export.grid,
            s.export.grid,
            t,
            |s: &mut AppState| s.export.grid = true,
        ));
    if s.export.grid {
        controls = controls.child(
            row()
                .key("export-columns")
                .gap(4.0)
                .child(button(
                    "export-cols-down".to_string(),
                    "-".to_string(),
                    false,
                    false,
                    t,
                    |s: &mut AppState| s.export.columns = s.export.columns.saturating_sub(1).max(1),
                ))
                .child(
                    text(s.export.columns.to_string())
                        .font(type_::LABEL)
                        .color(t.text),
                )
                .child(button(
                    "export-cols-up".to_string(),
                    "+".to_string(),
                    false,
                    false,
                    t,
                    |s: &mut AppState| s.export.columns = s.export.columns.saturating_add(1),
                )),
        );
    }
    controls = controls
        .child(button(
            "export-scale".to_string(),
            format!("scale ×{}", s.export.scale),
            false,
            false,
            t,
            |s: &mut AppState| {
                s.export.scale = match s.export.scale {
                    1 => 2,
                    2 => 4,
                    4 => 8,
                    8 => 16,
                    _ => 1,
                };
            },
        ))
        .child(
            text("export")
                .font(type_::LABEL)
                .color(t.bg)
                .key("export-run")
                .role("button")
                .label("export to a folder")
                .focusable(true)
                .px(10.0)
                .py(5.0)
                .radius(radius::SM)
                .fill(tok::accent(0, true))
                .on_press_tx(|_s: &mut AppState, tx: &mut rune::Tx| {
                    let options = FileDialogOptions {
                        title: "Choose an export folder".to_string(),
                        ..Default::default()
                    };
                    tx.open_folder(options, |s: &mut AppState, result, _| match result {
                        Ok(Some(dir)) => crate::store_io::export(s, &dir),
                        Ok(None) => {}
                        Err(e) => s.notice = Some(format!("error: {e}")),
                    });
                }),
        );
    col()
        .key("export")
        .gap(8.0)
        .child(text("Export").font(type_::SECTION).color(t.text_secondary))
        .child(controls)
        .child(
            text(
                s.export
                    .message
                    .clone()
                    .or_else(|| s.notice.clone())
                    .unwrap_or_default(),
            )
            .key("export-result")
            .font(type_::MONO_CAPTION)
            .color(t.text_secondary),
        )
}

/// Provider, model, style card, name and shape: everything a generation needs.
fn gen_cockpit(s: &AppState, t: Theme) -> El {
    let g = &s.r#gen;
    let mut cockpit = col()
        .key("gen")
        .gap(8.0)
        .child(
            text("Generate")
                .font(type_::SECTION)
                .color(t.text_secondary),
        )
        .child(
            row()
                .gap(8.0)
                .child(button(
                    "gen-provider-http".to_string(),
                    "http".to_string(),
                    g.provider == "http",
                    g.provider == "http",
                    t,
                    |s: &mut AppState| s.r#gen.provider = "http".to_string(),
                ))
                .child(button(
                    "gen-provider-opencode".to_string(),
                    "opencode".to_string(),
                    g.provider == "opencode",
                    g.provider == "opencode",
                    t,
                    |s: &mut AppState| {
                        s.r#gen.provider = "opencode".to_string();
                        // provider_rule rejects an opencode start that carries a base_url.
                        s.r#gen.base_url.clear();
                    },
                ))
                .child(button(
                    "gen-refresh-models".to_string(),
                    "refresh models".to_string(),
                    false,
                    false,
                    t,
                    |s: &mut AppState| {
                        if let Some(tx) = &s.gen_tx {
                            let _ = tx.send(GenCommand::Discover);
                        }
                    },
                )),
        );
    if g.provider == "http" {
        cockpit = cockpit.child(
            text(g.base_url.clone())
                .key("gen-baseurl")
                .font(type_::MONO_CAPTION)
                .color(t.text_secondary),
        );
    }

    // One numbered model list: discovered http pairs first, then opencode rows.
    let mut models = col().key("gen-models").gap(4.0);
    for (i, (base_url, model)) in g.models.iter().enumerate() {
        let base_url = base_url.clone();
        let model = model.clone();
        let active = g.provider == "http" && g.model == *model && g.base_url == *base_url;
        models = models.child(button(
            format!("gen-model-{i}"),
            format!("{model} — {base_url}"),
            active,
            active,
            t,
            move |s: &mut AppState| {
                s.r#gen.provider = "http".to_string();
                s.r#gen.base_url = base_url.clone();
                s.r#gen.model = model.clone();
            },
        ));
    }
    for (i, model) in g.opencode_models.iter().enumerate() {
        let model = model.clone();
        let index = g.models.len() + i;
        let active = g.provider == "opencode" && g.model == *model;
        models = models.child(button(
            format!("gen-model-{index}"),
            format!("{model} — opencode"),
            active,
            active,
            t,
            move |s: &mut AppState| {
                s.r#gen.provider = "opencode".to_string();
                s.r#gen.base_url.clear();
                s.r#gen.model = model.clone();
            },
        ));
    }
    cockpit = cockpit.child(
        models
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .max_h(120.0),
    );

    let mut cards = row().key("gen-cards").gap(6.0);
    for card in &s.cards {
        let hash8 = hash8(&card.hash).to_string();
        let card_hash = card.hash.clone();
        let active = g.card_hash.as_deref() == Some(card.hash.as_str());
        cards = cards.child(button(
            format!("gen-card-{hash8}"),
            format!("{} {hash8}", card.name),
            active,
            active,
            t,
            move |s: &mut AppState| s.r#gen.card_hash = Some(card_hash.clone()),
        ));
    }
    cockpit = cockpit.child(cards.scroll_x());

    cockpit = cockpit
        .child(
            text_input(&g.name_edit, type_::BODY)
                .key("gen-name")
                .single_line()
                .on_edit(|s: &mut AppState| &mut s.r#gen.name_edit)
                .on_input_change(|s: &mut AppState, _| {
                    s.r#gen.name = s.r#gen.name_edit.text.clone()
                }),
        )
        .child(button(
            "gen-kind".to_string(),
            if g.is_sprite {
                "sprite".to_string()
            } else {
                "tileset".to_string()
            },
            false,
            g.is_sprite,
            t,
            |s: &mut AppState| s.r#gen.is_sprite = !s.r#gen.is_sprite,
        ))
        .child(
            row()
                .key("gen-dims")
                .gap(6.0)
                .child(button(
                    "gen-size-minus".to_string(),
                    "-".to_string(),
                    false,
                    false,
                    t,
                    step_kind(-1),
                ))
                .child(
                    text(if g.is_sprite {
                        format!("sprite {}×{}", g.sprite_w, g.sprite_h)
                    } else {
                        format!("tileset {}×{} ×{}", g.tile_w, g.tile_h, g.tile_count)
                    })
                    .font(type_::LABEL)
                    .color(t.text),
                )
                .child(button(
                    "gen-size-plus".to_string(),
                    "+".to_string(),
                    false,
                    false,
                    t,
                    step_kind(1),
                )),
        );

    // While a run is active only the cancel control shows; there is no run button.
    if g.state == RunState::Running {
        cockpit = cockpit.child(button(
            "gen-cancel".to_string(),
            "cancel".to_string(),
            false,
            false,
            t,
            |s: &mut AppState| {
                if let Some(tx) = &s.gen_tx {
                    let _ = tx.send(GenCommand::Cancel);
                }
            },
        ));
    } else {
        let no_model = g.provider == "http"
            && g.base_url.is_empty()
            && g.models.is_empty()
            && g.opencode_models.is_empty();
        if no_model {
            // Inert: drawn and named, but pressing it does nothing.
            cockpit = cockpit.child(
                text("run")
                    .font(type_::LABEL)
                    .color(t.text_secondary)
                    .key("gen-run")
                    .role("button")
                    .label("run")
                    .px(10.0)
                    .py(5.0)
                    .radius(radius::SM)
                    .fill(t.control_off)
                    .inert(),
            );
        } else {
            cockpit = cockpit.child(button(
                "gen-run".to_string(),
                "run".to_string(),
                false,
                false,
                t,
                run_generate,
            ));
        }
        let note = g
            .note
            .clone()
            .or_else(|| no_model.then(|| "no model available".to_string()))
            .unwrap_or_default();
        cockpit = cockpit.child(
            text(note)
                .key("gen-note")
                .font(type_::MONO_CAPTION)
                .color(t.text_secondary),
        );
        if g.state == RunState::Passed && g.pending.is_some() {
            let ready = g.pending.as_ref().map_or(0, |pending| pending.assets.len());
            cockpit = cockpit.child(
                row()
                    .key("gen-result")
                    .gap(8.0)
                    .child(
                        text(format!("passed — {ready} assets ready"))
                            .font(type_::LABEL)
                            .color(t.text),
                    )
                    .child(button(
                        "gen-save".to_string(),
                        "save".to_string(),
                        false,
                        false,
                        t,
                        |s: &mut AppState| crate::store_io::save_generated(s),
                    )),
            );
        }
    }

    let mut log = col().key("gen-events").gap(2.0);
    for (i, event) in g.events.iter().enumerate() {
        log = log.child(
            text(event.clone())
                .key(format!("gen-event-{i}"))
                .font(type_::MONO_CAPTION)
                .color(t.text_secondary),
        );
    }
    cockpit.child(log.scroll(ScrollSpec { stick_bottom: true }).max_h(140.0))
}

/// Validate the panel, build the [`AssetRequest`] and dispatch the Start command.
fn run_generate(s: &mut AppState) {
    let Some(card_hash) = s.r#gen.card_hash.clone() else {
        s.notice = Some("select a style card first".to_string());
        return;
    };
    let name = s.r#gen.name.clone();
    if name.is_empty() {
        s.notice = Some("name is required".to_string());
        return;
    }
    let root = match conjure_store::locate(&s.root) {
        Ok(root) => root,
        Err(e) => {
            s.notice = Some(format!("error: {e}"));
            return;
        }
    };
    let store = match conjure_store::Store::open(&root) {
        Ok(store) => store,
        Err(e) => {
            s.notice = Some(format!("error: {e}"));
            return;
        }
    };
    let card = match store.load_card(&card_hash) {
        Ok(card) => card,
        Err(e) => {
            s.notice = Some(format!("error: {e}"));
            return;
        }
    };
    let kind = if s.r#gen.is_sprite {
        AssetKind::Sprite {
            size: Dimensions {
                w: s.r#gen.sprite_w,
                h: s.r#gen.sprite_h,
            },
        }
    } else {
        AssetKind::Tileset {
            tile_size: Dimensions {
                w: s.r#gen.tile_w,
                h: s.r#gen.tile_h,
            },
            count: s.r#gen.tile_count as usize,
        }
    };
    let request = AssetRequest {
        name,
        kind,
        style_card: card,
    };
    if let Some(tx) = &s.gen_tx {
        let _ = tx.send(GenCommand::Start {
            provider: s.r#gen.provider.clone(),
            model: s.r#gen.model.clone(),
            base_url: s.r#gen.base_url.clone(),
            request,
        });
    }
}
