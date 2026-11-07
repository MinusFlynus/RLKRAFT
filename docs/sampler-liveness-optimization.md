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
and live player/server data.

## Measured result, 2026-11-07

Not accepted as the production FPS optimization. Candidate
`b26dffd6a28713a0858fabb8b568760de2ebacfa` is preserved on
`experiments/sampler-liveness`. Restore production runtime/workflow files to
the measured best base `f40af12021ba4cd15ce186765ca1f3cb34278657`
(runtime identical to `6b28be88a1dae5916b4cd9056a6f6ff0182c25b0`).
The launcher stays on its existing tested binary.

Windows CI run 42381670845 passed. Artifact 13578627126 ZIP SHA-256:
`457f0f477d3ff7baa1417f8b80bb37533bf38beb2ef7a68c6bc80773d6212006`.
Candidate client SHA-256:
`d2aa1efcbef619fd204b7456da3bc4e517c75128880ca6a55855da3a3fa810df`.

The same executable ran off/on/on/off, with six eight-second samples per
mode/view (24 samples total), no pass-timer instrumentation, fixed per-frame
cameras, and fresh disposable copies of the current packs/settings and fixture
world. At native 1920x1080, RD16, Complementary Unbound r5.9.3:

- Overview: 118.281 -> 117.297 FPS (-0.83%); GPU 8.269 -> 8.327 ms.
- Terrain: 101.856 -> 101.027 FPS (-0.81%); GPU 9.608 -> 9.684 ms.
- Overview 1% low: 93.809 -> 81.970 FPS; terrain 81.961 -> 84.164 FPS.
  Low-frame samples were variable; this is no evidence of a general stutter win.
- Tracked GPU allocations fell by 70,811,392 / 70,739,744 bytes in the two
  views (about 67.5 MiB). Reserved pool memory remained 1,677,721,600 bytes.
  Tracked allocation savings are not the same as physical VRAM returned.
- First-frame times off: 7645 / 7554 ms; on: 12271 / 7503 ms.
  One enabled cold launch was slower; the repeated enabled launch was similar
  to controls. Do not attribute a repeatable startup penalty from two runs.
- Runtime notes reported 460 omitted sampler declarations across programs.
  Both modes loaded 1057 chunks / 6982 resident sections, with matching camera
  poses and 1652 / 1856 section draws. Vertex allocation varied slightly from
  ordinary scene animation; it was not claimed byte-identical across launches.

Screenshot inspection found no obvious terrain/water loss and retained the
native minimap. This is limited visual evidence, not complete shader-pack or
mod parity. No Java Minecraft comparison was run. Only the existing expected
cloud program with no outputs was skipped; shader compatibility remains partial.
Graphics quality was not reduced. Live settings, player data and server data
were untouched; all test clients exited.

Local evidence: `outputs/Horizon-sampler-liveness-validation.json` and
`work/fabric-rust-port/sampler-liveness-{1..4}-{0|1}-20261107/band-64`.
Because measured average FPS did not improve and memory pressure was not shown
to be a bottleneck here, preserve the smaller-resource method for future
memory-constrained testing rather than enabling it by default.

The next investigation should target the measured terrain pass (5.832 ms in
the separate instrumented baseline), with conservative geometry/deformation,
alpha/discard, temporal-history and attachment-consumer semantics preserved.
Do not infer an Early-Z hardware counter result: those counters were not captured.
Naga 30.0.1 offers `compact(module, KeepUnused::No)` to remove unreachable
functions/globals, but that follow-up has not been implemented or benchmarked.
Compaction alone must not be advertised as a GPU-time win.
