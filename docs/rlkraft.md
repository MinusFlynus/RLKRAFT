# RLKRAFT port status

Target: Shivaxi's original RLCraft 2.9.3, Minecraft 1.12.2,
Forge 14.23.5.2860. Horizon targets Minecraft 26.3. This branch is preparation
for a port, not a playable RLCraft release or a claim of gameplay parity.

## What this branch adds

The pack inventory reads Forge 1.12 `mcmod.info`, both array and `modList`
forms, and counts legacy recipes, loot tables and structures under `assets/`.
It records explicitly enabled `requiredMods`, excluding the game and loader;
optional ordering dependencies are not treated as requirements. Annotation
dependencies, additional mod identities in a multi-mod jar, registry behavior
and Java logic still need separate inspection. Counting content does not make
its 1.12 formats compatible with 26.3.

`shaderpacks/rlkraft_lite` is an original native Horizon shader pack. It uses
one colour-grading pass with no explicit texture reads or temporal history.
The 0.5 ms budget is a target, not a measured result. It supplies gentle colour
grading rather than the shadows and lighting of a full shader suite. Validate
and package it with:

```sh
cargo run -p horizon-shader-api --bin horizon-shader -- check shaderpacks/rlkraft_lite --json
cargo run -p horizon-shader-api --bin horizon-shader -- pack shaderpacks/rlkraft_lite
```

## Evidence and reproduction

Official inputs:

- [RLCraft client manifest archive](https://www.curseforge.com/minecraft/modpacks/rlcraft/files/4612979),
  SHA-256 `5caa25d31f47f4ac69846e4faa741811baa9239804747769f6d54f7b1bbf1291`.
  Its manifest requests 187 files; the archive also contains 3 jars.
- [RLCraft server archive](https://www.curseforge.com/minecraft/modpacks/rlcraft/files/4612990),
  SHA-256 `a29b3c0c99b41e1c7d404a78f48b7f9698f15b068141a042d5ca1108a8636c55`.
  It supplies 180 jars. A server inventory does not cover every client-only file.
- [Forge 1.12 metadata documentation](https://docs.minecraftforge.net/en/1.12.x/gettingstarted/structuring/).

REA 6.8.0 inspected the archive entry inventories. Its JADX 0.7.1 engine also
successfully loaded SimpleDifficulty (172 top-level classes) and identified
the thirst capability, handler, GUI and update message. The JAR was copied
byte-for-byte to an `.apk` analysis carrier, following Horizon's existing
`tools/rea_vanilla.py` convention; it is a Java mod, not an Android app.
Static observations do not establish runtime behavior. No mod jars,
decompiled code, Minecraft assets or pack configuration are committed here.

The completed REA/JADX pass inspected all 180 server-pack jar artifacts,
enumerating 28,363 top-level classes with no failed inventories. The per-jar
SHA-256, class count and Evidence ID are in
[the inventory CSV](modpacks/rlcraft-2.9.3-rea.csv). Each empty-query class
selection returned the engine's full reported class inventory. This is not a
method-by-method behavior audit; inner/synthetic class coverage follows JADX's
reported inventory, and the client-only artifacts remain a separate boundary.
SimpleDifficulty's `ThirstCapability.tickUpdate` was also inspected separately
(Evidence `ev_15b47fbeb06bfa6b2c6cb400b609383f9c7c4cfdc0b147e0036769f6348f2895`).
Its behavior is not implemented or runtime-verified on this branch.

## Validation and remaining work

Formatting and the public-tree check run locally. Compilation on the test PC
was blocked by Windows Application Control (error 4551) when Cargo tried to
run a generated dependency build script. The focused GitHub workflow tests
the importer and shader API on Windows and Linux, validates the shader and
packages it; its result must be checked independently.

The test PC has a Ryzen 5 5600X and RTX 3060 Ti. The checked default Minecraft
installation contains 26.2; this Horizon revision needs 26.3 resources. No
in-game test, FPS measurement or shader GPU timing has been completed.

Remaining: resolve the client-only inventory; inspect each mod's behavior and
pack overrides; implement dependencies and gameplay in dependency order;
compare original Java behavior with Rust under fixed scenarios; integrate
registrations, assets, saving and multiplayer; then measure cold load, RAM,
frame-time percentiles and server ticks in a fresh isolated test instance.
Optimization claims require that evidence, not simply a change of language.
