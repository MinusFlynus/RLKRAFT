//! Fold only the zero-filled fields that this renderer does not supply.
//! Live frame, custom, compatibility and per-draw uniforms keep their bindings.
use std::collections::HashSet;

use naga::{Arena, Block, Expression, Function, Handle, Module, Statement, Type, TypeInner};

/// Keep the original module if validation fails: an optimization must never
/// prevent a pack that already rendered from loading.
pub(super) fn zero_fallbacks(module: &mut Module, offsets: &HashSet<u32>) -> Result<usize, String> {
    if offsets.is_empty() {
        return Ok(0);
    }
    let mut candidate = module.clone();
    let info = validator()
        .validate(&candidate)
        .map_err(|e| e.to_string())?;
    let Some((global, variable)) = candidate.global_variables.iter().find(|(_, g)| {
        g.space == naga::AddressSpace::Uniform
            && g.binding
                .as_ref()
                .is_some_and(|b| b.group == 0 && b.binding == 0)
    }) else {
        return Ok(0);
    };
    let TypeInner::Struct { members, .. } = &candidate.types[variable.ty].inner else {
        return Ok(0);
    };
    let fields: HashSet<u32> = members
        .iter()
        .enumerate()
        .filter(|(_, m)| offsets.contains(&m.offset))
        .map(|(i, _)| i as u32)
        .collect();
    let mut count = 0;
    for (handle, function) in candidate.functions.iter_mut() {
        count += specialize(
            function,
            &info[handle],
            &mut candidate.types,
            global,
            &fields,
        );
    }
    for (i, entry) in candidate.entry_points.iter_mut().enumerate() {
        count += specialize(
            &mut entry.function,
            info.get_entry_point(i),
            &mut candidate.types,
            global,
            &fields,
        );
    }
    if count > 0 {
        validator()
            .validate(&candidate)
            .map_err(|e| e.to_string())?;
        *module = candidate;
    }
    Ok(count)
}

fn validator() -> naga::valid::Validator {
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
}

fn member(
    expressions: &Arena<Expression>,
    pointer: Handle<Expression>,
    global: Handle<naga::GlobalVariable>,
) -> Option<u32> {
    match expressions[pointer] {
        Expression::AccessIndex { base, index } if matches!(expressions[base], Expression::GlobalVariable(g) if g == global) => {
            Some(index)
        }
        Expression::AccessIndex { base, .. } | Expression::Access { base, .. } => {
            member(expressions, base, global)
        }
        _ => None,
    }
}

fn specialize(
    function: &mut Function,
    info: &naga::valid::FunctionInfo,
    types: &mut naga::UniqueArena<Type>,
    global: Handle<naga::GlobalVariable>,
    fields: &HashSet<u32>,
) -> usize {
    let replacements: Vec<_> = function
        .expressions
        .iter()
        .filter_map(|(h, e)| {
            let field = match *e {
                Expression::Load { pointer } => member(&function.expressions, pointer, global),
                // The GLSL frontend loads a block value then accesses its
                // members; WGSL commonly loads through a member pointer.
                Expression::AccessIndex { base, index }
                    if matches!(function.expressions[base], Expression::Load { pointer }
                        if matches!(function.expressions[pointer], Expression::GlobalVariable(g) if g == global)) => Some(index),
                _ => None,
            };
            fields
                .contains(&field?)
                .then(|| (h, info[h].ty.clone()))
        })
        .collect();
    let mut count = 0;
    for (handle, resolution) in replacements {
        let inner = resolution.inner_with(types);
        // Uniform scalar/vector/matrix values have the exact all-zero bit
        // pattern already stored in Pass::block. Leave compound values alone.
        if !matches!(
            inner,
            TypeInner::Scalar(_) | TypeInner::Vector { .. } | TypeInner::Matrix { .. }
        ) {
            continue;
        }
        let ty = match resolution {
            naga::proc::TypeResolution::Handle(h) => h,
            naga::proc::TypeResolution::Value(inner) => {
                types.insert(Type { name: None, inner }, naga::Span::UNDEFINED)
            }
        };
        function.expressions[handle] = Expression::ZeroValue(ty);
        count += 1;
    }
    if count > 0 {
        fix_emits(&mut function.body, &function.expressions);
    }
    count
}

/// ZeroValue is in scope from function entry, unlike the Load it replaces.
/// Split emits around it, including inside branches, switches and loops.
fn fix_emits(block: &mut Block, expressions: &Arena<Expression>) {
    let old = std::mem::take(block);
    for (mut statement, span) in old.span_into_iter() {
        match &mut statement {
            Statement::Emit(range) => {
                let mut run = None;
                for handle in range.clone() {
                    if expressions[handle].needs_pre_emit() {
                        if let Some((first, last)) = run.take() {
                            block.push(
                                Statement::Emit(naga::Range::new_from_bounds(first, last)),
                                span,
                            );
                        }
                    } else {
                        run = Some((run.map_or(handle, |(first, _)| first), handle));
                    }
                }
                if let Some((first, last)) = run {
                    block.push(
                        Statement::Emit(naga::Range::new_from_bounds(first, last)),
                        span,
                    );
                }
                continue;
            }
            Statement::Block(b) => fix_emits(b, expressions),
            Statement::If { accept, reject, .. } => {
                fix_emits(accept, expressions);
                fix_emits(reject, expressions);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    fix_emits(&mut case.body, expressions);
                }
            }
            Statement::Loop {
                body, continuing, ..
            } => {
                fix_emits(body, expressions);
                fix_emits(continuing, expressions);
            }
            _ => {}
        }
        block.push(statement, span);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "
        struct Frame { supplied: vec4<f32>, missing: vec4<f32>, matrix: mat4x4<f32>, index: u32, }
        @group(0) @binding(0) var<uniform> frame: Frame;
        @group(2) @binding(0) var<uniform> draw: vec4<f32>;
        fn helper(i: u32) -> f32 { return frame.missing[i] + frame.matrix[1].z; }
        @fragment fn main() -> @location(0) vec4<f32> {
            var v = frame.supplied + draw;
            if(frame.missing.x > 0.0) { v += frame.missing; }
            for(var i=0u; i<2u; i++) { v.x += helper(i); }
            switch(frame.index) { case 0u: { v.y += frame.missing.y; } default: {} }
            return v;
        }";

    #[test]
    fn scalar_vector_matrix_components_and_nested_blocks_validate() {
        let mut module = naga::front::wgsl::parse_str(SOURCE).unwrap();
        let count = zero_fallbacks(&mut module, &HashSet::from([16, 32])).unwrap();
        assert!(count >= 5);
        let info = validator().validate(&module).unwrap();
        let used: Vec<_> = module
            .global_variables
            .iter()
            .filter(|(h, _)| !info.get_entry_point(0)[*h].is_empty())
            .filter_map(|(_, g)| g.binding.as_ref().map(|b| b.group))
            .collect();
        assert_eq!(used, [0, 2], "supplied and per-draw inputs stay live");
        assert_eq!(
            zero_fallbacks(&mut module, &HashSet::from([16, 32])).unwrap(),
            0
        );
    }

    #[test]
    fn empty_and_unmatched_offsets_leave_module_unchanged() {
        let mut module = naga::front::wgsl::parse_str(SOURCE).unwrap();
        let original = format!("{module:?}");
        assert_eq!(zero_fallbacks(&mut module, &HashSet::new()).unwrap(), 0);
        assert_eq!(
            zero_fallbacks(&mut module, &HashSet::from([1234])).unwrap(),
            0
        );
        assert_eq!(format!("{module:?}"), original);
    }

    #[test]
    fn gpu_results_match_zero_buffer_and_live_values_still_change() {
        let source = "
            struct Frame { live: vec4<f32>, missing: vec4<f32>, matrix: mat4x4<f32>, index: u32, }
            @group(0) @binding(0) var<uniform> frame: Frame;
            @group(0) @binding(1) var<storage, read_write> output: vec4<f32>;
            fn helper(i: u32) -> f32 { return frame.missing[i] + frame.matrix[1].z; }
            @compute @workgroup_size(1) fn main() {
                var value = frame.live;
                if(frame.missing.x > 0.0) { value *= frame.missing; }
                for(var i=0u; i<2u; i++) { value.x += helper(i); }
                switch(frame.index) { case 2u: { value.y += frame.missing.y; } default: { value.z += 1.0; } }
                output = value;
            }";
        let original = naga::front::wgsl::parse_str(source).unwrap();
        let mut specialized = original.clone();
        assert!(zero_fallbacks(&mut specialized, &HashSet::from([16, 32])).unwrap() >= 4);
        let glsl = naga::front::glsl::Frontend::default().parse(
            &naga::front::glsl::Options::from(naga::ShaderStage::Compute), "
            #version 450
            layout(local_size_x=1) in;
            layout(std140,set=0,binding=0) uniform Frame { vec4 live; vec4 missing; mat4 matrix; uint index; } frame;
            layout(std430,set=0,binding=1) buffer Output { vec4 value; } result;
            float helper(uint i) { return frame.missing[i] + frame.matrix[1].z; }
            void main() {
                vec4 value=frame.live;
                if(frame.missing.x>0.0) value*=frame.missing;
                for(uint i=0u;i<2u;i++) value.x+=helper(i);
                switch(frame.index) { case 2u: value.y+=frame.missing.y; break; default: value.z+=1.0; }
                result.value=value;
            }").unwrap();
        let mut glsl_specialized = glsl.clone();
        assert!(zero_fallbacks(&mut glsl_specialized, &HashSet::from([16, 32])).unwrap() >= 4);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            eprintln!("GPU equivalence test skipped: no adapter");
            return;
        };
        eprintln!("Zero specialization GPU test: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 112,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let pipelines: Vec<_> = [original, specialized, glsl, glsl_specialized]
            .into_iter()
            .map(|module| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: None,
                    source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
                });
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: None,
                    module: &shader,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
            .collect();
        for live in [[-1.0f32, 0.25, 0.6, 2.0], [7.0, 0.5, -3.0, 0.0]] {
            let mut data = [0u32; 28];
            data[..4].copy_from_slice(&live.map(f32::to_bits));
            data[24] = 2;
            queue.write_buffer(&uniform, 0, bytemuck::cast_slice(&data));
            let mut results = Vec::new();
            for pipeline in &pipelines {
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: output.as_entire_binding(),
                        },
                    ],
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                encoder.copy_buffer_to_buffer(&output, 0, &read, 0, 16);
                queue.submit([encoder.finish()]);
                read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                results.push(read.slice(..).get_mapped_range().unwrap().to_vec());
                read.unmap();
            }
            for result in &results {
                assert_eq!(
                    result, &results[0],
                    "specialized GPU output must match runtime zeros"
                );
                assert_eq!(result, bytemuck::cast_slice::<f32, u8>(&live));
            }
        }
    }

    #[test]
    fn glsl_uniform_loads_and_invalid_module_fallback() {
        let mut frontend = naga::front::glsl::Frontend::default();
        let mut module = frontend.parse(&naga::front::glsl::Options::from(naga::ShaderStage::Fragment), "
            #version 450
            layout(set=0, binding=0) uniform Frame { vec4 live; int missing; vec3 missingVector; } frame;
            layout(location=0) out vec4 color;
            void main() { color=frame.live; if(frame.missing!=0) { color+=vec4(frame.missingVector,0.0); } }
        ").unwrap();
        assert!(zero_fallbacks(&mut module, &HashSet::from([16, 32])).unwrap() >= 2);
        validator().validate(&module).unwrap();
        module.entry_points[0].workgroup_size = [0, 0, 0];
        module.entry_points[0].stage = naga::ShaderStage::Compute;
        let before = format!("{module:?}");
        assert!(zero_fallbacks(&mut module, &HashSet::from([16])).is_err());
        assert_eq!(format!("{module:?}"), before);
    }
}
