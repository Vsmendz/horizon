//! Complete vanilla BlockState & Model baking catalog indexed by `BlockStateId`.
use crate::{ATLAS_SIDE, ModelQuad, VoxelBlock};
use horizon_assets::{
    bake::{BlockModelTransform, bake_block_model, bake_flat_face},
    blockstate::{
        BlockStateDefinition, Variant, VariantSet,
        bind::{PropertySchema, PropertyValues, SelectedModels},
        choice::CompiledChoices,
    },
    model::{Direction as AssetDirection, Material},
    resolve::{ModelLibrary, ResolvedCuboidModel},
};
use horizon_core::{
    position::BlockPos,
    random::{LegacyRandom, position_seed},
};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadPass {
    Opaque,
    Cutout,
    Translucent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateRenderKind {
    Invisible,
    FluidWater,
    FluidLava,
    EntityProxy,
    Model,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TintKind {
    None,
    BiomeGrass,
    BiomeFoliage,
    BiomeDryFoliage,
    BiomeWater,
    Fixed([f32; 3]),
}

#[derive(Clone, Debug)]
pub struct BakedStateQuad {
    pub quad: ModelQuad,
    pub layer: u32,
    pub pass: QuadPass,
    pub full_face: Option<AssetDirection>,
}

#[derive(Clone, Debug)]
pub struct BakedVariant {
    pub quads: Vec<BakedStateQuad>,
    pub face_mask: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct StateModelMeta {
    pub render_kind: StateRenderKind,
    pub is_opaque_full_cube: bool,
    pub cull_same_state: bool,
    pub has_weighted_random: bool,
    pub tint: TintKind,
}

impl Default for StateModelMeta {
    fn default() -> Self {
        Self {
            render_kind: StateRenderKind::Invisible,
            is_opaque_full_cube: false,
            cull_same_state: false,
            has_weighted_random: false,
            tint: TintKind::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelCoverageStats {
    pub total_states: usize,
    pub bound_states: usize,
    pub model_states: usize,
    pub entity_proxy_states: usize,
    pub fluid_states: usize,
    pub invisible_states: usize,
    pub unique_blocks: usize,
    pub unique_variants: usize,
    pub total_baked_quads: usize,
    pub missing_states: usize,
    pub stone_fallbacks: usize,
}

pub struct StateModelCatalog {
    pub block_choices: Vec<CompiledChoices<u32>>,
    pub state_selections: Box<[Option<(u16, SelectedModels)>]>,
    pub default_variants: Box<[Box<[u32]>]>,
    pub variants: Vec<BakedVariant>,
    pub state_meta: Box<[StateModelMeta]>,
    pub block_default_state: BTreeMap<String, u32>,
    pub coverage: ModelCoverageStats,
}

fn rgb(color: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((color >> shift) & 255) as f32 / 255.0)
}

fn classify_block_render_kind(name: &str) -> Option<StateRenderKind> {
    match name {
        "air" | "cave_air" | "void_air" | "barrier" | "light" | "structure_void"
        | "moving_piston" => Some(StateRenderKind::Invisible),
        "water" | "bubble_column" => Some(StateRenderKind::FluidWater),
        "lava" => Some(StateRenderKind::FluidLava),
        _ => None,
    }
}

fn entity_proxy_bounds(name: &str) -> ([f32; 3], [f32; 3]) {
    if name.ends_with("chest") {
        ([1.0, 0.0, 1.0], [15.0, 14.0, 15.0])
    } else if name.ends_with("_wall_head") || name.ends_with("_wall_skull") {
        ([4.0, 4.0, 8.0], [12.0, 12.0, 16.0])
    } else if name.ends_with("_head") || name.ends_with("_skull") {
        ([4.0, 0.0, 4.0], [12.0, 8.0, 12.0])
    } else if name == "conduit" {
        ([5.0, 5.0, 5.0], [11.0, 11.0, 11.0])
    } else if name == "decorated_pot" {
        ([1.0, 0.0, 1.0], [15.0, 16.0, 15.0])
    } else if name == "end_portal" {
        ([0.0, 0.0, 0.0], [16.0, 12.0, 16.0])
    } else if name.ends_with("_wall_banner") {
        ([2.0, 0.0, 14.0], [14.0, 16.0, 16.0])
    } else if name.ends_with("_banner") {
        ([2.0, 0.0, 7.0], [14.0, 16.0, 9.0])
    } else if name.ends_with("copper_golem_statue") {
        ([3.0, 0.0, 3.0], [13.0, 16.0, 13.0])
    } else {
        ([0.0, 0.0, 0.0], [16.0, 16.0, 16.0])
    }
}

fn is_cull_same_state_block(name: &str) -> bool {
    name == "glass"
        || name == "tinted_glass"
        || name == "ice"
        || name == "frosted_ice"
        || name == "slime_block"
        || name == "honey_block"
        || name == "nether_portal"
        || name.ends_with("_stained_glass")
        || name.ends_with("_stained_glass_pane")
        || name == "glass_pane"
}

fn is_forced_translucent_block(name: &str) -> bool {
    name == "tinted_glass"
        || name == "ice"
        || name == "frosted_ice"
        || name == "slime_block"
        || name == "honey_block"
        || name == "nether_portal"
        || name == "water"
        || name == "bubble_column"
        || name.ends_with("_stained_glass")
        || name.ends_with("_stained_glass_pane")
}

fn compute_state_tint(name: &str, props: &BTreeMap<String, String>) -> TintKind {
    match name {
        "birch_leaves" => TintKind::Fixed(rgb(0x80_a755)),
        "spruce_leaves" => TintKind::Fixed(rgb(0x61_9961)),
        "lily_pad" => TintKind::Fixed(rgb(0x20_8030)),
        "attached_melon_stem" | "attached_pumpkin_stem" => TintKind::Fixed(rgb(0xe0_c71c)),
        "melon_stem" | "pumpkin_stem" => {
            let age = props
                .get("age")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0)
                .min(7);
            TintKind::Fixed([
                (age * 32) as f32 / 255.0,
                (255 - age * 8) as f32 / 255.0,
                (age * 4) as f32 / 255.0,
            ])
        }
        "redstone_wire" => {
            let power = props
                .get("power")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0)
                .min(15);
            let f = power as f32 / 15.0;
            let r = f * 0.6 + if power > 0 { 0.4 } else { 0.3 };
            let g = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
            let b = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
            TintKind::Fixed([r, g, b])
        }
        "water_cauldron" => TintKind::BiomeWater,
        "leaf_litter" => TintKind::BiomeDryFoliage,
        "oak_leaves" | "dark_oak_leaves" | "jungle_leaves" | "acacia_leaves"
        | "mangrove_leaves" | "vine" => TintKind::BiomeFoliage,
        "grass_block" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "potted_fern"
        | "sugar_cane" | "bamboo" | "bamboo_sapling" | "bush" | "pink_petals" | "wildflowers" => {
            TintKind::BiomeGrass
        }
        _ => TintKind::None,
    }
}

fn quad_full_face(positions: &[[f32; 3]; 4], facing: AssetDirection) -> Option<AssetDirection> {
    let (axis, plane, u_axis, v_axis) = match facing {
        AssetDirection::Down => (1, 0.0f32, 0, 2),
        AssetDirection::Up => (1, 1.0f32, 0, 2),
        AssetDirection::North => (2, 0.0f32, 0, 1),
        AssetDirection::South => (2, 1.0f32, 0, 1),
        AssetDirection::West => (0, 0.0f32, 2, 1),
        AssetDirection::East => (0, 1.0f32, 2, 1),
    };
    let mut min_u = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    for p in positions {
        if (p[axis] - plane).abs() > 1e-4 {
            return None;
        }
        let u = p[u_axis];
        let v = p[v_axis];
        if !((u.abs() < 1e-4 || (u - 1.0).abs() < 1e-4)
            && (v.abs() < 1e-4 || (v - 1.0).abs() < 1e-4))
        {
            return None;
        }
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    if min_u <= 1e-4 && max_u >= 1.0 - 1e-4 && min_v <= 1e-4 && max_v >= 1.0 - 1e-4 {
        Some(facing)
    } else {
        None
    }
}

fn inspect_quad_alpha(rgba: &[u8], uv: &[[f32; 2]; 4]) -> (bool, bool) {
    let side = ATLAS_SIDE as usize;
    if rgba.len() < side * side * 4 {
        return (false, false);
    }
    let mut min_u = 1.0f32;
    let mut max_u = 0.0f32;
    let mut min_v = 1.0f32;
    let mut max_v = 0.0f32;
    for &[u, v] in uv {
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    let x0 = ((min_u.clamp(0.0, 1.0) * side as f32).floor() as usize).min(side.saturating_sub(1));
    let x1 = ((max_u.clamp(0.0, 1.0) * side as f32).ceil() as usize)
        .clamp(x0 + 1, side);
    let y0 = ((min_v.clamp(0.0, 1.0) * side as f32).floor() as usize).min(side.saturating_sub(1));
    let y1 = ((max_v.clamp(0.0, 1.0) * side as f32).ceil() as usize)
        .clamp(y0 + 1, side);
    let mut has_zero = false;
    let mut has_partial = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let a = rgba[(y * side + x) * 4 + 3];
            if a == 0 {
                has_zero = true;
            } else if a < 255 {
                has_partial = true;
            }
        }
    }
    (has_zero, has_partial)
}

fn classify_baked_quad(
    quad: ModelQuad,
    block_name: &str,
    sprite_to_layer: &BTreeMap<String, u32>,
    texture_layers: &[Vec<u8>],
) -> Result<BakedStateQuad, String> {
    let layer = *sprite_to_layer
        .get(&quad.material.sprite)
        .ok_or_else(|| format!("block {block_name}: missing sprite {}", quad.material.sprite))?;
    let (has_zero, has_partial) = texture_layers
        .get(layer as usize)
        .map_or((false, false), |rgba| inspect_quad_alpha(rgba, &quad.uv));
    let pass = if quad.material.force_translucent || is_forced_translucent_block(block_name) {
        QuadPass::Translucent
    } else if has_zero || has_partial {
        QuadPass::Cutout
    } else {
        QuadPass::Opaque
    };
    let full_face = if pass == QuadPass::Opaque {
        quad_full_face(&quad.positions, quad.facing)
    } else {
        None
    };
    Ok(BakedStateQuad {
        quad,
        layer,
        pass,
        full_face,
    })
}

fn synthesize_box_quads(
    from: [f32; 3],
    to: [f32; 3],
    material: Material,
    cull_faces: bool,
    shade: bool,
) -> Result<Vec<ModelQuad>, String> {
    let mut out = Vec::with_capacity(6);
    for dir in AssetDirection::ALL {
        let mut q = bake_flat_face(from, to, [0.0, 0.0, 16.0, 16.0], dir, material.clone(), 0)
            .map_err(|e| e.to_string())?;
        if cull_faces {
            q.cull = Some(dir);
        }
        q.shade = shade;
        out.push(q);
    }
    Ok(out)
}

impl StateModelCatalog {
    pub fn bake(
        root: &Path,
        library: &ModelLibrary,
        sprite_to_layer: &BTreeMap<String, u32>,
        texture_layers: &[Vec<u8>],
    ) -> Result<Self, String> {
        let bytes = fs::read(root.join("facts.json")).map_err(|e| e.to_string())?;
        let (registry, _) =
            BlockRegistry::from_facts_json_bytes(&bytes).map_err(|e| e.to_string())?;
        let total_states = registry.len();

        let mut states_by_block: BTreeMap<String, Vec<(u32, BTreeMap<String, String>)>> =
            BTreeMap::new();
        for i in 0..total_states {
            let state = registry
                .state(BlockStateId(i as u32))
                .ok_or_else(|| format!("missing state {i}"))?;
            let short = state
                .name
                .strip_prefix("minecraft:")
                .unwrap_or(&state.name)
                .to_string();
            let props = state
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            states_by_block
                .entry(short)
                .or_default()
                .push((i as u32, props));
        }

        let bundle_path = root.join("assets/blockstates_bundle.json");
        let bs_bundle: Option<BTreeMap<String, String>> = if bundle_path.is_file() {
            let text = fs::read_to_string(&bundle_path).map_err(|e| e.to_string())?;
            Some(serde_json::from_str(&text).map_err(|e| e.to_string())?)
        } else {
            None
        };

        let mut block_choices = Vec::with_capacity(states_by_block.len());
        let mut state_selections: Vec<Option<(u16, SelectedModels)>> = vec![None; total_states];
        let mut default_variants: Vec<Box<[u32]>> = vec![Box::default(); total_states];
        let mut state_meta: Vec<StateModelMeta> = vec![StateModelMeta::default(); total_states];
        let mut block_default_state = BTreeMap::new();
        let mut resolved_cache: BTreeMap<String, ResolvedCuboidModel> = BTreeMap::new();
        let mut variant_lookup: BTreeMap<(String, String, [u8; 3], bool), u32> = BTreeMap::new();
        let mut variants: Vec<BakedVariant> = Vec::new();
        let mut coverage = ModelCoverageStats {
            total_states,
            ..Default::default()
        };

        for (block_name, states) in &states_by_block {
            let bs_text = if let Some(ref bundle) = bs_bundle {
                bundle.get(block_name).cloned()
            } else {
                let p = root.join(format!("assets/minecraft/blockstates/{block_name}.json"));
                p.is_file()
                    .then(|| fs::read_to_string(p).ok())
                    .flatten()
            };
            let Some(bs_text) = bs_text else {
                coverage.missing_states += states.len();
                continue;
            };
            coverage.unique_blocks += 1;

            let full_name = format!("minecraft:{block_name}");
            if let Some(def_id) = registry.default_state(&full_name) {
                block_default_state.insert(block_name.clone(), def_id.0);
            }

            let mut values = BTreeMap::<String, BTreeSet<String>>::new();
            for (_, props) in states {
                for (key, value) in props {
                    values.entry(key.clone()).or_default().insert(value.clone());
                }
            }
            let schema: PropertySchema = values
                .into_iter()
                .map(|(k, v)| (k, PropertyValues::Named(v)))
                .collect();

            let def = BlockStateDefinition::from_json(&bs_text)
                .map_err(|e| format!("BlockStateDefinition({block_name}): {e}"))?;
            let has_weighted_random = def
                .variants
                .iter()
                .flatten()
                .any(|(_, set)| matches!(set, VariantSet::Weighted(list) if list.len() > 1))
                || def
                    .multipart
                    .iter()
                    .flatten()
                    .any(|s| matches!(&s.variant, VariantSet::Weighted(list) if list.len() > 1));

            let prop_list: Vec<_> = states.iter().map(|(_, p)| p.clone()).collect();
            let selections = def
                .bind(&schema, &prop_list)
                .map_err(|e| format!("bind({block_name}): {e}"))?;
            let raw_choices = CompiledChoices::new(&def)
                .map_err(|e| format!("CompiledChoices({block_name}): {e}"))?;

            let special_kind = classify_block_render_kind(block_name);
            let mut block_is_entity_proxy = false;

            let mut bake_err: Option<String> = None;
            let mapped_choices = raw_choices.map(|variant: &Variant| -> u32 {
                if bake_err.is_some() {
                    return 0;
                }
                let key_block = if special_kind.is_some() {
                    block_name.as_str()
                } else {
                    ""
                };
                let key = (
                    key_block.to_string(),
                    variant.model.clone(),
                    variant.rotations,
                    variant.uv_lock,
                );
                if let Some(&idx) = variant_lookup.get(&key) {
                    if variants[idx as usize].quads.is_empty() && special_kind.is_none() {
                        block_is_entity_proxy = true;
                    }
                    return idx;
                }

                let resolved = if let Some(res) = resolved_cache.get(&variant.model) {
                    res.clone()
                } else {
                    match library.resolve(&variant.model) {
                        Ok(res) => {
                            resolved_cache.insert(variant.model.clone(), res.clone());
                            res
                        }
                        Err(e) => {
                            bake_err = Some(format!("resolve({}): {e}", variant.model));
                            return 0;
                        }
                    }
                };

                let mut raw_quads = match bake_block_model(
                    &resolved,
                    BlockModelTransform::new(variant.rotations, variant.uv_lock),
                ) {
                    Ok(q) => q,
                    Err(e) => {
                        bake_err = Some(format!("bake({}): {e}", variant.model));
                        return 0;
                    }
                };

                if raw_quads.is_empty() {
                    match special_kind {
                        Some(StateRenderKind::Invisible | StateRenderKind::FluidWater) => {}
                        Some(StateRenderKind::FluidLava) => {
                            let mat = resolved
                                .material("particle")
                                .ok()
                                .flatten()
                                .cloned()
                                .unwrap_or(Material {
                                    sprite: "minecraft:block/lava_still".to_string(),
                                    force_translucent: false,
                                });
                            match synthesize_box_quads([0.0; 3], [16.0; 3], mat, true, false) {
                                Ok(q) => raw_quads = q,
                                Err(e) => {
                                    bake_err = Some(e);
                                    return 0;
                                }
                            }
                        }
                        _ => {
                            block_is_entity_proxy = true;
                            if let Ok(Some(mat)) = resolved.material("particle").map(|m| m.cloned())
                                && mat.sprite != "minecraft:missingno"
                                && sprite_to_layer.contains_key(&mat.sprite)
                            {
                                let (from, to) = entity_proxy_bounds(block_name);
                                let cull_faces = from == [0.0, 0.0, 0.0] && to == [16.0, 16.0, 16.0];
                                match synthesize_box_quads(from, to, mat, cull_faces, true) {
                                    Ok(q) => raw_quads = q,
                                    Err(e) => {
                                        bake_err = Some(e);
                                        return 0;
                                    }
                                }
                            }
                        }
                    }
                }

                let mut baked_quads = Vec::with_capacity(raw_quads.len());
                let mut face_mask = 0u8;
                for q in raw_quads {
                    match classify_baked_quad(q, block_name, sprite_to_layer, texture_layers) {
                        Ok(bq) => {
                            if let Some(dir) = bq.full_face {
                                face_mask |= 1 << (dir as u8);
                            }
                            baked_quads.push(bq);
                        }
                        Err(e) => {
                            bake_err = Some(e);
                            return 0;
                        }
                    }
                }

                coverage.total_baked_quads += baked_quads.len();
                let idx = variants.len() as u32;
                variants.push(BakedVariant {
                    quads: baked_quads,
                    face_mask,
                });
                variant_lookup.insert(key, idx);
                idx
            });

            if let Some(err) = bake_err {
                return Err(err);
            }

            let block_idx = block_choices.len() as u16;
            let cull_same = is_cull_same_state_block(block_name);

            for ((state_id, props), selection) in states.iter().zip(selections) {
                let selection = selection.ok_or_else(|| {
                    format!("missing model selection for {block_name} state {state_id}")
                })?;
                let mut collected = Vec::new();
                mapped_choices
                    .collect(&selection, &mut LegacyRandom::new(0), &mut collected)
                    .map_err(|e| e.to_string())?;
                let mut combined_face_mask = 0u8;
                let mut total_state_quads = 0usize;
                for &&v_idx in &collected {
                    combined_face_mask |= variants[v_idx as usize].face_mask;
                    total_state_quads += variants[v_idx as usize].quads.len();
                }
                let variant_ids: Box<[u32]> = collected.into_iter().copied().collect();

                let render_kind = if let Some(kind) = special_kind {
                    kind
                } else if block_is_entity_proxy {
                    StateRenderKind::EntityProxy
                } else if total_state_quads == 0 {
                    StateRenderKind::Invisible
                } else {
                    StateRenderKind::Model
                };

                let is_opaque_full_cube = render_kind == StateRenderKind::Model
                    && combined_face_mask == 0b111111
                    && !block_name.ends_with("_leaves")
                    && !matches!(
                        block_name.as_str(),
                        "chorus_flower"
                            | "test_instance_block"
                            | "powder_snow"
                            | "frosted_ice"
                            | "honey_block"
                    );

                let tint = compute_state_tint(block_name, props);

                coverage.bound_states += 1;
                match render_kind {
                    StateRenderKind::Model => coverage.model_states += 1,
                    StateRenderKind::EntityProxy => coverage.entity_proxy_states += 1,
                    StateRenderKind::FluidWater | StateRenderKind::FluidLava => {
                        coverage.fluid_states += 1
                    }
                    StateRenderKind::Invisible => coverage.invisible_states += 1,
                }

                let idx = *state_id as usize;
                state_selections[idx] = Some((block_idx, selection));
                default_variants[idx] = variant_ids;
                state_meta[idx] = StateModelMeta {
                    render_kind,
                    is_opaque_full_cube,
                    cull_same_state: cull_same,
                    has_weighted_random,
                    tint,
                };
            }

            block_choices.push(mapped_choices);
        }

        coverage.unique_variants = variants.len();
        coverage.stone_fallbacks = coverage.missing_states;

        Ok(Self {
            block_choices,
            state_selections: state_selections.into_boxed_slice(),
            default_variants: default_variants.into_boxed_slice(),
            variants,
            state_meta: state_meta.into_boxed_slice(),
            block_default_state,
            coverage,
        })
    }

    #[inline]
    pub fn meta(&self, id: BlockStateId) -> StateModelMeta {
        self.state_meta
            .get(id.0 as usize)
            .copied()
            .unwrap_or_default()
    }

    #[inline]
    pub fn has_state(&self, id: BlockStateId) -> bool {
        self.state_selections
            .get(id.0 as usize)
            .is_some_and(Option::is_some)
    }

    #[inline]
    pub fn is_visible(&self, id: BlockStateId) -> bool {
        self.meta(id).render_kind != StateRenderKind::Invisible
    }

    #[inline]
    pub fn is_opaque_full_cube(&self, id: BlockStateId) -> bool {
        self.meta(id).is_opaque_full_cube
    }

    #[inline]
    pub fn should_cull_face(&self, self_id: BlockStateId, neighbor_id: BlockStateId) -> bool {
        let neighbor_meta = self.meta(neighbor_id);
        if neighbor_meta.is_opaque_full_cube {
            return true;
        }
        self_id == neighbor_id && self.meta(self_id).cull_same_state
    }

    pub fn for_each_quad(
        &self,
        id: BlockStateId,
        pos: BlockPos,
        mut f: impl FnMut(&BakedStateQuad),
    ) {
        let idx = id.0 as usize;
        let Some(&meta) = self.state_meta.get(idx) else {
            return;
        };
        if matches!(
            meta.render_kind,
            StateRenderKind::Invisible | StateRenderKind::FluidWater
        ) {
            return;
        }
        if !meta.has_weighted_random {
            for &v_idx in &self.default_variants[idx] {
                for quad in &self.variants[v_idx as usize].quads {
                    f(quad);
                }
            }
        } else if let Some((block_idx, selection)) = &self.state_selections[idx] {
            let mut rng = LegacyRandom::new(position_seed(pos));
            let _ = self.block_choices[*block_idx as usize].for_each(
                selection,
                &mut rng,
                |&v_idx| {
                    for quad in &self.variants[v_idx as usize].quads {
                        f(quad);
                    }
                },
            );
        }
    }

    pub fn default_quads_for_state(&self, id: BlockStateId) -> Vec<ModelQuad> {
        let mut out = Vec::new();
        if let Some(list) = self.default_variants.get(id.0 as usize) {
            for &v_idx in list.iter() {
                out.extend(
                    self.variants[v_idx as usize]
                        .quads
                        .iter()
                        .map(|bq| bq.quad.clone()),
                );
            }
        }
        out
    }

    pub fn coverage_log_line(&self) -> String {
        let c = &self.coverage;
        format!(
            "MODEL_COVERAGE total_states={} bound_states={} model_states={} entity_proxy_states={} fluid_states={} invisible_states={} unique_blocks={} unique_variants={} baked_quads={} missing_states={} stone_fallbacks={}",
            c.total_states,
            c.bound_states,
            c.model_states,
            c.entity_proxy_states,
            c.fluid_states,
            c.invisible_states,
            c.unique_blocks,
            c.unique_variants,
            c.total_baked_quads,
            c.missing_states,
            c.stone_fallbacks
        )
    }
}

pub(super) fn bake(
    root: &Path,
    library: &ModelLibrary,
    sprite_to_layer: &BTreeMap<String, u32>,
    texture_layers: &[Vec<u8>],
) -> Result<
    (
        BTreeMap<u32, Vec<ModelQuad>>,
        BTreeMap<String, u32>,
        StateModelCatalog,
    ),
    String,
> {
    let catalog = StateModelCatalog::bake(root, library, sprite_to_layer, texture_layers)?;
    let mut quads = BTreeMap::new();
    for id in 0..catalog.default_variants.len() as u32 {
        let state_quads = catalog.default_quads_for_state(BlockStateId(id));
        if !state_quads.is_empty() {
            quads.insert(id, state_quads);
        }
    }
    let defaults = catalog.block_default_state.clone();
    Ok((quads, defaults, catalog))
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
            if !name.ends_with("_log") && !name.contains("mushroom_block") && name != "mushroom_stem"
            {
                continue;
            }
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

    #[test]
    fn full_vanilla_model_catalog_covers_all_35723_states_with_zero_stone_fallbacks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/test11-romfs");
        if !root.join("assets/blockstates_bundle.json").is_file() {
            return;
        }
        let assets = crate::bake_assets(root.display().to_string()).unwrap();
        let catalog = &assets.state_catalog;
        println!("{}", catalog.coverage_log_line());
        assert_eq!(catalog.coverage.total_states, 35723);
        assert_eq!(catalog.coverage.bound_states, 35723);
        assert_eq!(catalog.coverage.unique_blocks, 1286);
        assert_eq!(catalog.coverage.missing_states, 0);
        assert_eq!(catalog.coverage.stone_fallbacks, 0);

        let (registry, _) =
            BlockRegistry::from_facts_json_bytes(&fs::read(root.join("facts.json")).unwrap())
                .unwrap();

        for (block_name, expected_sprite_sub, expected_opaque_cube) in [
            ("minecraft:crafting_table", "crafting_table_front", true),
            ("minecraft:diamond_block", "diamond_block", true),
            ("minecraft:bookshelf", "bookshelf", true),
            ("minecraft:oak_planks", "oak_planks", true),
            ("minecraft:glowstone", "glowstone", true),
            ("minecraft:obsidian", "obsidian", true),
            ("minecraft:poplar_log", "poplar_log", true),
            ("minecraft:pale_oak_log", "pale_oak_log", true),
            ("minecraft:bee_nest", "bee_nest_side", true),
            ("minecraft:furnace", "furnace_front", true),
            ("minecraft:orange_poplar_leaves", "orange_poplar_leaves", false),
            ("minecraft:azalea_leaves", "azalea_leaves", false),
            ("minecraft:pale_oak_leaves", "pale_oak_leaves", false),
            ("minecraft:oak_stairs", "oak_planks", false),
            ("minecraft:stone_slab", "stone", false),
            ("minecraft:smooth_stone_slab", "smooth_stone_slab_side", false),
            ("minecraft:oak_fence", "oak_planks", false),
            ("minecraft:amethyst_cluster", "amethyst_cluster", false),
            ("minecraft:pointed_dripstone", "pointed_dripstone", false),
            ("minecraft:torch", "torch", false),
            ("minecraft:lantern", "lantern", false),
            ("minecraft:red_stained_glass", "red_stained_glass", false),
            ("minecraft:chest", "oak_planks", false),
            ("minecraft:red_shulker_box", "red_shulker_box", false),
        ] {
            let id = registry
                .default_state(block_name)
                .unwrap_or_else(|| panic!("missing default state for {block_name}"));
            let meta = catalog.meta(id);
            assert_eq!(
                meta.is_opaque_full_cube, expected_opaque_cube,
                "{block_name} is_opaque_full_cube mismatch"
            );
            let mut quads = Vec::new();
            catalog.for_each_quad(id, BlockPos::new(0, 64, 0), |q| quads.push(q.clone()));
            assert!(!quads.is_empty(), "{block_name} produced 0 quads");
            assert!(
                quads
                    .iter()
                    .any(|q| q.quad.material.sprite.contains(expected_sprite_sub)),
                "{block_name} quads did not contain {expected_sprite_sub}: {:?}",
                quads
                    .iter()
                    .map(|q| &q.quad.material.sprite)
                    .collect::<Vec<_>>()
            );
            if block_name != "minecraft:stone" && block_name != "minecraft:stone_slab" {
                assert!(
                    quads
                        .iter()
                        .all(|q| q.quad.material.sprite != "minecraft:block/stone"),
                    "{block_name} falsely used stone sprite"
                );
            }
            if block_name == "minecraft:red_stained_glass" {
                assert!(quads.iter().all(|q| q.pass == QuadPass::Translucent));
                assert!(catalog.should_cull_face(id, id));
            }
            if block_name.ends_with("_leaves")
                || block_name == "minecraft:amethyst_cluster"
                || block_name == "minecraft:pointed_dripstone"
            {
                assert!(
                    quads.iter().any(|q| q.pass == QuadPass::Cutout),
                    "{block_name} expected Cutout quad"
                );
            }
            if block_name != "minecraft:red_stained_glass" {
                assert!(
                    quads.iter().all(|q| q.pass != QuadPass::Translucent),
                    "{block_name} unexpectedly had Translucent quad"
                );
            }
        }
    }
}
