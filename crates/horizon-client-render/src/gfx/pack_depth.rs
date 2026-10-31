//! A depth-only fragment entry point retaining the pack's discard logic.
//! Removing colour results lets the GPU compiler eliminate unused lighting;
//! vertex deformation, alpha tests and their inputs stay the pack's own.

use naga::{
    AddressSpace, Binding, Block, ImageClass, Module, ShaderStage, Statement, StorageAccess,
    TypeInner,
};

pub(super) fn program(original: &Module) -> Option<Module> {
    // A second invocation must not repeat observable writes.
    for (_, global) in original.global_variables.iter() {
        if matches!(global.space, AddressSpace::Storage { access } if access.contains(StorageAccess::STORE))
            || matches!(original.types[global.ty].inner,
                TypeInner::Image { class: ImageClass::Storage { access, .. }, .. }
                    if access.contains(StorageAccess::STORE))
        {
            return None;
        }
    }
    let index = original
        .entry_points
        .iter()
        .position(|e| e.stage == ShaderStage::Fragment)?;
    let result = original.entry_points[index].function.result.as_ref()?;
    let colours_only = match &result.binding {
        Some(Binding::Location { .. }) => true,
        Some(_) => false,
        None => match &original.types[result.ty].inner {
            TypeInner::Struct { members, .. } => members
                .iter()
                .all(|m| matches!(m.binding, Some(Binding::Location { .. }))),
            _ => false,
        },
    };
    // Explicit depth and sample coverage must remain on the original path.
    if !colours_only {
        return None;
    }
    let mut module = original.clone();
    let function = &mut module.entry_points[index].function;
    function.result = None;
    strip_returns(&mut function.body);
    Some(module)
}

fn strip_returns(block: &mut Block) {
    for statement in block.iter_mut() {
        match statement {
            Statement::Return { value } => *value = None,
            Statement::Block(b) => strip_returns(b),
            Statement::If { accept, reject, .. } => {
                strip_returns(accept);
                strip_returns(reject);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    strip_returns(&mut case.body);
                }
            }
            Statement::Loop {
                body, continuing, ..
            } => {
                strip_returns(body);
                strip_returns(continuing);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_colour_interface_but_keeps_alpha_discard_and_inputs() {
        let original = naga::front::wgsl::parse_str("@fragment fn fs(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { if uv.x < 0.5 { discard; } return vec4<f32>(uv, 0.0, 1.0); }").unwrap();
        let depth = program(&original).unwrap();
        assert!(depth.entry_points[0].function.result.is_none());
        assert_eq!(
            format!("{:?}", original.entry_points[0].function.arguments),
            format!("{:?}", depth.entry_points[0].function.arguments)
        );
        assert_eq!(
            format!("{:?}", original.functions),
            format!("{:?}", depth.functions)
        );
        assert!(depth.entry_points[0].function.body.iter().any(|s|
            matches!(s, Statement::If { accept, .. } if accept.iter().any(|s| matches!(s, Statement::Kill)))));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&depth)
        .unwrap();
        assert!(original.entry_points[0].function.result.is_some());
    }

    #[test]
    fn rejects_depth_coverage_and_storage_side_effects() {
        for text in [
            "@fragment fn fs() -> @builtin(frag_depth) f32 { return 0.5; }",
            "struct Out { @location(0) colour: vec4<f32>, @builtin(sample_mask) mask: u32 } @fragment fn fs() -> Out { return Out(vec4<f32>(1.0), 1u); }",
            "@group(0) @binding(0) var<storage,read_write> counter: u32; @fragment fn fs() -> @location(0) vec4<f32> { counter = 1u; return vec4<f32>(1.0); }",
        ] {
            let original = naga::front::wgsl::parse_str(text).unwrap();
            assert!(program(&original).is_none());
        }
    }

    #[test]
    fn gpu_prepass_keeps_cutout_holes_and_visible_colours() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            eprintln!("depth prepass GPU oracle skipped: no adapter");
            return;
        };
        eprintln!("depth prepass GPU oracle: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let source = r#"
struct Varying { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) @interpolate(flat) layer: u32 }
@vertex fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) layer: u32) -> Varying {
    let p = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u)) * 2.0 - 1.0;
    return Varying(vec4<f32>(p, select(0.7, 0.3, layer == 1u), 1.0), p * 0.5 + 0.5, layer);
}
@fragment fn fs(v: Varying) -> @location(0) vec4<f32> {
    if v.layer == 1u && v.uv.x < 0.5 { discard; }
    return select(vec4<f32>(1.0,0.0,0.0,1.0), vec4<f32>(0.0,0.0,1.0,1.0), v.layer == 1u);
}"#;
        let original = naga::front::wgsl::parse_str(source).unwrap();
        let depth = program(&original).unwrap();
        let main = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(original)),
        });
        let depth = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(depth)),
        });
        let targets = [Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba8Unorm,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = |fragment: &wgpu::ShaderModule, depth_only: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &main,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: fragment,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: if depth_only { &[] } else { &targets },
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let main_pipeline = pipeline(&main, false);
        let depth_pipeline = pipeline(&depth, true);
        let render = |prepass: bool| {
            let size = wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            };
            let texture = |format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let colour = texture(
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let colour_view = colour.create_view(&Default::default());
            let depth = texture(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let depth_view = depth.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            if prepass {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&depth_pipeline);
                pass.draw(0..3, 0..2);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &colour_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: if prepass {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(1.0)
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&main_pipeline);
                pass.draw(0..3, 0..2);
            }
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 256 * 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                colour.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                size,
            );
            queue.submit([encoder.finish()]);
            read.slice(..)
                .map_async(wgpu::MapMode::Read, |result| result.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let mapped = read.slice(..).get_mapped_range().unwrap();
            mapped
                .chunks(256)
                .flat_map(|row| row[..64].iter().copied())
                .collect::<Vec<u8>>()
        };
        let original = render(false);
        assert_eq!(&original[..4], &[255, 0, 0, 255]);
        assert_eq!(&original[60..64], &[0, 0, 255, 255]);
        assert_eq!(
            render(true),
            original,
            "depth-only alpha discard must preserve holes"
        );
    }
}
