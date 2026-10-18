use crate::player::Player;
use crate::world::World;

/// The world and every player in it, ticked together so players can see each other.
#[derive(Default)]
pub struct Game {
    pub world: World,
    pub players: Vec<Player>,
}

impl Game {
    /// Advances every player one tick, in order.
    pub fn tick(&mut self) {
        for index in 0..self.players.len() {
            let (before, rest) = self.players.split_at_mut(index);
            let (player, after) = rest.split_first_mut().expect("index is below the length");
            player.tick(&mut self.world, |cell| {
                before.iter().chain(after.iter()).any(|other| other.overlaps_block(cell))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use glam::{IVec3, Vec3};

    use super::*;
    use crate::avatar::Skin;
    use crate::world::{Block, Facing};

    /// A stone floor at y = 0 and a builder at (8.5, 1, 8.5) facing +X, looking down at the floor two blocks ahead, with
    /// `block` to place on top of it.
    fn builder(block: Block) -> Game {
        let mut world = World::empty(IVec3::new(1, 4, 1));
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5), -FRAC_PI_2, Skin::Player);
        player.controls.head_pitch = -(1.62f32).atan2(2.0);
        player.controls.place = true;
        player.inventory.add(block, 1);
        Game {
            world,
            players: vec![player],
        }
    }

    #[test]
    fn blocks_are_not_placed_where_another_player_stands() {
        let cell = IVec3::new(10, 1, 8);
        let mut game = builder(Block::Cobblestone);
        game.tick();
        assert_eq!(game.world.block(cell), Block::Cobblestone);

        let mut game = builder(Block::Cobblestone);
        game.players.push(Player::new(Vec3::new(10.5, 1.0, 8.5), 0.0, Skin::Player));
        for _ in 0..10 {
            game.tick();
        }
        assert_eq!(game.world.block(cell), Block::Air);
        assert!(game.players[0].inventory.slot(0).is_some());
    }

    #[test]
    fn stairs_are_placed_stepping_up_away_from_the_builder() {
        let mut game = builder(Block::OakStairs(Facing::North));
        game.tick();
        assert_eq!(game.world.block(IVec3::new(10, 1, 8)), Block::OakStairs(Facing::East));
    }
}
