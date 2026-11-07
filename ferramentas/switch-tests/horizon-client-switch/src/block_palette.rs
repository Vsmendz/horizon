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
    #[cfg(feature = "vanilla-visuals")]
    BirchLog,
    #[cfg(feature = "vanilla-visuals")]
    BirchLeaves,
    #[cfg(feature = "vanilla-visuals")]
    SpruceLog,
    #[cfg(feature = "vanilla-visuals")]
    SpruceLeaves,
    #[cfg(feature = "vanilla-visuals")]
    DarkOakLog,
    #[cfg(feature = "vanilla-visuals")]
    DarkOakLeaves,
    #[cfg(feature = "vanilla-visuals")]
    JungleLog,
    #[cfg(feature = "vanilla-visuals")]
    JungleLeaves,
    #[cfg(feature = "vanilla-visuals")]
    AcaciaLog,
    #[cfg(feature = "vanilla-visuals")]
    AcaciaLeaves,
    #[cfg(feature = "vanilla-visuals")]
    CherryLog,
    #[cfg(feature = "vanilla-visuals")]
    CherryLeaves,
    #[cfg(feature = "vanilla-visuals")]
    MangroveLog,
    #[cfg(feature = "vanilla-visuals")]
    MangroveLeaves,
    #[cfg(feature = "vanilla-visuals")]
    OakSapling,
    #[cfg(feature = "vanilla-visuals")]
    ShortGrass,
    #[cfg(feature = "vanilla-visuals")]
    Fern,
    #[cfg(feature = "vanilla-visuals")]
    DeadBush,
    #[cfg(feature = "vanilla-visuals")]
    Dandelion,
    #[cfg(feature = "vanilla-visuals")]
    Poppy,
    #[cfg(feature = "vanilla-visuals")]
    BlueOrchid,
    #[cfg(feature = "vanilla-visuals")]
    Allium,
    #[cfg(feature = "vanilla-visuals")]
    AzureBluet,
    #[cfg(feature = "vanilla-visuals")]
    OxeyeDaisy,
    #[cfg(feature = "vanilla-visuals")]
    Cornflower,
    #[cfg(feature = "vanilla-visuals")]
    LilyOfTheValley,
    #[cfg(feature = "vanilla-visuals")]
    BrownMushroom,
    #[cfg(feature = "vanilla-visuals")]
    RedMushroom,
    #[cfg(feature = "vanilla-visuals")]
    SugarCane,
    #[cfg(feature = "vanilla-visuals")]
    LilyPad,
    #[cfg(feature = "vanilla-visuals")]
    Cactus,
    #[cfg(feature = "vanilla-visuals")]
    Glass,
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

    #[cfg(feature = "vanilla-visuals")]
    pub const VISUAL_MODELS: [Self; 32] = [
        Self::BirchLog,
        Self::BirchLeaves,
        Self::SpruceLog,
        Self::SpruceLeaves,
        Self::DarkOakLog,
        Self::DarkOakLeaves,
        Self::JungleLog,
        Self::JungleLeaves,
        Self::AcaciaLog,
        Self::AcaciaLeaves,
        Self::CherryLog,
        Self::CherryLeaves,
        Self::MangroveLog,
        Self::MangroveLeaves,
        Self::OakSapling,
        Self::ShortGrass,
        Self::Fern,
        Self::DeadBush,
        Self::Dandelion,
        Self::Poppy,
        Self::BlueOrchid,
        Self::Allium,
        Self::AzureBluet,
        Self::OxeyeDaisy,
        Self::Cornflower,
        Self::LilyOfTheValley,
        Self::BrownMushroom,
        Self::RedMushroom,
        Self::SugarCane,
        Self::LilyPad,
        Self::Cactus,
        Self::Glass,
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
            #[cfg(feature = "vanilla-visuals")]
            Self::BirchLog => "birch_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::BirchLeaves => "birch_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::SpruceLog => "spruce_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::SpruceLeaves => "spruce_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::DarkOakLog => "dark_oak_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::DarkOakLeaves => "dark_oak_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::JungleLog => "jungle_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::JungleLeaves => "jungle_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::AcaciaLog => "acacia_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::AcaciaLeaves => "acacia_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::CherryLog => "cherry_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::CherryLeaves => "cherry_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::MangroveLog => "mangrove_log",
            #[cfg(feature = "vanilla-visuals")]
            Self::MangroveLeaves => "mangrove_leaves",
            #[cfg(feature = "vanilla-visuals")]
            Self::OakSapling => "oak_sapling",
            #[cfg(feature = "vanilla-visuals")]
            Self::ShortGrass => "short_grass",
            #[cfg(feature = "vanilla-visuals")]
            Self::Fern => "fern",
            #[cfg(feature = "vanilla-visuals")]
            Self::DeadBush => "dead_bush",
            #[cfg(feature = "vanilla-visuals")]
            Self::Dandelion => "dandelion",
            #[cfg(feature = "vanilla-visuals")]
            Self::Poppy => "poppy",
            #[cfg(feature = "vanilla-visuals")]
            Self::BlueOrchid => "blue_orchid",
            #[cfg(feature = "vanilla-visuals")]
            Self::Allium => "allium",
            #[cfg(feature = "vanilla-visuals")]
            Self::AzureBluet => "azure_bluet",
            #[cfg(feature = "vanilla-visuals")]
            Self::OxeyeDaisy => "oxeye_daisy",
            #[cfg(feature = "vanilla-visuals")]
            Self::Cornflower => "cornflower",
            #[cfg(feature = "vanilla-visuals")]
            Self::LilyOfTheValley => "lily_of_the_valley",
            #[cfg(feature = "vanilla-visuals")]
            Self::BrownMushroom => "brown_mushroom",
            #[cfg(feature = "vanilla-visuals")]
            Self::RedMushroom => "red_mushroom",
            #[cfg(feature = "vanilla-visuals")]
            Self::SugarCane => "sugar_cane",
            #[cfg(feature = "vanilla-visuals")]
            Self::LilyPad => "lily_pad",
            #[cfg(feature = "vanilla-visuals")]
            Self::Cactus => "cactus",
            #[cfg(feature = "vanilla-visuals")]
            Self::Glass => "glass",
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
            #[cfg(feature = "vanilla-visuals")]
            "birch_log" => Self::BirchLog,
            #[cfg(feature = "vanilla-visuals")]
            "birch_leaves" => Self::BirchLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "spruce_log" => Self::SpruceLog,
            #[cfg(feature = "vanilla-visuals")]
            "spruce_leaves" => Self::SpruceLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "dark_oak_log" => Self::DarkOakLog,
            #[cfg(feature = "vanilla-visuals")]
            "dark_oak_leaves" => Self::DarkOakLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "jungle_log" => Self::JungleLog,
            #[cfg(feature = "vanilla-visuals")]
            "jungle_leaves" => Self::JungleLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "acacia_log" => Self::AcaciaLog,
            #[cfg(feature = "vanilla-visuals")]
            "acacia_leaves" => Self::AcaciaLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "cherry_log" => Self::CherryLog,
            #[cfg(feature = "vanilla-visuals")]
            "cherry_leaves" => Self::CherryLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "mangrove_log" => Self::MangroveLog,
            #[cfg(feature = "vanilla-visuals")]
            "mangrove_leaves" => Self::MangroveLeaves,
            #[cfg(feature = "vanilla-visuals")]
            "oak_sapling" => Self::OakSapling,
            #[cfg(feature = "vanilla-visuals")]
            "short_grass" => Self::ShortGrass,
            #[cfg(feature = "vanilla-visuals")]
            "fern" => Self::Fern,
            #[cfg(feature = "vanilla-visuals")]
            "dead_bush" => Self::DeadBush,
            #[cfg(feature = "vanilla-visuals")]
            "dandelion" => Self::Dandelion,
            #[cfg(feature = "vanilla-visuals")]
            "poppy" => Self::Poppy,
            #[cfg(feature = "vanilla-visuals")]
            "blue_orchid" => Self::BlueOrchid,
            #[cfg(feature = "vanilla-visuals")]
            "allium" => Self::Allium,
            #[cfg(feature = "vanilla-visuals")]
            "azure_bluet" => Self::AzureBluet,
            #[cfg(feature = "vanilla-visuals")]
            "oxeye_daisy" => Self::OxeyeDaisy,
            #[cfg(feature = "vanilla-visuals")]
            "cornflower" => Self::Cornflower,
            #[cfg(feature = "vanilla-visuals")]
            "lily_of_the_valley" => Self::LilyOfTheValley,
            #[cfg(feature = "vanilla-visuals")]
            "brown_mushroom" => Self::BrownMushroom,
            #[cfg(feature = "vanilla-visuals")]
            "red_mushroom" => Self::RedMushroom,
            #[cfg(feature = "vanilla-visuals")]
            "sugar_cane" => Self::SugarCane,
            #[cfg(feature = "vanilla-visuals")]
            "lily_pad" => Self::LilyPad,
            #[cfg(feature = "vanilla-visuals")]
            "cactus" => Self::Cactus,
            #[cfg(feature = "vanilla-visuals")]
            "glass" => Self::Glass,
            _ => Self::Air,
        }
    }

    #[cfg(feature = "vanilla-visuals")]
    pub fn is_cutout(self) -> bool {
        matches!(
            self,
            Self::OakLeaves
                | Self::BirchLeaves
                | Self::SpruceLeaves
                | Self::DarkOakLeaves
                | Self::JungleLeaves
                | Self::AcaciaLeaves
                | Self::CherryLeaves
                | Self::MangroveLeaves
                | Self::OakSapling
                | Self::ShortGrass
                | Self::Fern
                | Self::DeadBush
                | Self::Dandelion
                | Self::Poppy
                | Self::BlueOrchid
                | Self::Allium
                | Self::AzureBluet
                | Self::OxeyeDaisy
                | Self::Cornflower
                | Self::LilyOfTheValley
                | Self::BrownMushroom
                | Self::RedMushroom
                | Self::SugarCane
                | Self::LilyPad
        )
    }

    #[cfg(feature = "vanilla-visuals")]
    pub fn is_blended(self) -> bool {
        matches!(self, Self::Ice | Self::Glass)
    }

    pub fn is_solid(self) -> bool {
        #[cfg(feature = "vanilla")]
        if self == Self::Lava {
            return false;
        }
        !matches!(self, Self::Air | Self::Water)
    }

    pub fn is_opaque(self) -> bool {
        #[cfg(feature = "vanilla-visuals")]
        if self.is_cutout() || self.is_blended() {
            return false;
        }
        #[cfg(feature = "vanilla")]
        if matches!(self, Self::Lava | Self::Ice) {
            return false;
        }
        !matches!(self, Self::Air | Self::Water | Self::OakLeaves)
    }

    pub fn should_cull_against(self, neighbor: Self) -> bool {
        #[cfg(feature = "vanilla-visuals")]
        if self.is_blended() && self == neighbor {
            return true;
        }
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
