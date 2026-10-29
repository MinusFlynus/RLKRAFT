//! What a mod jar holds, read without decompiling: its metadata
//! (`neoforge.mods.toml`, `mods.toml`, `fabric.mod.json`, `mcmod.info`) and a census of
//! its data, assets and classes.
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct JarInfo {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub license: String,
    /// Required dependencies by mod id (loaders and `minecraft` left out).
    pub depends: Vec<String>,
    pub census: Census,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Census {
    pub blockstates: usize,
    pub item_models: usize,
    pub recipes: usize,
    pub loot_tables: usize,
    pub tags: usize,
    pub worldgen: usize,
    pub structures: usize,
    pub languages: usize,
    pub textures: usize,
    pub sounds: usize,
    pub classes: usize,
    pub mixin_configs: usize,
}

const NOT_MODS: [&str; 5] = ["minecraft", "neoforge", "forge", "fabricloader", "java"];

pub fn read(path: &Path) -> Result<JarInfo, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = |zip: &mut zip::ZipArchive<fs::File>, name: &str| -> Option<String> {
        let mut s = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut s).ok()?;
        Some(s)
    };
    let mut info = if let Some(t) = text(&mut zip, "META-INF/neoforge.mods.toml")
        .or_else(|| text(&mut zip, "META-INF/mods.toml"))
    {
        mods_toml(&t)?
    } else if let Some(t) = text(&mut zip, "fabric.mod.json") {
        fabric(&t)?
    } else if let Some(t) = text(&mut zip, "mcmod.info") {
        // Legacy Forge accepts some non-JSON metadata. Keep the file census
        // and filename fallback even when this inventory cannot decode it.
        mcmod_info(&t).unwrap_or_default()
    } else {
        JarInfo::default()
    };
    // `version = "${file.jarVersion}"` is filled in from the manifest at load.
    if info.version.starts_with("${") {
        info.version = text(&mut zip, "META-INF/MANIFEST.MF")
            .and_then(|m| manifest_value(&m, "Implementation-Version"))
            .unwrap_or_default();
    }
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    info.census = census(&names);
    if info.mod_id.is_empty() {
        info.mod_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    Ok(info)
}

/// A main-section attribute of `META-INF/MANIFEST.MF`.
fn manifest_value(manifest: &str, key: &str) -> Option<String> {
    manifest
        .lines()
        .take_while(|l| !l.trim().is_empty())
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// `[[mods]]` (the first one is the jar's mod), the top-level `license`, and
/// `[[dependencies.<id>]]` with `type = "required"` (or the older
/// `mandatory = true`).
pub fn mods_toml(text: &str) -> Result<JarInfo, String> {
    let doc: toml::Table = toml::from_str(text).map_err(|e| format!("mods.toml: {e}"))?;
    let first = doc
        .get("mods")
        .and_then(|m| m.as_array())
        .and_then(|m| m.first())
        .and_then(|m| m.as_table());
    let get = |t: Option<&toml::Table>, k: &str| {
        t.and_then(|t| t.get(k))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    let mod_id = get(first, "modId");
    let mut depends = Vec::new();
    if let Some(deps) = doc
        .get("dependencies")
        .and_then(|d| d.get(&mod_id))
        .and_then(|d| d.as_array())
    {
        for d in deps.iter().filter_map(|d| d.as_table()) {
            let required = d.get("type").and_then(|t| t.as_str()).map_or_else(
                || d.get("mandatory").and_then(|m| m.as_bool()) == Some(true),
                |t| t.eq_ignore_ascii_case("required"),
            );
            let id = get(Some(d), "modId");
            if required && !NOT_MODS.contains(&id.as_str()) && !id.is_empty() {
                depends.push(id);
            }
        }
    }
    Ok(JarInfo {
        name: get(first, "displayName"),
        version: get(first, "version"),
        license: get(Some(&doc), "license"),
        mod_id,
        depends,
        census: Census::default(),
    })
}

pub fn fabric(text: &str) -> Result<JarInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("fabric.mod.json: {e}"))?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    let license = match &v["license"] {
        serde_json::Value::Array(l) => l
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(" OR "),
        l => l.as_str().unwrap_or("").to_owned(),
    };
    let depends = v["depends"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, _)| id.clone())
        .filter(|id| !NOT_MODS.contains(&id.as_str()) && !id.starts_with("fabric"))
        .collect();
    Ok(JarInfo {
        mod_id: s("id"),
        name: s("name"),
        version: s("version"),
        license,
        depends,
        census: Census::default(),
    })
}

/// Forge 1.12 metadata, in either the array or version-2 `modList` form.
/// As with modern metadata, the first entry supplies this jar's identity.
/// Only explicitly enabled hard dependencies are recorded. `dependencies`
/// expresses optional load ordering, and annotations may supply other facts
/// that this metadata-only inventory cannot recover.
pub fn mcmod_info(text: &str) -> Result<JarInfo, String> {
    let text = text.trim_start_matches('\u{feff}');
    let doc: serde_json::Value = serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&legacy_control_characters(text)))
        .map_err(|e| format!("mcmod.info: {e}"))?;
    let entries = doc.as_array().or_else(|| doc["modList"].as_array());
    let first = entries
        .and_then(|entries| entries.first())
        .filter(|entry| entry.is_object())
        .ok_or("mcmod.info: expected a nonempty mod array or modList")?;
    let s = |key: &str| first[key].as_str().unwrap_or("").to_owned();
    let mut depends = Vec::new();
    if first["useDependencyInformation"].as_bool() == Some(true) {
        for requirement in first["requiredMods"].as_array().into_iter().flatten() {
            let Some(requirement) = requirement.as_str() else {
                continue;
            };
            let id = requirement.split('@').next().unwrap_or("").trim();
            if !id.is_empty() && !NOT_MODS.contains(&id) && !depends.iter().any(|d| d == id) {
                depends.push(id.to_owned());
            }
        }
    }
    Ok(JarInfo {
        mod_id: s("modid"),
        name: s("name"),
        version: s("version"),
        depends,
        ..JarInfo::default()
    })
}

/// Some shipped Forge metadata uses literal newlines in description strings.
/// Preserve those characters as JSON escapes, without changing token syntax
/// or interpreting unquoted values. This is not a general lenient JSON parser.
fn legacy_control_characters(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(text.len());
    let (mut quoted, mut escaped) = (false, false);
    for ch in text.chars() {
        if quoted && !escaped && ch <= '\u{1f}' {
            write!(out, "\\u{:04x}", ch as u32).unwrap();
            continue;
        }
        if escaped {
            escaped = false;
        } else if quoted && ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        }
        out.push(ch);
    }
    out
}

pub fn census(names: &[String]) -> Census {
    let mut c = Census::default();
    for n in names {
        let parts: Vec<&str> = n.split('/').collect();
        if n.ends_with(".class") {
            c.classes += 1;
        } else if n.ends_with(".mixins.json") || (parts.len() == 1 && n.contains("mixins")) {
            c.mixin_configs += 1;
        }
        // assets/<ns>/<kind>/... and data/<ns>/<kind>/...
        let (Some(root), Some(kind)) = (parts.first(), parts.get(2)) else {
            continue;
        };
        let json = n.ends_with(".json");
        match (*root, *kind) {
            ("assets", "blockstates") if json => c.blockstates += 1,
            ("assets", "models") if json && parts.get(3) == Some(&"item") => c.item_models += 1,
            ("assets", "lang") => c.languages += 1,
            ("assets", "textures") if n.ends_with(".png") => c.textures += 1,
            ("assets", "sounds.json") => c.sounds += 1,
            ("data", "recipe" | "recipes") if json => c.recipes += 1,
            ("data", "loot_table" | "loot_tables") if json => c.loot_tables += 1,
            ("data", "tags") if json => c.tags += 1,
            ("data", "worldgen") if json => c.worldgen += 1,
            ("data", "structure" | "structures") if n.ends_with(".nbt") => c.structures += 1,
            ("assets", "recipes") if json => c.recipes += 1,
            ("assets", "loot_tables") if json => c.loot_tables += 1,
            ("assets", "structures") if n.ends_with(".nbt") => c.structures += 1,
            _ => {}
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_neoforge_metadata() {
        let info = mods_toml(
            r#"modLoader = "javafml"
license = "MIT"
[[mods]]
modId = "ruby"
version = "1.2.0"
displayName = "Ruby"
[[dependencies.ruby]]
modId = "neoforge"
type = "required"
[[dependencies.ruby]]
modId = "geckolib"
type = "required"
[[dependencies.ruby]]
modId = "jei"
type = "optional"
[[dependencies.ruby]]
modId = "curios"
mandatory = true
"#,
        )
        .unwrap();
        assert_eq!(
            (
                info.mod_id.as_str(),
                info.version.as_str(),
                info.license.as_str()
            ),
            ("ruby", "1.2.0", "MIT")
        );
        assert_eq!(info.depends, ["geckolib", "curios"]);
    }

    #[test]
    fn reads_fabric_metadata() {
        let info = fabric(
            r#"{"id": "gem", "version": "3", "name": "Gem", "license": ["MIT", "Apache-2.0"],
                "depends": {"fabricloader": "*", "fabric-api": "*", "minecraft": "1.21.1", "cloth-config": "*"}}"#,
        )
        .unwrap();
        assert_eq!(info.license, "MIT OR Apache-2.0");
        assert_eq!(info.depends, ["cloth-config"]);
    }

    #[test]
    fn reads_legacy_forge_array_without_promoting_soft_dependencies() {
        let info = mcmod_info(
            r#"[{"modid":"survival", "name":"Survival", "version":"1.12.2-1.0",
            "useDependencyInformation":true,
            "requiredMods":["forge@[14.23.5.2860,)","baubles@[1.5.1,)","baubles"],
            "dependencies":["baubles","optional_lighting"]}]"#,
        )
        .unwrap();
        assert_eq!(info.mod_id, "survival");
        assert_eq!(info.name, "Survival");
        assert_eq!(info.version, "1.12.2-1.0");
        assert_eq!(info.depends, ["baubles"]);
        assert!(info.license.is_empty());
    }

    #[test]
    fn reads_legacy_forge_wrapped_metadata_and_ignores_disabled_dependencies() {
        let info = mcmod_info(
            "\u{feff}{\"modListVersion\":2,\"modList\":[{\"modid\":\"survival\",\"requiredMods\":[\"unused\"]}]}",
        ).unwrap();
        assert_eq!(info.mod_id, "survival");
        assert!(info.depends.is_empty());
        for invalid in ["[]", "{}", "{\"modList\":[]}", "[null]", "invalid"] {
            assert!(mcmod_info(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn preserves_literal_control_characters_in_legacy_strings() {
        let info =
            mcmod_info("[{\"modid\":\"survival\",\"name\":\"Line one\nLine two\tend\"}]").unwrap();
        assert_eq!(info.name, "Line one\nLine two\tend");
        let valid = r#"[{"modid":"survival","name":"A \"quoted\" name"}]"#;
        assert_eq!(legacy_control_characters(valid), valid);
        assert_eq!(mcmod_info(valid).unwrap().name, "A \"quoted\" name");
        assert!(mcmod_info(r#"[{"modid":unquoted}]"#).is_err());
    }

    #[test]
    fn reads_legacy_jar_and_resolves_manifest_version() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!(
            "horizon-legacy-{}-{}.jar",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, text) in [
            (
                "mcmod.info",
                r#"[{"modid":"survival","version":"${version}"}]"#,
            ),
            (
                "META-INF/MANIFEST.MF",
                "Manifest-Version: 1.0\nImplementation-Version: 2.3.4\n\n",
            ),
            ("assets/survival/recipes/bandage.json", "{}"),
            ("example/Survival.class", "fixture, not executable bytecode"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let result = read(&path);
        fs::remove_file(&path).unwrap();
        let info = result.unwrap();
        assert_eq!(info.mod_id, "survival");
        assert_eq!(info.version, "2.3.4");
        assert_eq!((info.census.recipes, info.census.classes), (1, 1));
    }

    #[test]
    fn counts_legacy_forge_content_without_claiming_it_is_portable() {
        let names = [
            "assets/survival/recipes/bandage.json",
            "assets/survival/loot_tables/chests/tower.json",
            "assets/survival/structures/tower.nbt",
            "assets/survival/lang/en_us.lang",
            "assets/survival/recipes/readme.txt",
        ]
        .map(String::from);
        let c = census(&names);
        assert_eq!(
            (c.recipes, c.loot_tables, c.structures, c.languages),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn counts_files_by_kind() {
        let names = [
            "assets/ruby/blockstates/ruby_block.json",
            "assets/ruby/models/item/ruby.json",
            "assets/ruby/models/block/ruby_block.json",
            "assets/ruby/textures/item/ruby.png",
            "assets/ruby/lang/en_us.json",
            "data/ruby/recipe/ruby.json",
            "data/ruby/loot_table/blocks/ruby_block.json",
            "data/minecraft/tags/block/mineable/pickaxe.json",
            "data/ruby/structure/hut.nbt",
            "com/example/Ruby.class",
            "ruby.mixins.json",
        ]
        .map(String::from);
        let c = census(&names);
        assert_eq!(
            (c.blockstates, c.item_models, c.textures, c.languages),
            (1, 1, 1, 1)
        );
        assert_eq!(
            (c.recipes, c.loot_tables, c.tags, c.structures),
            (1, 1, 1, 1)
        );
        assert_eq!((c.classes, c.mixin_configs), (1, 1));
    }
}
