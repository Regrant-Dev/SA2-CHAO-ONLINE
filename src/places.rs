//! Names for where a player is (level ids and Chao World areas), for people to read: the server's
//! status file (a status page can show who's playing where) and the F9 player list.

/// A level's name, by the game's level id.
pub fn level_name(level: i16) -> Option<&'static str> {
    Some(match level {
        3 => "Green Forest",
        4 => "White Jungle",
        5 => "Pumpkin Hill",
        6 => "Sky Rail",
        7 => "Aquatic Mine",
        8 => "Security Hall",
        9 => "Prison Lane",
        10 => "Metal Harbor",
        11 => "Iron Gate",
        12 => "Weapons Bed",
        13 => "City Escape",
        14 => "Radical Highway",
        16 => "Wild Canyon",
        17 => "Mission Street",
        18 => "Dry Lagoon",
        19 => "Sonic vs Shadow",
        20 => "Tails vs Eggman",
        21 => "Sand Ocean",
        22 => "Crazy Gadget",
        23 => "Hidden Base",
        24 => "Eternal Engine",
        25 => "Death Chamber",
        26 => "Egg Quarters",
        27 => "Lost Colony",
        28 => "Pyramid Cave",
        29 => "Tails vs Eggman 2",
        30 => "Final Rush",
        31 => "Green Hill",
        32 => "Meteor Herd",
        33 => "Knuckles vs Rouge",
        34..=38 => "Cannon's Core",
        39 => "Mission Street (2P)",
        40 => "Final Chase",
        41 => "Wild Canyon (2P)",
        42 => "Sonic vs Shadow 2",
        43 => "Cosmic Wall",
        44 => "Mad Space",
        45 => "Sand Ocean (2P)",
        46 => "Dry Lagoon (2P)",
        47 => "Pyramid Race",
        48 => "Hidden Base (2P)",
        49 => "Pool Quest",
        50 => "Planet Quest",
        51 => "Deck Race",
        52 => "Downtown Race",
        53 => "Cosmic Wall (2P)",
        54 => "Grind Race",
        55 => "Lost Colony (2P)",
        56 => "Eternal Engine (2P)",
        57 => "Metal Harbor (2P)",
        58 => "Iron Gate (2P)",
        59 => "Death Chamber (2P)",
        60 => "Big Foot",
        61 => "Hot Shot",
        62 => "Flying Dog",
        63 => "King Boom Boo",
        64 | 67 => "Egg Golem",
        65 => "Biolizard",
        66 => "Final Hazard",
        70 => "Route 101/280",
        71 => "Kart Race",
        _ => return None,
    })
}

/// Where a player is: a level, an area of Chao World, or the menus.
pub fn place(level: i16, chao_area: i8) -> String {
    match (level, chao_area) {
        (90, 1) => "Neutral Garden".into(),
        (90, 2) => "Hero Garden".into(),
        (90, 3) => "Dark Garden".into(),
        (90, 4) => "Chao Race".into(),
        (90, 10) => "Chao Karate".into(),
        (90, _) => "Chao World".into(),
        (l, _) if l <= 0 => "Menus".into(),
        (l, _) => level_name(l).map_or_else(|| format!("Level {l}"), str::to_string),
    }
}

/// A treasure hunt stage (Knuckles and Rouge; missions 1, 4 and 5 hunt).
pub fn is_hunt_stage(level: i16) -> bool {
    matches!(level, 5 | 7 | 8 | 16 | 18 | 25 | 26 | 32 | 44)
}

/// A time in centiseconds as m:ss.cc.
pub fn time(cs: u32) -> String {
    format!("{}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

#[cfg(test)]
mod tests {
    use super::place;

    #[test]
    fn times() {
        assert_eq!(super::time(8345), "1:23.45");
    }

    #[test]
    fn places() {
        assert_eq!(place(13, -1), "City Escape");
        assert_eq!(place(90, 2), "Hero Garden");
        assert_eq!(place(0, -1), "Menus");
        assert_eq!(place(99, -1), "Level 99");
    }
}
