# Shader resource liveness experiment

## Finding and scope

The attachment audit found that `build_pass` creates runtime bindings and read
dependencies for every declared sampler, including declarations never read by
a reachable shader entry point. Those dependencies feed color target allocation,
clear lists, depth-copy requirements and sampler groups. Driver elimination of
unused shader instructions cannot remove this engine-side resource work.

The candidate reuses the `ModuleInfo` already returned by the shader translator's
validation. It marks each sampler used if either its texture or sampler binding
is referenced by an entry point in either stage. Queries and uniform-dependent
branches remain dependencies; never-called helpers do not create dependencies.
Original binding numbers and shader instructions are unchanged.

The renderer omits only bindings marked unused. `HORIZON_PACK_SAMPLER_PRUNE=0`
restores declaration-based bindings for same-executable comparisons. Shader load
notes report omitted declaration counts. Any declaration-level format semantics
remain intact: declaring `watershadow` still changes the legacy `shadow` alias,
even when the former is not sampled.

This does not strip discard, invent deformation bounds, reduce resolution,
modify geometry, drop output attachments, or blindly apply StoreOp::Discard.
Existing clears, writes, flips, temporal history and mipmap request scheduling
remain authoritative for resources still needed. In particular, a mipmap request
must not simply be masked by that program's own reads: it can prepare data read
by a later program.

## Audit/profiling context

The report's proposed blanket transient-attachment change is unsafe for G-buffers
sampled by later passes. Existing code also uses separate light-frustum shadow
visibility and already has asynchronous per-pass GPU timers. Nsight/RenderDoc
were not found on PATH or in the checked standard install directories; no profiler
installation or security/driver settings were changed. Use the existing timers
first and keep pass profiling separate from ordinary frame-rate comparisons.

Primary API references:

- Naga's pinned local `valid::FunctionInfo`/`ModuleInfo` implementation documents
  per-global use information indexed by global handle, including call analysis.
- https://docs.rs/wgpu/30.0.1/wgpu/struct.BindGroupLayoutDescriptor.html
- https://docs.vulkan.org/spec/latest/chapters/fragops.html
- https://developer.nvidia.com/blog/advanced-api-performance-shaders/

## Validation and acceptance

All 51 shader tests passed, including a new test covering vertex-only reads,
fragment reads, queries, dynamic branches, unused declarations and uncalled
helpers. Original declaration/binding metadata is preserved. Four renderer
pass tests passed, including GPU byte-for-byte equivalence with full and pruned
binding layouts on RTX 3060 Ti/Vulkan. Shader instructions and active inputs
were identical in that GPU test.

Windows CI, full-game profiling, repeated camera-locked outdoor comparisons,
memory/resource counts, startup and visual checks are required before installing.
Keep the same current packs/settings, opaque plane ordering and RD16/1080p.
Accept only a useful repeatable FPS/stutter or memory improvement without a
material performance/visual regression. Preserve the existing installed build
and live player/server data. No useful gain is claimed yet.
