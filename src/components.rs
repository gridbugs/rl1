use crate::visibility::Light;
use gridbugs::entity_table;

entity_table::declare_entity_module! {
    components {
        tile: Tile,
        opacity: u8,
        solid: (),
        light: Light,
    }
}
pub use components::Components;
pub use components::EntityData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Player,
    Wall,
    Floor,
}
