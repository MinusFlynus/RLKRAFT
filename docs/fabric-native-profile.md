# Fabric profile: native feature ports

The target is the user's Fabric 26.3 profile, inventoried locally with REA
6.8.0 and JADX 0.7.1: 35 installed jar artifacts. The original jars,
decompiled source, configuration and game assets stay outside this repository.
Class inventories establish available names and members, not runtime parity.

## Native chat portraits: initial implementation

An optional `chatHeads` switch in Horizon Extras adds flat 8-pixel portraits
with a 1-pixel right gap to UUID-associated multiplayer player messages.
REA's `ChatHeads.headWidth` observation confirms 8 + right padding + rounded
3D width: the user's zero 3D amount and right padding 1 imply 9 pixels.
Evidence: `ev_25d2e9602187a18e17fc6830b6748d86ac0496852d40e7f4205934ba87937431`;
input jar SHA-256 `1da6f5e86ba3920e096509e1d6d7bb5613b7458ff5b2d4be7144925e0d3f545d`.

The implementation reuses Horizon's player-list face renderer, downloaded
skins and normal fallback skins. It does not add downloads or per-frame file
reads. The history remains capped at 100 lines; a wrapped message has one
portrait, and normal fade, visibility filters and text click actions apply.
Disabling the switch restores the original layout. Tests cover wrapping,
face count, text history, fading, visibility and clicking through the padding.

This is a feature port, not complete Chat Heads compatibility. Portraits use
a left text gutter rather than inserting into decorated names. System-message
heuristics, aliases, 3D heads, face shadows and book/suggestion portraits are
not implemented. The skin is resolved when the message arrives, so an old
message can retain its fallback skin after a later skin download.

## Features already implemented in Rust

Shader packs opened from disk now read adjacent `<pack filename>.txt` scalar
overrides once at load time. Numeric and boolean values replace exact GLSL
define names or scalar constants without rewriting the original pack. This
covers the user's five Unbound overrides; it does not establish compatibility
with every Iris shader stage, profile, expression or rendering feature.
Tests check archive preservation, language-file isolation and source handling.

The optional native Falling Leaves tuning keeps the installed profile's
0.1-block leaf size, 200-tick lifetime and wind switch. REA observed that the
installed regular-leaf rate 20 maps to the same 0.01 ambient-tick chance Horizon
already uses, while conifer rate 0 matches Horizon's disabled spruce leaves.
The mod's seasonal multipliers, snowflakes, decaying-leaf bursts and custom
wind-state noise are not implemented.

Horizon already provides dynamic lights, Mouse Tweaks-style controls,
inventory sorting with mouse bindings, a minimap and a recipe-viewer API.
Those are native equivalents, not complete ports of LambDynamicLights,
Mouse Tweaks, ClientSort, Xaero or JEI. In particular, Xaero's full-world
map, saved map format and waypoints are not replaced by a corner minimap.
Sodium, Lithium, C2ME, FerriteCore and ImmediatelyFast optimize the Java
engine; their Java mixins are not loaded into Horizon's Rust engine.

Remaining feature ports include ambient sound behavior, sound occlusion and
reverb, extra animations, connected textures, build-printing tools and the
additional UI/mod integrations. No claim of their implementation or of
equivalent performance is made. Shader support is a separate renderer
compatibility question; copying a pack or its settings does not prove it runs.

The focused Windows workflow lints the integration, tests the native chat and
extras logic and builds an optimized client. In-game checks and profiling
still need to be run before calling this profile verified or faster.
