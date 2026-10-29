override saturation: f32 = 1.08;
override contrast: f32 = 1.03;
override warmth: f32 = 0.02;

// Constant work per pixel, using the colour the renderer already supplies.
fn post(input: PostInput) -> vec4<f32> {
    let grey = dot(input.color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let saturated = mix(vec3<f32>(grey), input.color.rgb, saturation);
    let graded = (saturated - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
    let warm = graded * vec3<f32>(1.0 + warmth, 1.0, 1.0 - warmth);
    return vec4<f32>(clamp(warm, vec3<f32>(0.0), vec3<f32>(1.0)), input.color.a);
}
