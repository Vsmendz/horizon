use horizon_world::{
    chunk::{BlockProperties, BlockState},
    heightmap::HeightmapState,
    nbt::Tag,
};

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VoxelBlock {
    Air = 0,
    GrassBlock = 1,
    Dirt = 2,
    Stone = 3,
    OakLog = 4,
    OakLeaves = 5,
    Water = 6,
    Sand = 7,
    GoldBlock = 8,
    SeaLantern = 9,
    Bedrock = 10,
    #[cfg(feature = "vanilla")]
    Deepslate,
    #[cfg(feature = "vanilla")]
    Gravel,
    #[cfg(feature = "vanilla")]
    Tuff,
    #[cfg(feature = "vanilla")]
    DeepslateIronOre,
    #[cfg(feature = "vanilla")]
    IronOre,
    #[cfg(feature = "vanilla")]
    CoalOre,
    #[cfg(feature = "vanilla")]
    CopperOre,
    #[cfg(feature = "vanilla")]
    Granite,
    #[cfg(feature = "vanilla")]
    Diorite,
    #[cfg(feature = "vanilla")]
    Andesite,
    #[cfg(feature = "vanilla")]
    Sandstone,
    #[cfg(feature = "vanilla")]
    Clay,
    #[cfg(feature = "vanilla")]
    SnowBlock,
    #[cfg(feature = "vanilla")]
    Ice,
    #[cfg(feature = "vanilla")]
    Calcite,
    #[cfg(feature = "vanilla")]
    Lava,
    #[cfg(feature = "vanilla")]
    RedSand,
    #[cfg(feature = "vanilla")]
    Terracotta,
    #[cfg(feature = "vanilla")]
    CoarseDirt,
    #[cfg(feature = "vanilla")]
    Podzol,
    #[cfg(feature = "vanilla")]
    Mud,
    #[cfg(feature = "vanilla")]
    MossBlock,
    #[cfg(feature = "vanilla")]
    DeepslateDiamondOre,
}

impl VoxelBlock {
    pub const ALL: [Self; 11] = [
        Self::Air,
        Self::GrassBlock,
        Self::Dirt,
        Self::Stone,
        Self::OakLog,
        Self::OakLeaves,
        Self::Water,
        Self::Sand,
        Self::GoldBlock,
        Self::SeaLantern,
        Self::Bedrock,
    ];

    pub const ALL_NON_AIR: [Self; 10] = [
        Self::GrassBlock,
        Self::Dirt,
        Self::Stone,
        Self::OakLog,
        Self::OakLeaves,
        Self::Water,
        Self::Sand,
        Self::GoldBlock,
        Self::SeaLantern,
        Self::Bedrock,
    ];

    pub const HOTBAR_BLOCKS: [Self; 9] = [
        Self::GrassBlock,
        Self::Dirt,
        Self::Stone,
        Self::OakLog,
        Self::OakLeaves,
        Self::Sand,
        Self::GoldBlock,
        Self::SeaLantern,
        Self::Water,
    ];

    #[cfg(feature = "vanilla")]
    pub const TERRAIN_MODELS: [Self; 23] = [
        Self::Deepslate,
        Self::Gravel,
        Self::Tuff,
        Self::DeepslateIronOre,
        Self::IronOre,
        Self::CoalOre,
        Self::CopperOre,
        Self::Granite,
        Self::Diorite,
        Self::Andesite,
        Self::Sandstone,
        Self::Clay,
        Self::SnowBlock,
        Self::Ice,
        Self::Calcite,
        Self::Lava,
        Self::RedSand,
        Self::Terracotta,
        Self::CoarseDirt,
        Self::Podzol,
        Self::Mud,
        Self::MossBlock,
        Self::DeepslateDiamondOre,
    ];

    pub fn block_name(self) -> &'static str {
        match self {
            Self::Air => "air",
            Self::GrassBlock => "grass_block",
            Self::Dirt => "dirt",
            Self::Stone => "stone",
            Self::OakLog => "oak_log",
            Self::OakLeaves => "oak_leaves",
            Self::Water => "water",
            Self::Sand => "sand",
            Self::GoldBlock => "gold_block",
            Self::SeaLantern => "sea_lantern",
            Self::Bedrock => "bedrock",
            #[cfg(feature = "vanilla")]
            Self::Deepslate => "deepslate",
            #[cfg(feature = "vanilla")]
            Self::Gravel => "gravel",
            #[cfg(feature = "vanilla")]
            Self::Tuff => "tuff",
            #[cfg(feature = "vanilla")]
            Self::DeepslateIronOre => "deepslate_iron_ore",
            #[cfg(feature = "vanilla")]
            Self::IronOre => "iron_ore",
            #[cfg(feature = "vanilla")]
            Self::CoalOre => "coal_ore",
            #[cfg(feature = "vanilla")]
            Self::CopperOre => "copper_ore",
            #[cfg(feature = "vanilla")]
            Self::Granite => "granite",
            #[cfg(feature = "vanilla")]
            Self::Diorite => "diorite",
            #[cfg(feature = "vanilla")]
            Self::Andesite => "andesite",
            #[cfg(feature = "vanilla")]
            Self::Sandstone => "sandstone",
            #[cfg(feature = "vanilla")]
            Self::Clay => "clay",
            #[cfg(feature = "vanilla")]
            Self::SnowBlock => "snow_block",
            #[cfg(feature = "vanilla")]
            Self::Ice => "ice",
            #[cfg(feature = "vanilla")]
            Self::Calcite => "calcite",
            #[cfg(feature = "vanilla")]
            Self::Lava => "lava",
            #[cfg(feature = "vanilla")]
            Self::RedSand => "red_sand",
            #[cfg(feature = "vanilla")]
            Self::Terracotta => "terracotta",
            #[cfg(feature = "vanilla")]
            Self::CoarseDirt => "coarse_dirt",
            #[cfg(feature = "vanilla")]
            Self::Podzol => "podzol",
            #[cfg(feature = "vanilla")]
            Self::Mud => "mud",
            #[cfg(feature = "vanilla")]
            Self::MossBlock => "moss_block",
            #[cfg(feature = "vanilla")]
            Self::DeepslateDiamondOre => "deepslate_diamond_ore",
        }
    }

    pub fn from_nbt_name(name: &str) -> Self {
        let stripped = name.strip_prefix("minecraft:").unwrap_or(name);
        match stripped {
            "grass_block" => Self::GrassBlock,
            "dirt" => Self::Dirt,
            "stone" => Self::Stone,
            "oak_log" => Self::OakLog,
            "oak_leaves" => Self::OakLeaves,
            "water" => Self::Water,
            "sand" => Self::Sand,
            "gold_block" => Self::GoldBlock,
            "sea_lantern" => Self::SeaLantern,
            "bedrock" => Self::Bedrock,
            #[cfg(feature = "vanilla")]
            "deepslate" => Self::Deepslate,
            #[cfg(feature = "vanilla")]
            "gravel" => Self::Gravel,
            #[cfg(feature = "vanilla")]
            "tuff" => Self::Tuff,
            #[cfg(feature = "vanilla")]
            "deepslate_iron_ore" => Self::DeepslateIronOre,
            #[cfg(feature = "vanilla")]
            "iron_ore" => Self::IronOre,
            #[cfg(feature = "vanilla")]
            "coal_ore" => Self::CoalOre,
            #[cfg(feature = "vanilla")]
            "copper_ore" => Self::CopperOre,
            #[cfg(feature = "vanilla")]
            "granite" => Self::Granite,
            #[cfg(feature = "vanilla")]
            "diorite" => Self::Diorite,
            #[cfg(feature = "vanilla")]
            "andesite" => Self::Andesite,
            #[cfg(feature = "vanilla")]
            "sandstone" => Self::Sandstone,
            #[cfg(feature = "vanilla")]
            "clay" => Self::Clay,
            #[cfg(feature = "vanilla")]
            "snow_block" => Self::SnowBlock,
            #[cfg(feature = "vanilla")]
            "ice" => Self::Ice,
            #[cfg(feature = "vanilla")]
            "calcite" => Self::Calcite,
            #[cfg(feature = "vanilla")]
            "lava" => Self::Lava,
            #[cfg(feature = "vanilla")]
            "red_sand" => Self::RedSand,
            #[cfg(feature = "vanilla")]
            "terracotta" => Self::Terracotta,
            #[cfg(feature = "vanilla")]
            "coarse_dirt" => Self::CoarseDirt,
            #[cfg(feature = "vanilla")]
            "podzol" => Self::Podzol,
            #[cfg(feature = "vanilla")]
            "mud" => Self::Mud,
            #[cfg(feature = "vanilla")]
            "moss_block" => Self::MossBlock,
            #[cfg(feature = "vanilla")]
            "deepslate_diamond_ore" => Self::DeepslateDiamondOre,
            _ => Self::Air,
        }
    }

    pub fn is_solid(self) -> bool {
        #[cfg(feature = "vanilla")]
        if self == Self::Lava {
            return false;
        }
        !matches!(self, Self::Air | Self::Water)
    }

    pub fn is_opaque(self) -> bool {
        #[cfg(feature = "vanilla")]
        if matches!(self, Self::Lava | Self::Ice) {
            return false;
        }
        !matches!(self, Self::Air | Self::Water | Self::OakLeaves)
    }

    pub fn should_cull_against(self, neighbor: Self) -> bool {
        #[cfg(feature = "vanilla")]
        if matches!(self, Self::Lava | Self::Ice) {
            return self == neighbor || neighbor.is_opaque();
        }
        if neighbor == Self::Air {
            return false;
        }
        if self == Self::Water {
            return neighbor == Self::Water || neighbor.is_opaque();
        }
        if self == Self::OakLeaves {
            return neighbor.is_opaque();
        }
        neighbor.is_opaque()
    }

    pub fn biome_tint(self, tint_index: i32) -> [f32; 3] {
        if tint_index < 0 {
            return [1.0, 1.0, 1.0];
        }
        match self {
            Self::GrassBlock => [0.49, 0.78, 0.31],
            Self::OakLeaves => [0.38, 0.72, 0.25],
            Self::Water => [0.25, 0.46, 0.89],
            _ => [1.0, 1.0, 1.0],
        }
    }
}

impl HeightmapState for VoxelBlock {
    fn is_air(self) -> bool {
        self == Self::Air
    }
    fn is_air_block(self) -> bool {
        self == Self::Air
    }
    fn has_fluid(self) -> bool {
        self == Self::Water
    }
    fn blocks_motion_in_heightmap(self) -> bool {
        self.is_solid()
    }
    fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
        self.is_solid() && self != Self::OakLeaves
    }
}

pub(crate) fn block_tag(block: VoxelBlock) -> Tag {
    BlockState {
        name: format!("minecraft:{}", block.block_name()),
        properties: BlockProperties::new(),
    }
    .to_nbt()
}
