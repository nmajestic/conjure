# Conjure — Product Brief

**Status:** Draft v0.1
**Date:** 2026-11-16

---

## 1. Positioning

> **Conjure is the agent-first asset tool for solo and indie game devs. You describe your game's look once — palette, grid, style — and Conjure's agent generates, validates, and manages a whole consistent pixel-art set: characters, tiles, animations, icons.**

**In one sentence:** Describe the style once. Get a consistent set. Stay in charge.

**The problem in one line:** Pixel art is easy to generate *once* and nearly impossible to generate *consistently* — and consistency is the entire point of game assets.

## 2. The Problem

Pixel art is uniquely hostile to today's image models, for three concrete reasons:

1. **Grid discipline** — models blur and anti-alias. Pixel art needs hard, grid-snapped pixels at an exact scale (16×16, 32×32). Diffusion models almost never produce a true pixel grid.
2. **Palette fidelity** — pixel art is *defined* by a limited, deliberate palette (8–32 colors). Models drift colors by thousands of shades.
3. **Set consistency** — a character in three poses, a tileset where every tile shares a style, an animation's frames. "One great image" is easy; "one great *set*" is not.

**Who feels this pain:** Solo and indie game developers who can't (or don't want to) do pixel art, but need a coherent look across their whole game.

## 3. The Core Insight

Treating asset generation as a **generation** problem is the failure mode. Conjure treats it as a **constraint-satisfaction** problem:

> **Spec → Generate → Validate → Repair → Ship**

This reframe is what makes the product defensible and what makes it *agentic* — the loop of generate, validate, and repair is exactly what an LLM agent is good at.

## 4. The Core Loop

1. **Spec** — a structured description: grid size, palette, style tokens, frame count. Source of truth, *not* a prompt.
2. **Generate** — an LLM reasons over the spec and produces a compact spec that expands deterministically into pixel data.
3. **Validate** — a verifier enforces the contract: grid-snapped, palette-exact, no anti-aliasing, correct dimensions, correct frame count. Every failure returns a *reason the agent can act on*.
4. **Repair** — the agent fixes failures and re-validates.
5. **Ship** — output lands in the library, correctly named, versioned, exportable.

**Reproducibility:** same spec + seed = same result.

## 5. The Style Card (crown jewel)

"Across a whole set" only works if consistency lives in one reusable object. The **Style Card** is the single source of visual truth:

- One locked **palette** (8–32 named colors)
- **Grid & scale** conventions
- **Style tokens** (outline weight, shading, proportions, era/theme)

One style decision, made once, inherited by every sprite, tile, and frame.

## 6. The Three Pillars

1. **Generate** — spec-driven, LLM-reasoned, constraint-verified.
2. **Validate** — the verifier is as important as the generator. It's the *trust layer*.
3. **Manage** — palettes, sprite sheets, animation frames, variants, naming, versioning, engine export.

## 7. Agentic-First, Defined

Conjure is **both** a tool-callable surface **and** a built-in agent:

- **Tool-callable** — a library / CLI / MCP server with clear schemas and error messages a model can reason about.
- **Built-in agent** — an autonomous agent that owns a full generation task end-to-end.

Concrete consequences: tool/API surface first, UI second; verifiable outputs; deterministic and reproducible; a human is optional.

## 8. Personas

- **Primary — Solo/indie game dev.** Needs a coherent look, fast, with zero pixel-art skill.
- **Secondary — Agent/LLM builder.** Wires Conjure into their own agents.
- **Tertiary — Pixel artist.** Multiplies output while keeping their style.

## 9. The Wedge, and What Comes After

- **Wedge: pixel art.** The hardest consistency problem. Winning here proves the loop.
- **After:** the spec schema and verifiers are the reusable part — 3D, audio, UI kits later. **Not now. Depth first.**

## 10. Non-Goals

- Not a general image generator / Midjourney competitor.
- Not a pixel-art editor (Aseprite owns that).
- Not a "one great image" toy — it is a *set* tool.
- Not (v1) a collaboration/multi-user platform.

## 11. North-Star Metric

> **A solo dev can get a style-consistent, engine-ready asset set from a text brief in minutes, with zero pixel-art skill — and every asset passes validation.**

## 12. First Milestone

A CLI/tool that takes one Style Card + one asset request (e.g., "16×16 dungeon tileset, 12 tiles, this palette") and returns tiles that **pass every validation check**. No UI, no library — just prove the loop.

## 13. Open Questions

1. **Representation** — *(resolved)* indexed pixels + palette map; model emits a compact spec.
2. **LLM output reliability** — can a model reliably emit exact palette indices? Milestone 1b answers this empirically.
3. **Export targets** — which engines first? (Godot is a strong default.)
4. **Distribution form** — CLI/MCP first (per the agent-first principle).
