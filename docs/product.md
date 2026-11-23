# Conjure: product

## In one line

Describe your game's look once (palette, grid, style) and get a whole
consistent pixel-art set that passes validation, from an agent you stay in
charge of.

## The problem

Pixel art is easy to generate once and hard to generate consistently, and
consistency is the point of game assets. Image models struggle in three ways:

1. **Grid discipline.** They blur and anti-alias. Pixel art needs hard pixels on
   an exact grid.
2. **Palette fidelity.** Pixel art uses a small, deliberate palette. Models
   drift by thousands of shades.
3. **Set consistency.** One good image is easy. A character in three poses, or
   a tileset where every tile matches, is not.

The people who feel this most are solo and indie game developers who can't
draw pixel art, or don't want to, but need one coherent look across a game.

## The approach

Conjure treats asset generation as meeting a contract, not writing a prompt:

```
spec → generate → validate → repair → ship
```

1. **Spec:** a structured request (size, kind, count) plus a style card. This
   is the source of truth.
2. **Generate:** a model writes a compact text spec that expands exactly into
   pixels.
3. **Validate:** a verifier checks dimensions, pixel counts, palette and asset
   count. Every failure says what to fix.
4. **Repair:** the agent fixes the failures and validates again, then falls
   back to a deterministic repair if the model can't get there.
5. **Ship:** verified assets are saved to the project library and exported as
   indexed PNGs with a manifest.

The same request and seed always produce the same result.

## The style card

Consistency lives in one reusable object, the style card: a locked palette
(up to 32 named colors), grid and scale conventions, and style settings such
as outline, shading, proportions, era and theme. Every sprite, tile and frame
in a set inherits it.

## Three pillars

1. **Generate:** spec-driven, model-reasoned, verified.
2. **Validate:** the verifier matters as much as the generator. It is the
   trust layer.
3. **Manage:** a versioned library, tags, and engine-ready export.

## Agent-first

Conjure is both a tool an agent can call (CLI and MCP server, with clear
schemas and actionable errors) and an agent of its own that runs a whole
generation task. In practice that means: tools before UI, verifiable output,
reproducible results, and a human who can step in but doesn't have to.

## Who it's for

- **Primary:** solo and indie game developers who need a coherent look fast,
  without pixel-art skill.
- **Secondary:** people building agents who want to wire Conjure in.
- **Tertiary:** pixel artists who want more output in their own style.

## Scope

**Pixel art first.** It is the hardest consistency problem, so winning here
proves the loop. The spec format and verifiers could later extend to other
asset types, but not before pixel art is deep.

**Not:**
- a general image generator
- a pixel-art editor
- a "one great image" toy (it is a set tool)
- a multi-user or collaboration platform

## How we'll know it works

A solo developer gets a style-consistent, engine-ready asset set from a short
text brief in minutes, with no pixel-art skill, and every asset passes
validation.

## Open questions

1. **Model reliability:** how often local models produce valid compact specs
   without the deterministic fallback.
2. **Engine export:** which engine gets a dedicated exporter first (Godot is
   the likely default).
3. **Distribution:** how Conjure is packaged and installed.

Current status and what's built: see the [README](../README.md).
