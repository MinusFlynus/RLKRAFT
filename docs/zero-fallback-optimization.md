# Shader zero-fallback specialization experiment

Goal: reduce shader work at the current resolution, view distance and pack
settings, preserving the supported native mod features and current image.

Research (7 November 2026):

- NVIDIA Vulkan Dos and Don'ts recommends specialization constants and
  measuring pipeline changes rather than assuming an improvement:
  https://developer.nvidia.com/blog/vulkan-dos-donts/
- NVIDIA's shader performance guidance discusses uniform buffer access,
  control flow and register pressure:
  https://developer.nvidia.com/blog/advanced-api-performance-shaders/

Local evidence: the previous warmed 1080p/RD16 profile was GPU-bound. Terrain
cost roughly 4.7 ms of 8.7 ms total GPU time. The previous narrower draw
grouping and separate depth prepass experiments did not improve that scene;
the latter regressed performance. Stable opaque plane ordering helped a
constructed wall stress scene while outdoor FPS was effectively unchanged.

This experiment specializes only fields that `PackPost::bind` already leaves
at their initial all-zero bit pattern. Resolve the supplied frame and custom
names first, exclude compatibility fields with the reserved `hz_` prefix,
and inspect only the uniform block at group 0/binding 0. Replace loads or
value accesses of those members with typed zeros. Leave compound values and
all supplied/per-draw data alone. Validate a separate candidate module and
retain the original if validation fails.

The source handles both member-pointer loads and GLSL's loaded-block member
accesses. Emission ranges are repaired recursively because typed zero values
are in scope from function entry. No scene geometry, shader pack source,
texture, render resolution, shadow map size or view distance is reduced.
This does not add previously unsupported shader inputs.

Validation before runtime comparison: formatting, Clippy and four regressions
passed locally. The GPU test compares original and specialized WGSL and GLSL
compute output byte-for-byte on the RTX 3060 Ti/Vulkan for two different live
input sets. Tests also cover nested control flow, matrix/vector/scalar accesses,
per-draw input liveness, unmatched offsets, idempotence and invalid-module
fallback. GPU tests explicitly report a skip if CI has no adapter.

`HORIZON_PACK_ZERO_SPECIALIZE=0` keeps the original program for matched A/B
benchmarks from the same executable; the candidate defaults to specialization.
The installed launcher build remains unchanged until full-game timing and
visual checks justify replacing it. Use isolated offline fixtures and the
existing per-frame scripted camera path; hidden windows can still receive
physical mouse movement. Require fully loaded terrain and no competing game.

Result: rejected for this setup; production source restored to the measured best
renderer, commit `6b28be88a1dae5916b4cd9056a6f6ff0182c25b0`. The experiment
remains in commit `259fc9de32b95fec304a99455413cc66ab489447` and its archive
branch. It was not installed in the pinned RLKRAFT launcher.

Windows CI run 42369877743 passed. The artifact archive SHA-256 was verified as
`93b8a0c8561f178f1103b06278fc45e11c6af328fa01693c6a79eaaf764aefc5`.
The candidate client was
`775152fb0d8316069b9c75062f146a8740eff6f8ee1e7ca9a71a1127bcb9992a`.

Off/on/on/off comparisons used fresh copied worlds, six eight-second samples
per mode/view, fixed per-frame camera paths and current RLKRAFT packs/settings.
At 1920x1080, 100% scale and RD16, overview FPS averaged 118.756 -> 119.136
(+0.32%) and terrain 102.359 -> 102.672 (+0.31%). GPU time changed only
8.243 -> 8.217 ms and 9.565 -> 9.537 ms. The terrain 1% low was
92.001 -> 92.343 FPS. These differences are too small for a material FPS claim.

First-frame latency averaged 5.624 -> 6.805 seconds; full-world warm-up was
47.620 -> 48.501 seconds. The extra compilation/validation cost outweighed the
small steady-state result. Visual inspection found no obvious terrain loss,
and camera poses, loaded chunks (1057), resident sections (6982), and draw
counts (1652/1856) matched. This is an offline Horizon comparison, not a Java
Minecraft comparison or a test of all unported mods.

Evidence is saved locally in `outputs/Horizon-zero-specialization-validation.json`.
No launcher binaries, graphics settings or live player/server data were changed.
The restoration's runtime files and workflow match the previously validated
base exactly; no new game build is needed for the research-only result.
