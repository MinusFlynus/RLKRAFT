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

Windows release build, outdoor FPS, loading time and visual comparison: pending.
