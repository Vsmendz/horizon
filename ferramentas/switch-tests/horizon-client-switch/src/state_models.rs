//! Use the original registry selector for state-dependent woodland models.
use crate::{ModelQuad, VoxelBlock};
use horizon_assets::{
    bake::{BlockModelTransform, bake_block_model},
    blockstate::{
        BlockStateDefinition,
        bind::{PropertySchema, PropertyValues},
        choice::CompiledChoices,
    },
    resolve::ModelLibrary,
};
use horizon_core::random::LegacyRandom;
use horizon_world::registry::{BlockRegistry, BlockStateId};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(super) fn bake(
    root: &Path,
    library: &ModelLibrary,
) -> Result<(BTreeMap<u32, Vec<ModelQuad>>, BTreeMap<String, u32>), String> {
    let bytes = fs::read(root.join("facts.json")).map_err(|e| e.to_string())?;
    let (registry, _) = BlockRegistry::from_facts_json_bytes(&bytes).map_err(|e| e.to_string())?;
    let mut quads = BTreeMap::new();
    let mut defaults = BTreeMap::new();
    for block in VoxelBlock::VISUAL_MODELS
        .into_iter()
        .chain([VoxelBlock::OakLog])
    {
        let name = block.block_name();
        if !name.ends_with("_log") && !name.contains("mushroom_block") && name != "mushroom_stem" {
            continue;
        }
        let full = format!("minecraft:{name}");
        let states: Vec<_> = (0..registry.len())
            .filter_map(|i| {
                let state = registry.state(BlockStateId(i as u32))?;
                (state.name == full).then_some((
                    i as u32,
                    state
                        .properties
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<BTreeMap<_, _>>(),
                ))
            })
            .collect();
        let mut values = BTreeMap::<String, BTreeSet<String>>::new();
        for (_, props) in &states {
            for (key, value) in props {
                values.entry(key.clone()).or_default().insert(value.clone());
            }
        }
        let schema: PropertySchema = values
            .into_iter()
            .map(|(k, v)| (k, PropertyValues::Named(v)))
            .collect();
        let text =
            fs::read_to_string(root.join(format!("assets/minecraft/blockstates/{name}.json")))
                .map_err(|e| e.to_string())?;
        let def = BlockStateDefinition::from_json(&text).map_err(|e| e.to_string())?;
        let selections = def
            .bind(
                &schema,
                &states.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
        let choices = CompiledChoices::new(&def).map_err(|e| e.to_string())?;
        for ((id, _), selection) in states.into_iter().zip(selections) {
            let selection =
                selection.ok_or_else(|| format!("missing model for {full} state {id}"))?;
            let mut variants = Vec::new();
            choices
                .collect(&selection, &mut LegacyRandom::new(0), &mut variants)
                .map_err(|e| e.to_string())?;
            let mut baked = Vec::new();
            for variant in variants {
                let model = library.resolve(&variant.model).map_err(|e| e.to_string())?;
                baked.extend(
                    bake_block_model(
                        &model,
                        BlockModelTransform::new(variant.rotations, variant.uv_lock),
                    )
                    .map_err(|e| e.to_string())?,
                );
            }
            quads.insert(id, baked);
        }
        defaults.insert(
            name.to_string(),
            registry
                .default_state(&full)
                .ok_or("missing default state")?
                .0,
        );
    }
    Ok((quads, defaults))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn woodland_states_keep_materials_and_mushroom_face_variants() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/test10-romfs");
        let assets = crate::bake_assets(root.display().to_string()).unwrap();
        let (registry, _) =
            BlockRegistry::from_facts_json_bytes(&fs::read(root.join("facts.json")).unwrap())
                .unwrap();
        for i in 0..registry.len() {
            let state = registry.state(BlockStateId(i as u32)).unwrap();
            let name = state.name.strip_prefix("minecraft:").unwrap();
            if let Some(quads) = assets.state_quads.get(&(i as u32)) {
                assert_eq!(quads.len(), 6, "{state:?}");
                assert_ne!(VoxelBlock::from_nbt_name(name), VoxelBlock::Air, "{name}");
                assert!(
                    quads.iter().all(|q| !q.material.sprite.contains("stone")),
                    "{state:?}"
                );
                for q in quads {
                    assert!(assets.sprite_to_layer.contains_key(&q.material.sprite));
                    if name.ends_with("_log") {
                        use horizon_assets::model::Direction::*;
                        let end_face = match state.properties.get("axis").map(String::as_str) {
                            Some("x") => matches!(q.facing, West | East),
                            Some("y") => matches!(q.facing, Up | Down),
                            Some("z") => matches!(q.facing, North | South),
                            _ => panic!("log without axis: {state:?}"),
                        };
                        assert_eq!(q.material.sprite.ends_with("_top"), end_face, "{state:?}");
                    }
                }
                if name == "brown_mushroom_block" && state.properties.values().all(|v| v == "false")
                {
                    assert!(
                        quads
                            .iter()
                            .all(|q| q.material.sprite.ends_with("mushroom_block_inside"))
                    );
                }
                if name == "brown_mushroom_block" && state.properties.values().all(|v| v == "true")
                {
                    assert!(
                        quads
                            .iter()
                            .all(|q| q.material.sprite.ends_with("brown_mushroom_block"))
                    );
                }
            }
        }
        for block in VoxelBlock::VISUAL_MODELS
            .into_iter()
            .chain([VoxelBlock::OakLeaves])
        {
            if block.block_name().ends_with("_leaves") {
                let quads = &assets.baked_quads[&block];
                assert_eq!(quads.len(), 6);
                for quad in quads {
                    assert!(quad.material.sprite.ends_with(block.block_name()));
                    assert!(assets.sprite_to_layer.contains_key(&quad.material.sprite));
                }
            }
        }
        assert!(assets.state_quads.len() >= 192);
    }
}
