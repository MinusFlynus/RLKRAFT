//! Lightweight native material acoustics imported from Presence Footsteps.
//!
//! This intentionally operates on the vanilla step sound event which Horizon
//! has already resolved for the block state. That makes it O(1), allocation
//! free, and naturally covers every block sharing a vanilla sound type. The
//! optional second voice models the mod's simultaneous grass/brush acoustic.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub event: &'static str,
    pub volume: (f32, f32),
    pub pitch: (f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Acoustic {
    pub primary: Voice,
    pub secondary: Option<Voice>,
}

const STANDARD_PITCH: (f32, f32) = (0.95, 1.05);

const fn voice(event: &'static str, volume: (f32, f32), pitch: (f32, f32)) -> Voice {
    Voice {
        event,
        volume,
        pitch,
    }
}

const fn one(event: &'static str, volume: (f32, f32), pitch: (f32, f32)) -> Acoustic {
    Acoustic {
        primary: voice(event, volume, pitch),
        secondary: None,
    }
}

/// Presence Footsteps' `walk` acoustic for a vanilla block step event.
/// Unknown and specialist materials keep Horizon's vanilla sound.
pub fn acoustic(event: &str) -> Option<Acoustic> {
    let path = event.strip_prefix("minecraft:").unwrap_or(event);
    let group = path.strip_prefix("block.")?.strip_suffix(".step")?;
    Some(match group {
        "grass" | "moss" | "roots" => Acoustic {
            primary: voice(
                "presencefootsteps:pf_presence.grass.grass_walk",
                (0.2, 0.6),
                STANDARD_PITCH,
            ),
            secondary: Some(voice(
                "presencefootsteps:pf_presence.brush.brush_through",
                (0.2, 0.2),
                STANDARD_PITCH,
            )),
        },
        "dirt" | "rooted_dirt" => one(
            "presencefootsteps:pf_presence.dirt.dirt_walk",
            (0.9, 1.0),
            STANDARD_PITCH,
        ),
        "gravel" => one(
            "presencefootsteps:pf_presence.gravel.gravel_walk",
            (0.9, 1.0),
            STANDARD_PITCH,
        ),
        "sand" | "suspicious_sand" => one(
            "presencefootsteps:pf_presence.sand.sand_walk",
            (0.6, 0.6),
            STANDARD_PITCH,
        ),
        "snow" | "powder_snow" => one(
            "presencefootsteps:pf_presence.snow.snow_walk",
            (0.9, 1.0),
            STANDARD_PITCH,
        ),
        "mud" | "mud_bricks" => one(
            "presencefootsteps:pf_presence.mud.mud_walk",
            (0.9, 1.0),
            STANDARD_PITCH,
        ),
        "wool" => one(
            "presencefootsteps:pf_presence.rug.rug_walk",
            (0.7, 0.7),
            STANDARD_PITCH,
        ),
        "wood" | "cherry_wood" | "bamboo_wood" | "nether_wood" => one(
            "presencefootsteps:pf_presence.wood.wood_walk",
            (0.9, 1.0),
            STANDARD_PITCH,
        ),
        "metal" | "netherite_block" | "lantern" | "iron" => one(
            "presencefootsteps:pf_presence.metalbox.metalbox_walk",
            (0.9, 1.0),
            STANDARD_PITCH,
        ),
        "stone"
        | "deepslate"
        | "tuff"
        | "basalt"
        | "netherrack"
        | "nether_bricks"
        | "blackstone"
        | "polished_blackstone" => one(
            "presencefootsteps:pf_presence.stone.stone_walk",
            (0.9, 1.0),
            (0.65, 0.7),
        ),
        _ => return None,
    })
}

/// Play the imported acoustic and return true, or leave an unsupported event
/// to the caller's vanilla fallback. The global user volume is applied after
/// each acoustic file's own percentage range, as Presence Footsteps does.
pub fn play(
    audio: &mut horizon_client_render::audio::Audio,
    event: &str,
    at: [f64; 3],
    global_volume: f32,
) -> bool {
    let Some(acoustic) = acoustic(event) else {
        return false;
    };
    for voice in [Some(acoustic.primary), acoustic.secondary]
        .into_iter()
        .flatten()
    {
        audio.play_varied(
            voice.event,
            Some(at),
            voice.volume,
            voice.pitch,
            global_volume,
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_materials_and_keeps_unknown_steps_vanilla() {
        let grass = acoustic("minecraft:block.grass.step").unwrap();
        assert_eq!(
            grass.primary.event,
            "presencefootsteps:pf_presence.grass.grass_walk"
        );
        assert_eq!(
            grass.secondary.unwrap().event,
            "presencefootsteps:pf_presence.brush.brush_through"
        );
        assert_eq!(
            acoustic("minecraft:block.stone.step")
                .unwrap()
                .primary
                .pitch,
            (0.65, 0.7)
        );
        assert!(acoustic("minecraft:block.glass.step").is_none());
        assert!(acoustic("minecraft:entity.player.hurt").is_none());
    }
}

