//! The field: weather, each side's screens and hazards, and what waits on a slot (a heal, a
//! delayed attack).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Weather {
    #[default]
    Clear,
    Rain,
    Sun,
    Sand,
    Hail,
}

/// Turns a weather or screen from a move lasts.
pub const WEATHER_TURNS: u8 = 5;
pub const SCREEN_TURNS: u8 = 5;
/// Most hazard layers on a side.
pub const HAZARD_MAX: u8 = 3;

/// What protects one side; each screen is turns left (0 = off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SideState {
    /// Halves physical damage (2/3 in doubles with both up).
    pub physical_wall: u8,
    /// The same for special damage.
    pub special_wall: u8,
    /// Blocks stat drops from foes.
    pub stat_guard: u8,
    /// Blocks major status and confusion from foes.
    pub status_guard: u8,
    /// Layers hurting whoever comes in on this side (0..=3).
    pub hazards: u8,
    /// This turn, the foes' single-target moves go to this position.
    pub drawing: Option<usize>,
}

/// Which side screen an event is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    PhysicalWall,
    SpecialWall,
    StatGuard,
    StatusGuard,
}

impl Screen {
    pub const ALL: [Screen; 4] = [
        Screen::PhysicalWall,
        Screen::SpecialWall,
        Screen::StatGuard,
        Screen::StatusGuard,
    ];
}

impl SideState {
    pub fn get(&self, screen: Screen) -> u8 {
        match screen {
            Screen::PhysicalWall => self.physical_wall,
            Screen::SpecialWall => self.special_wall,
            Screen::StatGuard => self.stat_guard,
            Screen::StatusGuard => self.status_guard,
        }
    }

    pub fn set(&mut self, screen: Screen, turns: u8) {
        match screen {
            Screen::PhysicalWall => self.physical_wall = turns,
            Screen::SpecialWall => self.special_wall = turns,
            Screen::StatGuard => self.stat_guard = turns,
            Screen::StatusGuard => self.status_guard = turns,
        }
    }
}

/// A pending heal: heals whoever is in the slot when it comes true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingHeal {
    /// End-of-turns left (it comes true when this reaches 0).
    pub turns: u8,
    pub amount: u16,
}

/// A delayed attack aimed at a slot, its damage worked out when it was used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delayed {
    pub turns: u8,
    pub damage: u32,
    pub move_id: u16,
    /// Hit chance when it lands (0 never misses).
    pub accuracy: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Field {
    pub weather: Weather,
    /// Turns of weather left; 0 with a weather means it lasts (set by an ability).
    pub weather_turns: u8,
    pub sides: [SideState; 2],
    /// By side and position.
    pub heals: [[Option<PendingHeal>; 2]; 2],
    pub delayed: [[Option<Delayed>; 2]; 2],
}
