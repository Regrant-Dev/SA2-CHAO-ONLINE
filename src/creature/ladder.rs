//! Deterministic mod-owned progression using the source trainer roster.
use super::{
    rom::Rom,
    world::{TRAINER_COUNT, TrainerMon},
};
pub const LEADERS: usize = 8;
pub const LEAGUE: usize = 4;
pub const TIERS: usize = LEADERS + 1;
/// A trainer milestone in the mod's progression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leader {
    pub id: u16,
    /// Optional source teaching reward (0..TEACHING_MOVE_COUNT).
    pub teaching: Option<u8>,
    /// Optional source location label.
    pub town: Option<String>,
}

/// Every trainer on the ladder, by trainer id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ladder {
    /// Route trainers in `TIERS` tiers: tier k (k < LEADERS) holds the ones whose strongest
    /// creature is no stronger than leader k's (and stronger than leader k-1's), the last tier
    /// the ones above the last leader. Each tier is sorted by (top level, id).
    pub route: Vec<Vec<u16>>,
    /// `LEADERS` milestones in progression order.
    pub leaders: Vec<Leader>,
    /// `LEAGUE` league trainers in battle order.
    pub league: Vec<u16>,
    pub champion: u16,
}

/// One of a trainer's creatures as the game makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyMember {
    pub mon: TrainerMon,
    pub ability: u8,
    /// 0..25: raises stat `nature / 5` and lowers stat `nature % 5` (attack, defense, speed,
    /// sp. attack, sp. defense).
    pub nature: u8,
    pub friendship: u8,
}

impl Rom {
    pub fn trainer_party(&self, id: u16) -> Option<Vec<PartyMember>> {
        let trainer = self.trainer(id)?;
        let source = self.catalog.trainers.get(usize::from(id))?;
        trainer
            .party
            .iter()
            .zip(&source.party)
            .map(|(mon, raw)| {
                let info = self.species_form(raw.key)?;
                let slot = match raw.ability_selector {
                    1 => 0,
                    2 => 1,
                    3 => 2,
                    _ => usize::from((id + mon.species) % 2),
                };
                let ability = info.abilities[slot];
                let ability = if ability == 0 {
                    info.abilities[0]
                } else {
                    ability
                };
                // Random source nature/ability selectors become stable choices under this format.
                // They do not depend on SA2's or another player's frame clock.
                Some(PartyMember {
                    mon: *mon,
                    ability,
                    nature: if raw.nature < 25 {
                        raw.nature
                    } else {
                        ((u32::from(id) * 31 + u32::from(mon.species)) % 25) as u8
                    },
                    friendship: self.personal(raw.key)?.base_friendship,
                })
            })
            .collect()
    }
    /// A versioned strength ladder. Milestones are selected from ordinary source trainer
    /// teams by level; no badges or rewards from the retired game's scripts are inferred.
    pub fn ladder(&self) -> Option<Ladder> {
        let all: Vec<_> = (1..TRAINER_COUNT)
            .filter_map(|id| {
                let t = self.trainer(id)?;
                let party = self.trainer_party(id)?;
                if t.double
                    || t.party.is_empty()
                    || party.iter().any(|m| {
                        !crate::battle::source_rules::ability_supported(m.ability)
                            || !super::data::ordinary_form(super::data::SpeciesKey {
                                species: m.mon.species,
                                form: m.mon.form,
                            })
                            || self
                                .personal(super::data::SpeciesKey {
                                    species: m.mon.species,
                                    form: m.mon.form,
                                })
                                .is_none()
                            || m.mon.moves.iter().all(|&id| self.move_info(id).is_none())
                            || m.mon
                                .moves
                                .iter()
                                .any(|&id| id != 0 && self.move_info(id).is_none())
                    })
                {
                    return None;
                }
                Some((t.party.iter().map(|m| m.level).max()?, id))
            })
            .collect();
        strength_ladder(all)
    }
}

/// Reserve every later milestone before assigning route trainers. Equal or low trainer
/// levels must not drain the candidates needed by a later milestone.
fn strength_ladder(mut all: Vec<(u8, u16)>) -> Option<Ladder> {
    all.sort_unstable();
    if all.len() < LEADERS + LEAGUE + 2 {
        return None;
    }
    let champion = all.pop()?.1;
    let league = all
        .split_off(all.len() - LEAGUE)
        .into_iter()
        .map(|(_, id)| id)
        .collect();
    let mut leaders = Vec::with_capacity(LEADERS);
    let mut route = Vec::with_capacity(TIERS);
    for (index, target) in [12u8, 20, 28, 36, 44, 52, 60, 68].into_iter().enumerate() {
        let remaining = LEADERS - index - 1;
        let latest = all.len().checked_sub(remaining + 1)?;
        let at = all
            .iter()
            .position(|&(level, _)| level >= target)
            .unwrap_or(all.len() - 1)
            .min(latest);
        let (level, id) = all.remove(at);
        let count = all
            .partition_point(|&(lv, _)| lv <= level)
            .min(all.len().saturating_sub(remaining));
        route.push(all.drain(..count).map(|(_, id)| id).collect());
        leaders.push(Leader {
            id,
            teaching: None,
            town: Some(format!("Milestone {}", index + 1)),
        });
    }
    route.push(all.into_iter().map(|(_, id)| id).collect());
    Some(Ladder {
        route,
        leaders,
        league,
        champion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tied_or_low_rosters_keep_every_trainer_and_all_milestones() {
        for levels in [
            vec![6; 288],
            vec![100; 14],
            (0..32).map(|i| (i / 4 + 5) as u8).collect(),
        ] {
            let input: Vec<_> = levels
                .into_iter()
                .enumerate()
                .map(|(i, level)| (level, i as u16 + 1))
                .collect();
            let ladder = strength_ladder(input.clone()).unwrap();
            assert_eq!(ladder.leaders.len(), LEADERS);
            assert_eq!(ladder.league.len(), LEAGUE);
            assert_eq!(ladder.route.len(), TIERS);
            let mut ids: Vec<_> = ladder
                .route
                .into_iter()
                .flatten()
                .chain(ladder.leaders.iter().map(|m| m.id))
                .chain(ladder.league)
                .chain([ladder.champion])
                .collect();
            ids.sort_unstable();
            assert_eq!(ids, (1..=input.len() as u16).collect::<Vec<_>>());
            assert!(
                ladder
                    .leaders
                    .windows(2)
                    .all(|pair| input[usize::from(pair[0].id - 1)].0
                        <= input[usize::from(pair[1].id - 1)].0)
            );
        }
        assert!(strength_ladder(vec![(5, 1); LEADERS + LEAGUE + 1]).is_none());
    }
}
