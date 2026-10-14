//! Which chunks the camera might see past the terrain. Each chunk records which pairs of its sides
//! open space joins through it, and each frame a walk out from the camera's chunk follows only
//! those openings, so chunks that solid ground walls off from the camera aren't drawn.

use glam::IVec3;
use sim::world::{Block, CHUNK_SIZE, ChunkMap, ChunkSet, World};

/// A bit for each of the 15 pairs of a chunk's sides, set where open space joins the two through
/// the chunk.
pub type Connections = u16;

/// Every pair of sides joined, as through open air.
pub const ALL: Connections = (1 << 15) - 1;

/// A chunk's sides, in the order the walk numbers them; side `s ^ 1` faces the opposite way.
const SIDES: [IVec3; 6] = [IVec3::NEG_X, IVec3::X, IVec3::NEG_Y, IVec3::Y, IVec3::NEG_Z, IVec3::Z];

fn pair(a: usize, b: usize) -> Connections {
    let (a, b) = (a.min(b), a.max(b));
    debug_assert!(a != b);
    1 << (a * (11 - a) / 2 + b - a - 1)
}

pub fn connects(connections: Connections, a: usize, b: usize) -> bool {
    connections & pair(a, b) != 0
}

/// Whether light passes through a block: open space, water, and the border, which is drawn as a
/// faint plane.
fn see_through(block: Block) -> bool {
    matches!(block, Block::Air | Block::Water | Block::WorldBorder)
}

/// Which sides of the chunk at chunk coordinate `chunk` see each other through it, along paths of
/// see-through cells that step between cells sharing a face and stay inside the chunk.
pub fn connections(world: &World, chunk: IVec3) -> Connections {
    if let Some(block) = world.uniform_chunk(chunk) {
        return if see_through(block) { ALL } else { 0 };
    }
    let size = CHUNK_SIZE as i32;
    let origin = chunk * size;
    let open: Vec<bool> = world.blocks_in(origin, origin + size).into_iter().map(see_through).collect();
    let index = |cell: IVec3| ((cell.y * size + cell.z) * size + cell.x) as usize;
    let mut seen = vec![false; open.len()];
    let mut joined = 0;
    let mut stack = Vec::new();
    for start in (0..size).flat_map(|y| (0..size).flat_map(move |z| (0..size).map(move |x| IVec3::new(x, y, z)))) {
        if !open[index(start)] || seen[index(start)] {
            continue;
        }
        // Flood one pocket of open cells and note every side it reaches.
        seen[index(start)] = true;
        stack.push(start);
        let mut reached = [false; 6];
        while let Some(cell) = stack.pop() {
            for (side, &step) in SIDES.iter().enumerate() {
                let next = cell + step;
                if next.cmplt(IVec3::ZERO).any() || next.cmpge(IVec3::splat(size)).any() {
                    reached[side] = true;
                } else if open[index(next)] && !seen[index(next)] {
                    seen[index(next)] = true;
                    stack.push(next);
                }
            }
        }
        for a in 0..6 {
            for b in a + 1..6 {
                if reached[a] && reached[b] {
                    joined |= pair(a, b);
                }
            }
        }
    }
    joined
}

/// The chunks a camera in chunk `camera` might see. The walk starts at the camera's chunk and steps to
/// a neighbour only through a side that the chunk's open space joins to the side it was entered
/// through, and only away from the camera: never back along an axis it has already moved along in
/// the other direction. A straight line of sight passes through chunks in just that way, so every
/// chunk it can reach is visited. `in_view` bounds the walk to chunks in the frustum and range, and
/// `connections` gives each chunk's joined sides; space outside the loaded world should count as
/// open.
pub fn visible_chunks(camera: IVec3, connections: impl Fn(IVec3) -> Connections, in_view: impl Fn(IVec3) -> bool) -> ChunkSet {
    // The sides each chunk has been entered through. A chunk is walked on from once per side it is
    // entered through, since another side may join further sides.
    let mut entered: ChunkMap<u8> = ChunkMap::default();
    entered.insert(camera, 0);
    // Each chunk to walk on from: the side it was entered through, if any, and the sides stepped
    // out through on the way there.
    let mut queue = vec![(camera, None, 0u8)];
    while let Some((chunk, entry, moved)) = queue.pop() {
        let joined = connections(chunk);
        for (side, &step) in SIDES.iter().enumerate() {
            if moved & 1 << (side ^ 1) != 0 || entry.is_some_and(|entry| !connects(joined, entry, side)) {
                continue;
            }
            let next = chunk + step;
            let entry_side = side ^ 1;
            let sides = entered.get(&next).copied();
            if sides.is_some_and(|sides| sides & 1 << entry_side != 0) || (sides.is_none() && !in_view(next)) {
                continue;
            }
            *entered.entry(next).or_default() |= 1 << entry_side;
            queue.push((next, Some(entry_side), moved | 1 << side));
        }
    }
    entered.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const SIZE: i32 = CHUNK_SIZE as i32;

    /// A world of `chunks` chunks from the origin, every cell `fill`.
    fn filled(chunks: IVec3, fill: Block) -> World {
        let mut world = World::empty(chunks);
        if fill != Block::Air {
            for x in 0..chunks.x * SIZE {
                for y in 0..chunks.y * SIZE {
                    for z in 0..chunks.z * SIZE {
                        world.set_block(IVec3::new(x, y, z), fill);
                    }
                }
            }
        }
        world
    }

    fn joined_pairs(connections: Connections) -> HashSet<(usize, usize)> {
        (0..6)
            .flat_map(|a| (a + 1..6).map(move |b| (a, b)))
            .filter(|&(a, b)| connects(connections, a, b))
            .collect()
    }

    #[test]
    fn pairs_have_their_own_bits() {
        let bits: HashSet<Connections> = (0..6).flat_map(|a| (a + 1..6).map(move |b| pair(a, b))).collect();
        assert_eq!(bits.len(), 15);
        assert_eq!(bits.iter().fold(0, |all, bit| all | bit), ALL);
        assert_eq!(pair(3, 1), pair(1, 3));
    }

    #[test]
    fn open_chunks_join_every_side_and_solid_ones_none() {
        assert_eq!(connections(&filled(IVec3::ONE, Block::Air), IVec3::ZERO), ALL);
        assert_eq!(connections(&filled(IVec3::ONE, Block::Water), IVec3::ZERO), ALL);
        assert_eq!(connections(&filled(IVec3::ONE, Block::Stone), IVec3::ZERO), 0);
        assert_eq!(connections(&filled(IVec3::ONE, Block::OakLeaves), IVec3::ZERO), 0);
    }

    #[test]
    fn a_solid_floor_parts_the_top_from_the_bottom() {
        let mut world = filled(IVec3::ONE, Block::Air);
        for x in 0..SIZE {
            for z in 0..SIZE {
                world.set_block(IVec3::new(x, 7, z), Block::Stone);
            }
        }
        let joined = joined_pairs(connections(&world, IVec3::ZERO));
        let (bottom, top) = (2, 3);
        assert!(!joined.contains(&(bottom, top)));
        for side in [0, 1, 4, 5] {
            assert!(joined.contains(&(side.min(bottom), side.max(bottom))), "{side}");
            assert!(joined.contains(&(side.min(top), side.max(top))), "{side}");
        }
        // Each side wall reaches above and below the floor, so the walls join each other too.
        assert_eq!(joined.len(), 14);
    }

    #[test]
    fn a_straight_tunnel_joins_its_two_ends() {
        let mut world = filled(IVec3::ONE, Block::Stone);
        for x in 0..SIZE {
            world.set_block(IVec3::new(x, 8, 8), Block::Air);
        }
        assert_eq!(joined_pairs(connections(&world, IVec3::ZERO)), HashSet::from([(0, 1)]));
    }

    #[test]
    fn the_border_and_water_are_seen_through() {
        let mut world = filled(IVec3::ONE, Block::Stone);
        for x in 0..SIZE {
            let block = if x < 8 { Block::WorldBorder } else { Block::Water };
            world.set_block(IVec3::new(x, 3, 3), block);
        }
        assert_eq!(joined_pairs(connections(&world, IVec3::ZERO)), HashSet::from([(0, 1)]));
    }

    /// Five chunks across and six high: ground of stone up to `ground`, open air above it.
    fn ground_world(ground: i32) -> World {
        let mut world = filled(IVec3::new(5, 6, 5), Block::Air);
        for x in 0..5 * SIZE {
            for z in 0..5 * SIZE {
                for y in 0..ground {
                    world.set_block(IVec3::new(x, y, z), Block::Stone);
                }
            }
        }
        world
    }

    /// The chunks visible from `camera` in `world`, looking every way, with the world's own chunks
    /// in view and nothing outside them.
    fn walk(world: &World, camera: IVec3) -> HashSet<IVec3> {
        let inside = |chunk: IVec3| chunk.cmpge(IVec3::ZERO).all() && chunk.cmplt(IVec3::new(5, 6, 5)).all();
        visible_chunks(camera, |chunk| connections(world, chunk), inside)
            .into_iter()
            .collect()
    }

    fn dig(world: &mut World, from: IVec3, to: IVec3) {
        for x in from.x..=to.x {
            for y in from.y..=to.y {
                for z in from.z..=to.z {
                    world.set_block(IVec3::new(x, y, z), Block::Air);
                }
            }
        }
    }

    #[test]
    fn a_sealed_pocket_below_the_ground_is_not_visited() {
        // Ground up to y = 64, the camera in the open air at chunk level 4.
        let mut world = ground_world(4 * SIZE);
        let pocket = IVec3::new(2, 1, 2);
        let centre = pocket * SIZE + 8;
        dig(&mut world, centre - 3, centre + 3);
        let visible = walk(&world, IVec3::new(2, 4, 2));
        assert!(!visible.contains(&pocket));
        // The ground's top chunks are seen from above.
        assert!(visible.contains(&IVec3::new(2, 3, 2)));
        assert!(visible.contains(&IVec3::new(0, 3, 4)));
    }

    #[test]
    fn a_pocket_with_a_shaft_to_the_surface_is_visited() {
        let mut world = ground_world(4 * SIZE);
        let pocket = IVec3::new(2, 1, 2);
        let centre = pocket * SIZE + 8;
        dig(&mut world, centre - 3, centre + 3);
        dig(&mut world, centre, IVec3::new(centre.x, 4 * SIZE - 1, centre.z));
        assert!(walk(&world, IVec3::new(2, 4, 2)).contains(&pocket));
    }

    #[test]
    fn everything_above_open_ground_is_visited() {
        let world = ground_world(2 * SIZE + 5);
        let visible = walk(&world, IVec3::new(1, 4, 3));
        for x in 0..5 {
            for z in 0..5 {
                for y in 2..6 {
                    assert!(visible.contains(&IVec3::new(x, y, z)), "{x}, {y}, {z}");
                }
                assert!(!visible.contains(&IVec3::new(x, 0, z)), "{x}, 0, {z}");
            }
        }
    }

    #[test]
    fn the_walk_keeps_to_the_view() {
        let world = filled(IVec3::new(5, 6, 5), Block::Air);
        let in_view = |chunk: IVec3| chunk.x >= 2 && chunk.cmpge(IVec3::ZERO).all() && chunk.cmplt(IVec3::new(5, 6, 5)).all();
        let visible = visible_chunks(IVec3::new(2, 2, 2), |chunk| connections(&world, chunk), in_view);
        assert!(visible.iter().all(|chunk| chunk.x >= 2));
        assert_eq!(visible.len(), 3 * 6 * 5);
    }
}
