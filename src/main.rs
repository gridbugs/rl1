use gridbugs::{
    chargrid::{control_flow::*, prelude::*},
    chargrid_wgpu::*,
    direction::CardinalDirection,
    entity_table::Entity,
    rgb_int::Rgb24,
    shadowcast::Context as ShadowcastContext,
};

mod components;
mod spatial;
mod visibility;
mod world;

use components::Tile;
use spatial::{Layer, Location};
use visibility::{CellVisibility, EntityTile, VisibilityCell, VisibilityGrid};
use world::World;

const CELL_SCALE: f64 = 4.;
const CELL_HEIGHT: f64 = 6. * CELL_SCALE;
const CELL_WIDTH: f64 = 6. * CELL_SCALE;

fn main() {
    let context = Context::new(Config {
        font_bytes: FontBytes {
            normal: include_bytes!("./fonts/PxPlus_IBM_CGAthin-custom.ttf").to_vec(),
            bold: include_bytes!("./fonts/PxPlus_IBM_CGA-custom.ttf").to_vec(),
        },
        title: "rl1".to_string(),
        window_dimensions_px: Dimensions {
            width: 960.,
            height: 720.,
        },
        cell_dimensions_px: Dimensions {
            width: CELL_WIDTH,
            height: CELL_HEIGHT,
        },
        font_scale: Dimensions {
            width: CELL_WIDTH,
            height: CELL_HEIGHT,
        },
        underline_width_cell_ratio: 0.1,
        underline_top_offset_cell_ratio: 0.8,
        resizable: false,
        force_secondary_adapter: false,
    });
    context.run(app());
}

fn app() -> App {
    cf(GameComponent {})
        .with_state(Game::new())
        .catch_escape()
        .map_val(|| app::Exit)
        .clear_each_frame()
        .exit_on_close()
}

struct Terrain {
    world: World,
    player_entity: Entity,
}

impl Terrain {
    fn new() -> Self {
        let s = include_str!("./terrain.txt");
        let player_data = World::make_player();
        let rows = s.split('\n').filter(|s| !s.is_empty()).collect::<Vec<_>>();
        let size = Size::new_u16(rows[0].len() as u16, rows.len() as u16);
        let mut world = World::new(size);
        let mut player_data = Some(player_data);
        let mut player_entity = None;
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch.is_control() {
                    continue;
                }
                let coord = Coord::new(x as i32, y as i32);
                match ch {
                    '.' => {
                        world.spawn_floor(coord);
                    }
                    'R' => {
                        world.spawn_floor(coord);
                        world.spawn_light(coord, Rgb24::new(255, 0, 0));
                    }
                    'G' => {
                        world.spawn_floor(coord);
                        world.spawn_light(coord, Rgb24::new(0, 255, 0));
                    }
                    '#' => {
                        world.spawn_wall(coord);
                    }
                    '@' => {
                        world.spawn_floor(coord);
                        let location = Location {
                            coord,
                            layer: Some(Layer::Character),
                        };
                        player_entity =
                            Some(world.insert_entity_data(location, player_data.take().unwrap()));
                    }

                    other => panic!("unexpected char {}", other),
                }
            }
        }
        let player_entity = player_entity.expect("didn't create player");
        Terrain {
            world,
            player_entity,
        }
    }
}

struct Game {
    world: World,
    player_entity: Entity,
    visibility_grid: VisibilityGrid,
    shadowcast_context: ShadowcastContext<u8>,
}

impl Game {
    fn new() -> Self {
        let Terrain {
            world,
            player_entity,
        } = Terrain::new();
        let visibility_grid = VisibilityGrid::new(world.size());
        let shadowcast_context = ShadowcastContext::default();
        let mut s = Self {
            world,
            player_entity,
            visibility_grid,
            shadowcast_context,
        };
        s.update_visibility();
        s
    }

    fn update_visibility(&mut self) {
        if let Some(player_coord) = self.world.entity_coord(self.player_entity) {
            self.visibility_grid.update(
                player_coord,
                &self.world,
                &mut self.shadowcast_context,
                None,
            );
        }
    }

    fn visibility_grid(&self) -> &VisibilityGrid {
        &self.visibility_grid
    }

    pub fn player_walk(&mut self, direction: CardinalDirection) {
        let player_coord = self
            .world
            .spatial_table
            .coord_of(self.player_entity)
            .unwrap();
        let destination = player_coord + direction.coord();
        if let Some(layers) = self.world.spatial_table.layers_at(destination) {
            if let Some(feature) = layers.feature {
                if self.world.components.solid.contains(feature) {
                    return;
                }
            }
            if layers.floor.is_some() {
                let _ = self
                    .world
                    .spatial_table
                    .update_coord(self.player_entity, destination);
            }
        }
        self.update_visibility();
    }
}

struct GameComponent {}

impl Component for GameComponent {
    type Output = Option<()>;
    type State = Game;

    fn render(&self, state: &Self::State, ctx: Ctx, fb: &mut FrameBuffer) {
        render_game_with_visibility(state, ctx, fb);
    }

    fn update(&mut self, state: &mut Self::State, _ctx: Ctx, event: Event) -> Self::Output {
        if let Some(keyboard_input) = event.keyboard_input() {
            match keyboard_input {
                KeyboardInput::Left => state.player_walk(CardinalDirection::West),
                KeyboardInput::Right => state.player_walk(CardinalDirection::East),
                KeyboardInput::Up => state.player_walk(CardinalDirection::North),
                KeyboardInput::Down => state.player_walk(CardinalDirection::South),
                _ => (),
            }
        }
        None
    }

    fn size(&self, _state: &Self::State, ctx: Ctx) -> Size {
        ctx.bounding_box.size()
    }
}

#[derive(Clone, Copy)]
struct LightBlend {
    light_colour: Rgb24,
}

impl Tint for LightBlend {
    fn tint(&self, rgba32: Rgba32) -> Rgba32 {
        rgba32
            .to_rgb24()
            .normalised_mul(self.light_colour)
            .saturating_add(self.light_colour.saturating_scalar_mul_div(1, 10))
            .to_rgba32(255)
    }
}

fn render_game_with_visibility(game: &Game, ctx: Ctx, fb: &mut FrameBuffer) {
    let visibility_grid = game.visibility_grid();
    let vis_count = visibility_grid.count();
    for (coord, visibility_cell) in game.visibility_grid().enumerate() {
        match visibility_cell.visibility(vis_count) {
            CellVisibility::CurrentlyVisibleWithLightColour(Some(light_colour)) => {
                render_visibile(
                    coord,
                    visibility_cell,
                    ctx_tint!(ctx, LightBlend { light_colour }),
                    fb,
                );
            }
            CellVisibility::PreviouslyVisible => {
                render_remembered(coord, visibility_cell, ctx, fb);
            }
            CellVisibility::NeverVisible
            | CellVisibility::CurrentlyVisibleWithLightColour(None) => (),
        }
    }
}

fn render_visibile(coord: Coord, cell: &VisibilityCell, ctx: Ctx, fb: &mut FrameBuffer) {
    let mut render_tile = |_entity, tile| {
        let ch = match tile {
            Tile::Floor => '.',
            Tile::Wall => '█',
            Tile::Player => '@',
        };
        fb.set_cell_relative_to_ctx(
            ctx,
            coord,
            0,
            RenderCell::default()
                .with_character(ch)
                .with_foreground(Rgba32::new_grey(255)),
        );
    };
    let tile_layers = cell.tile_layers();
    if let Some(EntityTile { entity, tile }) = tile_layers.floor {
        render_tile(entity, tile);
    }
    if let Some(EntityTile { entity, tile }) = tile_layers.feature {
        render_tile(entity, tile);
    }
    if let Some(EntityTile { entity, tile }) = tile_layers.item {
        render_tile(entity, tile);
    }
    if let Some(EntityTile { entity, tile }) = tile_layers.character {
        render_tile(entity, tile);
    }
}

fn render_remembered(coord: Coord, cell: &VisibilityCell, ctx: Ctx, fb: &mut FrameBuffer) {
    let tile_layers = cell.tile_layers();
    if let Some(EntityTile { tile, .. }) = tile_layers.feature {
        match tile {
            Tile::Wall => {
                fb.set_cell_relative_to_ctx(
                    ctx,
                    coord,
                    0,
                    RenderCell::default()
                        .with_character('▒')
                        .with_foreground(Rgba32::new_grey(127)),
                );
            }
            _ => (),
        }
    }
}
