//! Mod-owned effect choreography driven by normalized modern move records. No cartridge
//! executable callbacks or source animation scripts are interpreted by this presentation.
use super::{
    battle_anim::{HIT_TAG, TAG_BASE, TYPE_STYLES, type_colour},
    data::{MoveCategory, MovePolicy, MoveRecord},
    rom::Rom,
};

/// How a move's sprites move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Movement {
    /// Nothing of its own (it only tints or moves the fighters).
    #[default]
    None,
    /// Only the plain hit, on the target.
    HitOnly,
    /// One shot from the user to the target.
    Straight,
    /// A quick stream of shots.
    Beam,
    /// A few shots one after another.
    Multi,
    /// A few lobbed shots.
    MultiArc,
    /// One lobbed shot.
    Arc,
    /// Shots twisting around their way to the target.
    Corkscrew,
    /// Swirling in onto a fighter.
    Spiral,
    /// Circling a fighter.
    Orbit,
    /// Falling from above onto a fighter.
    Rain,
    /// Rising from the ground around a fighter.
    Rise,
    /// From the target back to the user.
    Drain,
    /// Closing in on the user from all round.
    Gather,
    /// Bursting on a fighter.
    Burst,
    /// Showing on a fighter for a while.
    Aura,
}

impl Movement {
    /// Its sprites go from the user to the target.
    pub fn travels(self) -> bool {
        matches!(
            self,
            Movement::Straight
                | Movement::Beam
                | Movement::Multi
                | Movement::MultiArc
                | Movement::Arc
                | Movement::Corkscrew
                | Movement::Drain
        )
    }
}

/// Which fighter a movement that stays in one place shows on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Place {
    User,
    #[default]
    Target,
}

/// What a move's main sprites are drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A generated template key (its sheet tag).
    Template(usize),
    /// A generated sheet tag.
    Sheet(u16),
}

/// Semantic presentation derived from normalized move metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub movement: Movement,
    pub place: Place,
    pub main: Option<Source>,
    /// Sprites in the main group, and the median frames between them.
    pub count: u16,
    pub gap: u16,
    /// A burst on the target beside the main movement.
    pub impact: Option<usize>,
    /// Plain hits shown.
    pub hits: u16,
    /// The target shakes; the screen shakes (a big hit).
    pub shake: bool,
    pub big: bool,
    /// The commonest colour it tints with (BGR555).
    pub flash: Option<u16>,
    /// The user lunges (a sprite-less move of the user).
    pub lunge: bool,
    pub two_turn: bool,
    /// Total presentation frames.
    pub frames: u16,
    /// The main shot's duration and presentation arc height.
    pub duration: Option<u16>,
    pub height: Option<i16>,
}

/// Compile bounded visual direction separately from battle mechanics. Unknown ordinary
/// move mechanics may still have an honest generic effect; excluded power-ups have none.
pub fn plan_for(record: &MoveRecord) -> Option<Plan> {
    if record.id == 0 || record.type_id >= 18 || record.policy != MovePolicy::Ordinary {
        return None;
    }
    let status = record.category == MoveCategory::Status;
    let on_user = matches!(record.target_raw, 7 | 8 | 10 | 12);
    let contact = record.flags & 1 != 0;
    let drain = !status && (record.recoil_raw as i8) > 0;
    let multi = record.max_hits > 1;
    let movement = if drain {
        Movement::Drain
    } else if status {
        if on_user {
            Movement::Gather
        } else {
            Movement::Spiral
        }
    } else if contact {
        Movement::HitOnly
    } else if multi {
        Movement::Multi
    } else if matches!(record.target_raw, 4 | 5 | 11) {
        Movement::Rain
    } else if record.category == MoveCategory::Physical {
        Movement::Arc
    } else if record.power >= 90 {
        Movement::Beam
    } else if record.id.is_multiple_of(3) {
        Movement::Corkscrew
    } else {
        Movement::Straight
    };
    let style = if status {
        2
    } else if contact {
        1
    } else {
        0
    };
    let tag = TAG_BASE + TYPE_STYLES * style + u16::from(record.type_id);
    let color = type_colour(record.type_id);
    let flash = record.power >= 100;
    let count = if movement == Movement::Beam {
        8
    } else if status {
        6
    } else {
        u16::from(record.max_hits.max(1)).min(8)
    };
    Some(Plan {
        movement,
        place: if on_user { Place::User } else { Place::Target },
        main: Some(Source::Sheet(tag)),
        impact: (!status && !contact).then_some(usize::from(HIT_TAG)),
        count,
        gap: if status { 3 } else { 2 },
        hits: u16::from(record.min_hits.max(1)).min(8),
        shake: !status,
        big: flash,
        flash: flash.then_some(
            (u16::from(color[0] >> 3))
                | (u16::from(color[1] >> 3) << 5)
                | (u16::from(color[2] >> 3) << 10),
        ),
        lunge: contact,
        two_turn: record.flags & (1 << 1) != 0,
        frames: if status { 42 } else { 30 },
        duration: Some(if status { 32 } else { 22 }),
        height: Some(if movement == Movement::Arc { 20 } else { 0 }),
    })
}
impl Rom {
    pub fn move_plan(&self, id: u16, _hit: Option<u16>) -> Option<Plan> {
        plan_for(self.modern_move(id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> MoveRecord {
        let mut data = vec![0u8; 0x28];
        data[0] = 10;
        data[2] = 2;
        data[3] = 60;
        data[4] = 100;
        data[5] = 20;
        super::super::data::parse_move(&data, 710).unwrap()
    }
    #[test]
    fn modern_category_contact_and_drain_choose_distinct_directions() {
        let mut record = record();
        let special = plan_for(&record).unwrap();
        assert!(special.movement.travels());
        record.flags = 1;
        assert_eq!(plan_for(&record).unwrap().movement, Movement::HitOnly);
        record.flags = 0;
        record.recoil_raw = 50;
        assert_eq!(plan_for(&record).unwrap().movement, Movement::Drain);
        record.recoil_raw = 0;
        record.category = MoveCategory::Status;
        record.target_raw = 7;
        let aura = plan_for(&record).unwrap();
        assert_eq!(aura.place, Place::User);
        assert_eq!(aura.movement, Movement::Gather);
        record.target_raw = 0;
        assert_eq!(plan_for(&record).unwrap().place, Place::Target);
        record.policy = MovePolicy::ExcludedPowerUpMove;
        assert!(plan_for(&record).is_none());
    }
    #[test]
    fn large_or_multi_hit_effects_stay_bounded() {
        let mut record = record();
        record.max_hits = 255;
        record.min_hits = 255;
        record.power = 150;
        let plan = plan_for(&record).unwrap();
        assert_eq!(plan.count, 8);
        assert_eq!(plan.hits, 8);
        assert!(plan.big && plan.flash.is_some());
        record.type_id = 255;
        assert!(plan_for(&record).is_none());
    }
}
