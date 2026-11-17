use conjure_core::{AssetKind, AssetRequest, index_to_char};

pub(crate) fn system_prompt(request: &AssetRequest) -> String {
    let mut prompt = String::new();
    prompt.push_str("you are a pixel artist generating tile assets for a game.\n\n");

    prompt.push_str("palette chart:\n");
    for (i, color) in request.style_card.palette.iter().enumerate() {
        prompt.push_str(&format!(
            "'{}' -> index {i}: {} ({})\n",
            index_to_char(i as u8),
            color.name,
            color.hex
        ));
    }
    prompt.push_str("char '0' is the transparent background\n\n");

    append_schema(&mut prompt, request);

    prompt.push_str(&format!(
        "allowed row chars: '0'-'9' and 'a'-'v' only; a char for any index >= {} is invalid.\n\n",
        request.style_card.palette.len()
    ));

    let style = &request.style_card.style;
    let outline = format!("{:?}", style.outline).to_lowercase();
    let shading = format!("{:?}", style.shading).to_lowercase();
    prompt.push_str(&format!(
        "style tokens: outline: {outline}, outline color: {}, shading: {shading}, proportions: {}, era: {}, theme: {}.\n",
        style.outline_color, style.proportions, style.era, style.theme
    ));
    prompt.push_str(&format!(
        "draw something plausible for the theme '{}'.\n\n",
        style.theme
    ));

    prompt.push_str(&format!(
        "hard requirement: every row is EXACTLY the right char count ({} chars), no spaces, no padding.\n",
        kind_width(request)
    ));
    prompt
}

pub(crate) fn user_prompt(request: &AssetRequest) -> String {
    match request.kind {
        AssetKind::Sprite { size } => format!(
            "draw the sprite '{}' as a {}x{} pixel grid. respond with ONLY the JSON spec, no code fences, no commentary.",
            request.name, size.w, size.h
        ),
        AssetKind::Tileset { tile_size, count } => format!(
            "draw the tileset '{}' as {} tiles, each {}x{}. respond with ONLY the JSON spec, no code fences, no commentary.",
            request.name, count, tile_size.w, tile_size.h
        ),
    }
}

fn append_schema(prompt: &mut String, request: &AssetRequest) {
    match request.kind {
        AssetKind::Sprite { size } => prompt.push_str(&format!(
            "respond with exactly this JSON schema: {{\"type\":\"sprite\",\"rows\":[\"{} chars per row, {} rows\"]}}\n\n",
            size.w, size.h
        )),
        AssetKind::Tileset { tile_size, count } => prompt.push_str(&format!(
            "respond with exactly this JSON schema: {{\"type\":\"tileset\",\"tiles\":[[\"...row...\"],...]}}, with exactly {} tiles, each {} rows of {} chars.\n\n",
            count, tile_size.h, tile_size.w
        )),
    }
}

fn kind_width(request: &AssetRequest) -> u32 {
    match request.kind {
        AssetKind::Sprite { size } => size.w,
        AssetKind::Tileset { tile_size, .. } => tile_size.w,
    }
}
