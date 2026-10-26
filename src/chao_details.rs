//! F9 > Chao details: one of our Chao's stats as a screen of pages, a summary first. Pages are
//! lines of coloured text and bars on a grid of text cells (hud.rs draws them; menu.rs reads the
//! keys and pad). Field meanings and game rules: chao_stats.rs, re/chao-details.md.
//! All formatting is pure: viewing a record never calls native AI or changes a save.

use crate::chao::{ChaoData, TYPE_EGG};
use crate::chao_stats::{self as stats, ABILITIES, Ability, LifeLeft, Side};
use std::fmt::Write;

/// The panel's width in text cells, and the content's inside its one-cell margins.
pub const PANEL_COLUMNS: usize = 64;
pub const COLUMNS: usize = PANEL_COLUMNS - 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Summary,
    Abilities,
    Growth,
    Feelings,
    Personality,
    Bonds,
    Battle,
    Appearance,
    Health,
    Raw,
}

impl Page {
    pub const ALL: [Self; 10] = [
        Self::Summary,
        Self::Abilities,
        Self::Growth,
        Self::Feelings,
        Self::Personality,
        Self::Bonds,
        Self::Battle,
        Self::Appearance,
        Self::Health,
        Self::Raw,
    ];

    /// Its name in the tab strip.
    pub fn tab(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Abilities => "Stats",
            Self::Growth => "Growth",
            Self::Feelings => "Mood",
            Self::Personality => "Traits",
            Self::Bonds => "Bonds",
            Self::Battle => "Battle",
            Self::Appearance => "Looks",
            Self::Health => "Health",
            Self::Raw => "Raw",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }
    pub fn next(self, backwards: bool) -> Self {
        Self::ALL
            [(self.index() + if backwards { Self::ALL.len() - 1 } else { 1 }) % Self::ALL.len()]
    }
}

/// What the mod keeps of a Chao's battles (creature_link; not in the SA2 save).
#[derive(Clone, Debug, PartialEq)]
pub struct Battle {
    pub level: u8,
    /// XP toward the next level and the XP that level takes (0 at the top level).
    pub xp: Option<(u32, u32)>,
    pub partner: Partner,
    /// The moves it battles with, by name ("Move #id" without the catalog).
    pub moves: Vec<String>,
    /// How many moves it has learned.
    pub known: usize,
}

impl Default for Battle {
    fn default() -> Self {
        Battle {
            level: crate::battle::levels::START_LEVEL,
            xp: Some((
                0,
                crate::battle::levels::step(crate::battle::levels::START_LEVEL),
            )),
            partner: Partner::None,
            moves: Vec::new(),
            known: 0,
        }
    }
}

/// Its creature link partner's types.
#[derive(Clone, Debug, PartialEq)]
pub enum Partner {
    None,
    Types(Vec<String>),
    /// The type names come from the player's catalog, which isn't ready.
    NoCatalog,
}

#[derive(Default)]
pub struct View {
    pub slot: Option<usize>,
    pub page: Page,
    pub scroll: usize,
    /// The shown Chao's battle details, by its identity.
    pub battle: Option<(u64, Battle)>,
    /// The controller works the view (in a stage or garden): its hint shows.
    pub pad: bool,
}

impl View {
    pub fn select(&mut self, slots: &[usize], backwards: bool) {
        if slots.is_empty() {
            self.slot = None;
            return;
        }
        let current = self
            .slot
            .and_then(|slot| slots.iter().position(|&s| s == slot))
            .unwrap_or(0);
        self.slot =
            Some(slots[(current + if backwards { slots.len() - 1 } else { 1 }) % slots.len()]);
        self.scroll = 0;
    }

    pub fn retain_slot(&mut self, slots: &[usize]) {
        if !self.slot.is_some_and(|slot| slots.contains(&slot)) {
            self.slot = slots.first().copied();
            self.scroll = 0;
        }
    }

    pub fn turn_page(&mut self, backwards: bool) {
        self.page = self.page.next(backwards);
        self.scroll = 0;
    }
}

/// How a piece of text or a bar is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Text,
    Dim,
    Head,
    Accent,
    Good,
    Bad,
    Hero,
    Dark,
    Grade(u8),
}

impl Tone {
    /// 0xAARRGGBB.
    pub fn colour(self) -> u32 {
        match self {
            Self::Text => 0xFFE0_E0E0,
            Self::Dim => 0xFFA0_A0A0,
            Self::Head => 0xFFFF_E080,
            Self::Accent => 0xFF70_C8B0,
            Self::Good => 0xFF80_E080,
            Self::Bad => 0xFFFF_7070,
            Self::Hero => 0xFF70_B0FF,
            Self::Dark => 0xFFC0_60E0,
            Self::Grade(g) => stats::grade_colour(g),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub col: usize,
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gauge {
    /// Filled from the left, 0..1.
    Fill(f32, Tone),
    /// From the middle, −1..1, toward `minus` or `plus`, with marks at ±0.5 when `marks`.
    Signed {
        value: f32,
        minus: Tone,
        plus: Tone,
        marks: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub col: usize,
    pub cells: usize,
    pub gauge: Gauge,
}

/// One row of the screen: text at columns and bars over cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
    pub bars: Vec<Bar>,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn at(mut self, col: usize, text: impl Into<String>, tone: Tone) -> Self {
        self.spans.push(Span {
            col,
            text: text.into(),
            tone,
        });
        self
    }
    pub fn fill(mut self, col: usize, cells: usize, fraction: f32, tone: Tone) -> Self {
        self.bars.push(Bar {
            col,
            cells,
            gauge: Gauge::Fill(fraction, tone),
        });
        self
    }
    fn signed(mut self, col: usize, cells: usize, value: f32, (minus, plus): (Tone, Tone)) -> Self {
        self.bars.push(Bar {
            col,
            cells,
            gauge: Gauge::Signed {
                value,
                minus,
                plus,
                marks: false,
            },
        });
        self
    }
    /// A leaning: signed with the marks where evolution starts counting it.
    fn leaning(self, col: usize, cells: usize, value: f32, tones: (Tone, Tone)) -> Self {
        let mut line = self.signed(col, cells, value, tones);
        if let Some(Bar {
            gauge: Gauge::Signed { marks, .. },
            ..
        }) = line.bars.last_mut()
        {
            *marks = true;
        }
        line
    }

    /// The text as one string, each span at its column.
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut spans: Vec<&Span> = self.spans.iter().collect();
        spans.sort_by_key(|s| s.col);
        for s in spans {
            while out.chars().count() < s.col {
                out.push(' ');
            }
            out.push_str(&s.text);
        }
        out
    }

    /// The cells it covers: `(start, end)` of each span and bar.
    #[cfg(test)]
    pub fn cells(&self) -> Vec<(usize, usize)> {
        self.spans
            .iter()
            .map(|s| (s.col, s.col + s.text.chars().count()))
            .chain(self.bars.iter().map(|b| (b.col, b.col + b.cells)))
            .collect()
    }
}

fn text(t: impl Into<String>) -> Line {
    Line::new().at(0, t, Tone::Text)
}
fn dim(t: impl Into<String>) -> Line {
    Line::new().at(0, t, Tone::Dim)
}
fn head(t: impl Into<String>) -> Line {
    Line::new().at(0, t, Tone::Head)
}
fn value(label: &str, v: impl std::fmt::Display) -> Line {
    Line::new()
        .at(0, label, Tone::Dim)
        .at(LABEL, v.to_string(), Tone::Text)
}

/// Where values start after a label.
const LABEL: usize = 13;

/// The font size and how many content rows fit: the font as big as 40 rows of the height and
/// the panel's width allow, and the rows left after the screen's other ones (`CHROME_ROWS`).
pub fn layout(width: f32, height: f32) -> (f32, usize) {
    let font = (height / 40.0).min(width / 66.0).floor().max(4.0);
    let visible = ((height / font) as usize)
        .saturating_sub(CHROME_ROWS)
        .clamp(4, 24);
    (font, visible)
}

/// Rows around the content: a row above the panel, its margins, the title, the Chao, the tabs,
/// a gap, the scroll line and two hint lines, and two spare.
pub const CHROME_ROWS: usize = 12;

/// The whole screen: its rows (the content scrolled to `view.scroll`, kept in range) and the
/// current tab (row, first column, width) for its highlight.
pub struct Screen {
    pub rows: Vec<Line>,
    pub tab: (usize, usize, usize),
}

/// The screen for Chao `chao` (`label` names it; `index` of `count` of ours), `visible`
/// content rows tall.
pub fn screen(
    chao: Option<&ChaoData>,
    label: &str,
    index: Option<usize>,
    count: usize,
    view: &mut View,
    visible: usize,
) -> Screen {
    let mut rows =
        vec![
            Line::new()
                .at(0, "Chao details", Tone::Head)
                .at(COLUMNS - 9, "View only", Tone::Dim),
        ];
    let mut chao_line = Line::new().at(0, clip(&format!("< {label} >"), 48), Tone::Accent);
    if let Some(i) = index {
        let of = format!("{} of {count}", i + 1);
        chao_line = chao_line.at(COLUMNS - of.len(), of, Tone::Dim);
    }
    rows.push(chao_line);
    let (tabs, tab) = tab_strip(view.page);
    rows.push(tabs);
    rows.push(Line::new());
    let content = match chao {
        Some(c) => {
            let battle = view
                .battle
                .as_ref()
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
            lines(c, view.page, &battle)
        }
        None => vec![text("Enter Chao World or load a Chao save first.")],
    };
    view.scroll = view.scroll.min(content.len().saturating_sub(visible));
    rows.extend(content.iter().skip(view.scroll).take(visible).cloned());
    if content.len() > visible {
        rows.push(dim(format!(
            "Lines {}-{} of {} (Up/Down scroll)",
            view.scroll + 1,
            (view.scroll + visible).min(content.len()),
            content.len()
        )));
    }
    rows.push(dim(
        "Left/Right page  Up/Down scroll  PgUp/PgDn Chao  Esc back",
    ));
    if view.pad {
        rows.push(dim("Controller: D-pad or stick, L/R other Chao, B back"));
    }
    Screen {
        rows,
        tab: (2, tab.0, tab.1),
    }
}

/// The tabs one space apart, the current one in the accent colour; its (column, width).
fn tab_strip(page: Page) -> (Line, (usize, usize)) {
    let mut line = Line::new();
    let mut col = 0;
    let mut current = (0, 0);
    for p in Page::ALL {
        let name = p.tab();
        if p == page {
            current = (col, name.len());
        }
        line = line.at(col, name, if p == page { Tone::Head } else { Tone::Dim });
        col += name.len() + 1;
    }
    (line, current)
}

fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Rectangles `[x1, y1, x2, y2]` (pixels) and colours (0xAARRGGBB) for `bar` with its first
/// cell at pixel (x, y) and `size`-pixel cells: half a cell tall, in the middle of the row.
pub fn bar_rects(bar: &Bar, x: f32, y: f32, size: f32) -> Vec<([f32; 4], u32)> {
    const BACK: u32 = 0xFF30_3A48;
    const MIDDLE: u32 = 0xFFE0_E0E0;
    const MARK: u32 = 0xFF80_8C9C;
    let (x1, x2) = (x, x + bar.cells as f32 * size);
    let (y1, y2) = (y + size * 0.25, y + size * 0.75);
    let line = (size / 10.0).round().max(1.0);
    let mut out = vec![([x1, y1, x2, y2], BACK)];
    match bar.gauge {
        Gauge::Fill(f, tone) => {
            if f.is_finite() && f > 0.0 {
                out.push(([x1, y1, x1 + (x2 - x1) * f.min(1.0), y2], tone.colour()));
            }
        }
        Gauge::Signed {
            value,
            minus,
            plus,
            marks,
        } => {
            let mid = ((x1 + x2) / 2.0).round();
            let half = (x2 - x1) / 2.0;
            if value.is_finite() && value != 0.0 {
                let v = value.clamp(-1.0, 1.0);
                let end = mid + half * v;
                let (from, to, tone) = if v > 0.0 {
                    (mid, end, plus)
                } else {
                    (end, mid, minus)
                };
                out.push(([from, y1, to, y2], tone.colour()));
            }
            if marks {
                for m in [mid - half / 2.0, mid + half / 2.0] {
                    out.push(([m.round(), y1, m.round() + line, y2], MARK));
                }
            }
            out.push((
                [mid - line / 2.0, y1 - line, mid + line / 2.0, y2 + line],
                MIDDLE,
            ));
        }
    }
    out
}

/// "+42" / "-3" / "0".
fn signed_int(v: i32) -> String {
    if v == 0 { "0".into() } else { format!("{v:+}") }
}

/// Time of garden play: "about 5.2 h" or "about 25 min".
fn garden_time(ticks: u32) -> String {
    let h = stats::hours(ticks);
    if h >= 1.0 {
        format!("about {h:.1} h")
    } else {
        format!("about {} min", (h * 60.0).round() as u32)
    }
}

fn garden_name(garden: u8) -> &'static str {
    match garden {
        1 => "Neutral",
        2 => "Hero",
        3 => "Dark",
        _ => "Unknown",
    }
}

/// The lines of page `page` for Chao `c`, with what the mod knows of its battles.
pub fn lines(c: &ChaoData, page: Page, battle: &Battle) -> Vec<Line> {
    let rec = &c.bytes[..];
    if c.chao_type() == TYPE_EGG && page != Page::Raw {
        return vec![
            value("Form", "Egg"),
            value("Garden", garden_name(c.garden())),
            Line::new(),
            text("This Chao is still an egg."),
            text("Its abilities and traits begin when it hatches."),
        ];
    }
    match page {
        Page::Summary => summary(rec, battle),
        Page::Abilities => abilities(rec),
        Page::Growth => growth(rec),
        Page::Feelings => feelings(rec),
        Page::Personality => personality(rec),
        Page::Bonds => bonds(rec),
        Page::Battle => battle_page(battle),
        Page::Appearance => appearance(c),
        Page::Health => health(rec),
        Page::Raw => raw(c),
    }
}

/// The form, with how many lives an adult has had.
fn form_line(rec: &[u8]) -> Line {
    let kind = rec[0x80];
    let mut line = Line::new()
        .at(0, "Form", Tone::Dim)
        .at(13, stats::form(kind), Tone::Text);
    let lives = stats::reincarnations(rec);
    if lives > 0 {
        line = line.at(
            33,
            format!(
                "reincarnated {lives} time{}",
                if lives == 1 { "" } else { "s" }
            ),
            Tone::Dim,
        );
    }
    line
}

fn happiness_line(rec: &[u8]) -> Line {
    let h = stats::happiness(rec).clamp(-100, 100);
    Line::new()
        .at(0, "Happiness", Tone::Dim)
        .at(
            13,
            signed_int(i32::from(h)),
            if h >= 0 { Tone::Good } else { Tone::Bad },
        )
        .signed(18, 10, f32::from(h) / 100.0, (Tone::Bad, Tone::Good))
        .at(30, "Mood", Tone::Dim)
        .at(35, stats::mood(rec), Tone::Text)
}

/// A leaning's label, value, the names of its two ends and their colours.
type Leaning<'a> = (&'a str, Option<f32>, [&'a str; 2], (Tone, Tone));

/// Alignment (with its side) and the two leanings, each with its two ends around a bar.
fn leanings(rec: &[u8], child: bool) -> Vec<Line> {
    let rows: [Leaning; 3] = [
        (
            "Alignment",
            stats::alignment(rec),
            ["Dark", "Hero"],
            (Tone::Dark, Tone::Hero),
        ),
        (
            "Run/Power",
            stats::run_power(rec),
            ["Run", "Power"],
            (Tone::Accent, Tone::Accent),
        ),
        (
            "Swim/Fly",
            stats::swim_fly(rec),
            ["Swim", "Fly"],
            (Tone::Accent, Tone::Accent),
        ),
    ];
    let shown = if child { 3 } else { 1 };
    rows.into_iter()
        .take(shown)
        .enumerate()
        .map(|(i, (label, v, [minus, plus], tones))| {
            let line = Line::new().at(0, label, Tone::Dim);
            let Some(v) = v else {
                return line.at(13, "not set", Tone::Dim);
            };
            let v = v.clamp(-1.0, 1.0);
            let word = if i == 0 {
                let side = Side::of(v);
                Some((
                    side.name(),
                    match side {
                        Side::Hero => Tone::Hero,
                        Side::Dark => Tone::Dark,
                        Side::Neutral => Tone::Text,
                    },
                ))
            } else if v.abs() >= stats::LEANING {
                Some((if v > 0.0 { plus } else { minus }, Tone::Text))
            } else {
                None
            };
            let mut line = line
                .at(13, format!("{v:+.2}"), Tone::Text)
                .at(27, format!("{minus:>4}"), Tone::Dim)
                .leaning(32, 16, v, tones)
                .at(49, plus, Tone::Dim);
            if let Some((word, tone)) = word {
                line = line.at(19, word, tone);
            }
            line
        })
        .collect()
}

/// A child's way to its cocoon and what it would become now.
fn evolution_line(rec: &[u8]) -> Line {
    let line = Line::new().at(0, "Evolution", Tone::Dim);
    let Some(f) = stats::evolution(rec) else {
        return line.at(13, "not set", Tone::Dim);
    };
    let line = line
        .at(13, format!("{}%", (f * 100.0).floor() as u32), Tone::Text)
        .fill(18, 10, f, Tone::Accent);
    match stats::evolves_into(rec) {
        Some(kind) => line.at(29, format!("becomes {}", stats::form(kind)), Tone::Text),
        None => line,
    }
}

fn life_line(rec: &[u8]) -> Line {
    let line = Line::new().at(0, "Life", Tone::Dim);
    match stats::life_left(rec) {
        LifeLeft::NotYet => line.at(13, "starts counting down once it evolves", Tone::Dim),
        LifeLeft::Ageless => line.at(13, "does not age", Tone::Dim),
        LifeLeft::Ticks(ticks) => {
            let blockers = stats::rebirth_blockers(rec);
            let then = match blockers.first() {
                None => ("then reincarnates", Tone::Good),
                Some(b) if b.starts_with("happiness") => ("then dies (too unhappy)", Tone::Bad),
                Some(_) => ("then dies (too stressed)", Tone::Bad),
            };
            line.at(13, format!("{} left", garden_time(ticks)), Tone::Text)
                .at(33, then.0, then.1)
        }
    }
}

fn partner_text(p: &Partner) -> String {
    match p {
        Partner::None => "none".into(),
        Partner::Types(t) => t.join("/"),
        Partner::NoCatalog => "unknown (creature link not ready)".into(),
    }
}

fn battle_line(b: &Battle) -> Line {
    let line = Line::new()
        .at(0, "Battle", Tone::Dim)
        .at(13, format!("Lv {}", b.level), Tone::Text);
    let line = match b.xp {
        Some((_, 0)) => line.at(20, "top level", Tone::Dim),
        Some((xp, step)) => {
            let f = xp as f32 / step as f32;
            line.fill(20, 10, f, Tone::Accent).at(
                31,
                format!("{}%", (f * 100.0).floor() as u32),
                Tone::Text,
            )
        }
        None => line,
    };
    line.at(36, "Partner", Tone::Dim).at(
        44,
        clip(&partner_text(&b.partner), COLUMNS - 44),
        Tone::Text,
    )
}

/// One ability's row: name, grade letter, level, points, a bar of the level's progress and its
/// percentage (`per_level` adds the points a level gives at its grade).
fn ability_line(rec: &[u8], i: usize, per_level: bool) -> Line {
    let name = ABILITIES[i];
    let line = Line::new().at(0, name, Tone::Text);
    let Some(a) = Ability::read(rec, i) else {
        return line.at(14, "not set yet", Tone::Dim);
    };
    let grade = Tone::Grade(a.grade);
    let mut line = line
        .at(14, a.letter().to_string(), grade)
        .at(18, format!("Lv {:>2}", a.level), Tone::Text)
        .at(25, format!("{:>4}", a.points), Tone::Text);
    let (bar, cells) = if per_level {
        let (lo, hi) = stats::points_per_level(a.grade);
        line = line.at(31, format!("+{lo}-{hi}"), Tone::Dim);
        (41, 12)
    } else {
        (31, 20)
    };
    let percent = if a.level >= stats::LEVEL_CAP {
        "max".to_string()
    } else {
        format!("{}%", a.progress)
    };
    line.fill(bar, cells, a.fraction(), grade)
        .at(bar + cells + 1, percent, Tone::Text)
}

fn summary(rec: &[u8], battle: &Battle) -> Vec<Line> {
    let child = rec[0x80] == crate::chao_sim::record::TYPE_CHILD;
    let mut out = vec![form_line(rec), happiness_line(rec), Line::new()];
    out.push(
        Line::new()
            .at(0, "Ability", Tone::Dim)
            .at(12, "Grade", Tone::Dim)
            .at(18, "Level", Tone::Dim)
            .at(24, "Points", Tone::Dim)
            .at(31, "To the next level", Tone::Dim),
    );
    out.extend((0..ABILITIES.len()).map(|i| ability_line(rec, i, false)));
    out.push(Line::new());
    out.extend(leanings(rec, child));
    if child {
        out.push(evolution_line(rec));
    }
    out.push(life_line(rec));
    out.push(battle_line(battle));
    out
}

fn abilities(rec: &[u8]) -> Vec<Line> {
    let mut out = vec![
        head("Abilities: grade, level, points and progress"),
        Line::new()
            .at(0, "Ability", Tone::Dim)
            .at(12, "Grade", Tone::Dim)
            .at(18, "Level", Tone::Dim)
            .at(24, "Points", Tone::Dim)
            .at(31, "Per level", Tone::Dim)
            .at(41, "To the next level", Tone::Dim),
    ];
    out.extend((0..ABILITIES.len()).map(|i| ability_line(rec, i, true)));
    out.extend([
        Line::new(),
        dim("Each level up adds points by grade: E 11-15, D 14-18,"),
        dim("C 17-21, B 20-24, A 23-27, S 26-30, X 29-33."),
        dim(format!(
            "Points stop at {} and levels at {}.",
            stats::POINTS_CAP,
            stats::LEVEL_CAP
        )),
        dim("Fruit, animals and drives raise abilities. Evolving raises"),
        dim("one grade; grades carry over into the next life."),
    ]);
    out
}

fn growth(rec: &[u8]) -> Vec<Line> {
    let child = rec[0x80] == crate::chao_sim::record::TYPE_CHILD;
    let mut out = vec![head("Evolution, alignment and life"), form_line(rec)];
    if child {
        out.push(evolution_line(rec));
    }
    out.extend(leanings(rec, true));
    out.extend([
        dim("Hero from +0.50, Dark from -0.50, Neutral in between."),
        dim("A leaning of 0.50 or more makes a Run, Power, Swim or Fly"),
        dim("Chao (the stronger wins); otherwise Normal, or Chaos."),
        Line::new(),
        head("Chaos needs all three:"),
    ]);
    let animals = stats::animals(rec);
    let happiness = stats::happiness(rec);
    let lives = stats::reincarnations(rec);
    for (done, what, now) in [
        (
            animals == stats::ANIMAL_COUNT,
            "All 21 animals absorbed",
            format!("{animals} of 21"),
        ),
        (
            happiness > stats::CHAOS_HAPPINESS,
            "Happiness above 80",
            signed_int(i32::from(happiness)),
        ),
        (
            lives >= stats::CHAOS_REINCARNATIONS,
            "Reincarnated at least twice",
            lives.to_string(),
        ),
    ] {
        out.push(
            Line::new()
                .at(
                    2,
                    if done { "[x]" } else { "[ ]" },
                    if done { Tone::Good } else { Tone::Dim },
                )
                .at(6, what, Tone::Text)
                .at(36, now, Tone::Text),
        );
    }
    out.push(Line::new());
    let ticks = |n: u16| format!("{n} ({} in the garden)", garden_time(u32::from(n)));
    out.push(value("Age", ticks(stats::age(rec))));
    if !child {
        out.push(value("Adult age", ticks(stats::adult_age(rec))));
    }
    let (life, max) = stats::life(rec);
    out.push(value("Lifespan", format!("{life} (maximum {max})")));
    out.push(life_line(rec));
    let blockers = stats::rebirth_blockers(rec);
    out.push(if blockers.is_empty() {
        Line::new().at(0, "Rebirth", Tone::Dim).at(
            LABEL,
            "ready (happy and calm enough)",
            Tone::Good,
        )
    } else {
        Line::new().at(0, "Rebirth", Tone::Dim).at(
            LABEL,
            format!("needs {}", blockers.join(" and ")),
            Tone::Bad,
        )
    });
    out
}

/// A percentage row: label, value, bar and a note.
fn percent_line(label: &str, percent: u16, note: &str) -> Line {
    Line::new()
        .at(0, label, Tone::Dim)
        .at(17, format!("{percent:>3}%"), Tone::Text)
        .fill(23, 16, f32::from(percent) / 100.0, Tone::Accent)
        .at(41, note, Tone::Dim)
}

fn feelings(rec: &[u8]) -> Vec<Line> {
    let mut out = vec![
        Line::new()
            .at(0, "Mood", Tone::Dim)
            .at(17, stats::mood(rec), Tone::Text),
        head("Emotions"),
    ];
    for (i, (label, note)) in [
        ("Joy", ""),
        ("Anger", ""),
        ("Sorrow", ""),
        ("Fear", ""),
        ("Pain / surprise", ""),
        ("Dizziness", ""),
        ("Relax", "full while held"),
        ("All emotions", ""),
    ]
    .iter()
    .enumerate()
    {
        out.push(percent_line(label, stats::emotion_percent(rec, i), note));
    }
    out.push(head("Needs"));
    for (i, (label, note)) in [
        ("Sleepiness", ""),
        ("Sleep depth", ""),
        ("Hunger", ""),
        ("Breeding urge", "mates at 100%"),
        ("Boredom", ""),
        ("Loneliness", ""),
        ("Tiredness", ""),
        ("Stress", "rebirth: under 7%"),
        ("Nourishment", ""),
        ("Condition", ""),
        ("Thirst", ""),
    ]
    .iter()
    .enumerate()
    {
        out.push(percent_line(
            label,
            stats::need_percent(rec, stats::NEEDS + 2 * i),
            note,
        ));
    }
    out
}

/// A personality value (−80, −40, 0, 40 or 80 from the game) in words.
fn trait_word(v: i8) -> &'static str {
    match v {
        ..=-60 => "Very low",
        -59..=-20 => "Low",
        -19..=19 => "Average",
        20..=59 => "High",
        60.. => "Very high",
    }
}

/// The fruit kind a taste byte `rec+0x157` makes a Chao refuse (`0x5607C0`): none after a
/// rebirth (39).
fn disliked_fruit(taste: u8) -> Option<u8> {
    match taste {
        2 | 4 => Some(7),
        0 | 5 => Some(8),
        1 | 3 => Some(9),
        _ => None,
    }
}

/// The shop fruits a taste can rule out, by the name messages their item rows give
/// (`0x8A8028 + kind * 16`, +0xA: 38..40 in MsgAlItem).
fn fruit_name(kind: u8) -> String {
    match kind {
        7 => "Round fruit".into(),
        8 => "Triangle fruit".into(),
        9 => "Square fruit".into(),
        _ => format!("fruit kind {kind}"),
    }
}

fn personality(rec: &[u8]) -> Vec<Line> {
    let mut out = vec![head("Personality (can change at rebirth)")];
    for (i, label) in [
        "Curiosity",
        "Kindness",
        "Crybaby / energetic",
        "Naive / normal",
        "Solitude",
        "Vitality",
        "Appetite",
        "Recovery",
        "Skillfulness",
        "Charm",
        "Chattiness",
        "Carefree",
        "Fickleness",
    ]
    .iter()
    .enumerate()
    {
        let v = rec[0x14A + i] as i8;
        out.push(
            Line::new()
                .at(0, *label, Tone::Dim)
                .at(21, trait_word(v), Tone::Text)
                .signed(31, 12, f32::from(v) / 100.0, (Tone::Accent, Tone::Accent))
                .at(45, signed_int(i32::from(v)), Tone::Dim),
        );
    }
    out.push(Line::new());
    out.push(match disliked_fruit(rec[0x157]) {
        Some(kind) => Line::new()
            .at(0, "Dislikes", Tone::Dim)
            .at(21, fruit_name(kind), Tone::Text),
        None => Line::new()
            .at(0, "Dislikes", Tone::Dim)
            .at(21, "no fruit", Tone::Text),
    });
    match rec[0x80] {
        6..=22 if (rec[0x80] - 6).is_multiple_of(3) => {
            out.push(dim("As a Hero Chao it refuses dark fruit."))
        }
        7..=22 if (rec[0x80] - 7).is_multiple_of(3) => {
            out.push(dim("As a Dark Chao it refuses hero fruit."))
        }
        _ => {}
    }
    out.extend([
        dim("It won't eat a disliked fruit: it throws it away"),
        dim("and loses a little happiness."),
    ]);
    out
}

const CHARACTERS: [&str; 6] = ["Sonic", "Shadow", "Tails", "Eggman", "Knuckles", "Rouge"];
/// Above this fear a Chao is scared of the character (re/chao-ai.md).
const SCARED: u8 = 75;

fn bonds(rec: &[u8]) -> Vec<Line> {
    let mut out = vec![
        head("Bonds with each character"),
        Line::new()
            .at(0, "Character", Tone::Dim)
            .at(10, "Like", Tone::Dim)
            .at(27, "Fear", Tone::Dim)
            .at(50, "Interactions", Tone::Dim),
    ];
    for (i, name) in CHARACTERS.iter().enumerate() {
        let at = 0x16C + i * 6;
        let like = (rec[at] as i8).clamp(-100, 100);
        let fear = rec[at + 1];
        let mut line = Line::new()
            .at(0, *name, Tone::Text)
            .at(9, format!("{:>4}", signed_int(i32::from(like))), Tone::Text)
            .signed(14, 10, f32::from(like) / 100.0, (Tone::Bad, Tone::Good))
            .at(27, format!("{fear:>3}"), Tone::Text)
            .fill(31, 10, f32::from(fear) / 200.0, Tone::Bad)
            .at(
                50,
                format!("{:>5}", u16::from_le_bytes([rec[at + 4], rec[at + 5]])),
                Tone::Text,
            );
        if fear > SCARED {
            line = line.at(42, "scared", Tone::Bad);
        }
        out.push(line);
    }
    out.extend([
        Line::new(),
        dim("Like: -100 to +100. Fear: 0 to 200; above 75 it is scared."),
        dim("Kind acts lower fear; hits and throws raise it."),
    ]);
    out
}

fn battle_page(b: &Battle) -> Vec<Line> {
    let mut out = vec![
        head("Battles (kept by the mod, not in the SA2 save)"),
        value("Battle level", b.level),
    ];
    out.push(match b.xp {
        Some((_, 0)) => value("XP", "top level"),
        Some((xp, step)) => Line::new()
            .at(0, "XP", Tone::Dim)
            .at(LABEL, format!("{xp} of {step}"), Tone::Text)
            .fill(LABEL + 18, 12, xp as f32 / step as f32, Tone::Accent)
            .at(LABEL + 31, format!("to Lv {}", b.level + 1), Tone::Dim),
        None => value("XP", "unknown"),
    });
    out.push(value("Partner", partner_text(&b.partner)));
    out.push(Line::new());
    out.push(head("Moves it battles with"));
    if b.moves.is_empty() {
        out.push(text("  No moves learned yet: it battles with basic moves."));
    }
    for (i, name) in b.moves.iter().enumerate() {
        out.push(Line::new().at(2, (i + 1).to_string(), Tone::Dim).at(
            5,
            clip(name, COLUMNS - 5),
            Tone::Text,
        ));
    }
    out.push(value("Moves known", b.known));
    out.extend([
        Line::new(),
        dim("A garden's link machine picks the moves it battles with."),
    ]);
    out
}

fn appearance(c: &ChaoData) -> Vec<Line> {
    let mut out = vec![
        head("Appearance"),
        value(
            "Animals",
            format!("{} of 21 absorbed", stats::animals(&c.bytes)),
        ),
    ];
    for (label, at) in [
        ("Eyes", 0xD1),
        ("Mouth", 0xD2),
        ("Emotion ball", 0xD3),
        ("Hat", 0xD5),
        ("Hide feet", 0xD6),
        ("Medal", 0xD7),
        ("Colour", 0xD8),
        ("Monotone", 0xD9),
        ("Texture", 0xDA),
        ("Shiny", 0xDB),
        ("Egg colour", 0xDC),
        ("Body type", 0xDD),
        ("Animal body", 0xDE),
    ] {
        out.push(value(label, c.bytes[at]));
    }
    out.push(head("Animal parts"));
    for (i, label) in [
        "Arms", "Ears", "Forehead", "Horns", "Legs", "Tail", "Wings", "Face",
    ]
    .iter()
    .enumerate()
    {
        let part = c.bytes[0x11C + i] as i8;
        out.push(value(
            label,
            if part < 0 {
                "none".into()
            } else {
                format!("animal {part}")
            },
        ));
    }
    out
}

fn health(rec: &[u8]) -> Vec<Line> {
    let mut out = vec![head("Health")];
    for (i, label) in [
        "Cough",
        "Cold",
        "Rash",
        "Runny nose",
        "Hiccups",
        "Stomach ache",
    ]
    .iter()
    .enumerate()
    {
        let v = rec[0x15A + i] as i8;
        out.push(if v == 0 {
            Line::new()
                .at(0, *label, Tone::Dim)
                .at(LABEL, "fine", Tone::Good)
        } else {
            Line::new()
                .at(0, *label, Tone::Dim)
                .at(LABEL, "has it", Tone::Bad)
                .at(LABEL + 8, format!("({v})"), Tone::Dim)
        });
    }
    out.extend([
        Line::new(),
        dim("An illness wears off by itself: its counter moves back to 0."),
    ]);
    out
}

fn raw(c: &ChaoData) -> Vec<Line> {
    let mut out = vec![
        dim("Full read-only record, including unknown/unused fields."),
        dim("Offsets at left; bytes are hexadecimal. Up/Down scroll."),
    ];
    for (i, bytes) in c.bytes.chunks(16).enumerate() {
        let mut row = format!("{:03X}:", i * 16);
        for byte in bytes {
            let _ = write!(row, " {byte:02X}");
        }
        out.push(text(row));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; crate::chao::CHAO_SIZE],
        };
        c.bytes[0x80] = 2;
        c.bytes[0x81] = 1;
        c
    }

    fn all_text(lines: &[Line]) -> String {
        lines.iter().map(Line::text).collect::<Vec<_>>().join("\n")
    }

    fn battle() -> Battle {
        Battle {
            level: 23,
            xp: Some((460, 1000)),
            partner: Partner::Types(vec!["Electric".into(), "Fighting".into()]),
            moves: vec!["ABCDEFGHIJKL".into(), "Move #12".into()],
            known: 5,
        }
    }

    /// A Chao with every field at its widest.
    fn widest() -> ChaoData {
        let mut c = sample();
        for i in 0..7 {
            c.bytes[0x28 + i] = 6;
            c.bytes[0x30 + i] = 99;
            c.bytes[0x38 + 2 * i..0x3A + 2 * i].copy_from_slice(&9999u16.to_le_bytes());
            c.bytes[0x20 + i] = 99;
        }
        c.bytes[0x82..0x84].copy_from_slice(&(-100i16).to_le_bytes());
        for at in [0xA8, 0xAC, 0xB0] {
            c.bytes[at..at + 4].copy_from_slice(&(-1.0f32).to_le_bytes());
        }
        c.bytes[0x8E..0x90].copy_from_slice(&65535u16.to_le_bytes());
        c.bytes[0x86..0x88].copy_from_slice(&65535u16.to_le_bytes());
        c.bytes[0x88..0x8A].copy_from_slice(&65535u16.to_le_bytes());
        c.bytes[0x8A..0x8C].copy_from_slice(&9999u16.to_le_bytes());
        c.bytes[0x8C..0x8E].copy_from_slice(&9999u16.to_le_bytes());
        c.bytes[0x142..0x144].copy_from_slice(&10000u16.to_le_bytes());
        for i in 0..6 {
            c.bytes[0x16C + i * 6] = (-100i8) as u8;
            c.bytes[0x16D + i * 6] = 200;
            c.bytes[0x170 + i * 6..0x172 + i * 6].copy_from_slice(&65535u16.to_le_bytes());
            c.bytes[0x15A + i] = (-100i8) as u8;
        }
        for i in 0..8 {
            c.bytes[0x11C + i] = 20;
        }
        c
    }

    #[test]
    fn every_page_fits_the_panel_without_overlaps() {
        let mut child = widest();
        let mut adult = widest();
        adult.bytes[0x80] = 22;
        let mut mortal = widest();
        mortal.bytes[0x80] = 19;
        for c in [&mut child, &mut adult, &mut mortal] {
            for page in Page::ALL {
                for b in [battle(), Battle::default()] {
                    let lines = lines(c, page, &b);
                    for line in &lines {
                        let mut cells = line.cells();
                        cells.sort();
                        for (start, end) in &cells {
                            assert!(*end <= COLUMNS, "{page:?}: {}", line.text());
                            assert!(start <= end);
                        }
                        for pair in cells.windows(2) {
                            assert!(pair[0].1 <= pair[1].0, "{page:?}: {}", line.text());
                        }
                    }
                    if page != Page::Raw {
                        assert!(lines.len() <= 24, "{page:?}: {} lines", lines.len());
                    }
                }
            }
        }
    }

    #[test]
    fn the_summary_shows_every_ability_with_its_grade_and_level() {
        let mut c = sample();
        for i in 0..7 {
            c.bytes[0x28 + i] = i as u8;
            c.bytes[0x30 + i] = 10 + i as u8;
            c.bytes[0x38 + 2 * i..0x3A + 2 * i].copy_from_slice(&(100 * i as u16).to_le_bytes());
            c.bytes[0x20 + i] = 40;
        }
        c.bytes[0x30 + 6] = 99;
        c.bytes[0xA8..0xAC].copy_from_slice(&0.7f32.to_le_bytes());
        c.bytes[0xB0..0xB4].copy_from_slice(&0.6f32.to_le_bytes());
        c.bytes[0xC0..0xC4].copy_from_slice(&0.74f32.to_le_bytes());
        let lines = lines(&c, Page::Summary, &battle());
        let text = all_text(&lines);
        for (name, letter, level) in [
            ("Swim", 'E', 10),
            ("Fly", 'D', 11),
            ("Run", 'C', 12),
            ("Power", 'B', 13),
            ("Stamina", 'A', 14),
            ("Luck", 'S', 15),
        ] {
            let row = lines
                .iter()
                .map(Line::text)
                .find(|t| t.starts_with(name))
                .unwrap();
            assert!(row.contains(&format!(" {letter} ")), "{row}");
            assert!(row.contains(&format!("Lv {level}")), "{row}");
            assert!(row.ends_with("40%"), "{row}");
        }
        let intel = lines
            .iter()
            .find(|l| l.text().starts_with("Intelligence"))
            .unwrap();
        assert!(intel.text().contains(" X ") && intel.text().ends_with("max"));
        // Grade letters and bars are in the grade's colour.
        assert!(intel.spans.iter().any(|s| s.tone == Tone::Grade(6)));
        assert!(text.contains("becomes Hero Power"), "{text}");
        assert!(text.contains("74%"));
        assert!(text.contains("Hero") && text.contains("+0.60"));
        assert!(text.contains("Lv 23") && text.contains("46%"));
        assert!(text.contains("Electric/Fighting"));
        assert!(text.contains("starts counting down once it evolves"));
    }

    #[test]
    fn an_adults_summary_shows_its_form_and_time_left() {
        let mut c = sample();
        c.bytes[0x80] = 6 + 3 * 2; // Hero Fly
        c.bytes[0x8A..0x8C].copy_from_slice(&3000u16.to_le_bytes());
        c.bytes[0x8C..0x8E].copy_from_slice(&3000u16.to_le_bytes());
        c.bytes[0x8E..0x90].copy_from_slice(&1u16.to_le_bytes());
        c.bytes[0x82..0x84].copy_from_slice(&60i16.to_le_bytes());
        let text = all_text(&lines(&c, Page::Summary, &Battle::default()));
        assert!(text.contains("Hero Fly") && text.contains("reincarnated 1 time"));
        assert!(text.contains("about 11.9 h left") && text.contains("then reincarnates"));
        assert!(!text.contains("Run/Power"));
        assert!(text.contains("Lv 5") && text.contains("Partner") && text.contains("none"));
        c.bytes[0x82..0x84].copy_from_slice(&20i16.to_le_bytes());
        let text = all_text(&lines(&c, Page::Summary, &Battle::default()));
        assert!(text.contains("then dies (too unhappy)"));
    }

    #[test]
    fn inspection_reads_signed_and_unsigned_fields_and_changes_nothing() {
        let mut c = sample();
        c.bytes[0x14C] = (-80i8) as u8;
        c.bytes[0x12C] = 200;
        c.bytes[0x16C] = (-100i8) as u8;
        c.bytes[0x16D] = 200;
        c.bytes[0x138..0x13A].copy_from_slice(&10000u16.to_le_bytes());
        let before = c.bytes;
        let traits = all_text(&lines(&c, Page::Personality, &Battle::default()));
        assert!(traits.contains("Very low") && traits.contains("-80"));
        let feelings = all_text(&lines(&c, Page::Feelings, &Battle::default()));
        assert!(
            feelings
                .lines()
                .any(|l| l.starts_with("Joy") && l.contains("100%"))
        );
        assert!(
            feelings
                .lines()
                .any(|l| l.starts_with("Hunger") && l.contains("100%"))
        );
        let bonds = all_text(&lines(&c, Page::Bonds, &Battle::default()));
        assert!(
            bonds
                .lines()
                .any(|l| l.starts_with("Sonic") && l.contains("-100") && l.contains("200")),
            "{bonds}"
        );
        assert!(bonds.contains("scared"));
        let mut view = View::default();
        for page in Page::ALL {
            let _ = lines(&c, page, &battle());
            view.page = page;
            let _ = screen(Some(&c), "Chao", Some(0), 1, &mut view, 24);
        }
        assert_eq!(c.bytes, before);
    }

    #[test]
    fn eggs_and_unset_values_are_not_reported_as_real_stats() {
        let mut c = sample();
        c.bytes[0x80] = TYPE_EGG;
        c.bytes[0x2E] = 0x48;
        assert!(all_text(&lines(&c, Page::Abilities, &battle())).contains("egg"));
        assert!(all_text(&lines(&c, Page::Summary, &battle())).contains("egg"));
        c.bytes[0x80] = 2;
        c.bytes[0xB0..0xB4].copy_from_slice(&f32::NAN.to_le_bytes());
        let summary = all_text(&lines(&c, Page::Summary, &battle()));
        assert!(summary.contains("Alignment    not set"), "{summary}");
        // A grade past X levels all the same: '?' with its level and points.
        let abilities = all_text(&lines(&c, Page::Abilities, &battle()));
        assert!(abilities.contains("Intelligence  ?"), "{abilities}");
        assert!(!abilities.contains("not set yet"), "{abilities}");
        c.bytes[0x36] = 200;
        assert!(
            all_text(&lines(&c, Page::Abilities, &battle())).contains("Intelligence  not set yet")
        );
    }

    #[test]
    fn personality_words_and_fruit_dislikes() {
        assert_eq!(trait_word(-80), "Very low");
        assert_eq!(trait_word(-40), "Low");
        assert_eq!(trait_word(0), "Average");
        assert_eq!(trait_word(40), "High");
        assert_eq!(trait_word(80), "Very high");
        assert_eq!(disliked_fruit(2), Some(7));
        assert_eq!(disliked_fruit(5), Some(8));
        assert_eq!(disliked_fruit(3), Some(9));
        assert_eq!(fruit_name(7), "Round fruit");
        assert_eq!(fruit_name(9), "Square fruit");
        // After a rebirth the taste byte is 39: it eats everything.
        assert_eq!(disliked_fruit(39), None);
    }

    #[test]
    fn browsing_tracks_save_slots_and_survives_a_removed_chao() {
        let mut view = View {
            slot: Some(7),
            ..Default::default()
        };
        view.select(&[2, 7, 20], false);
        assert_eq!(view.slot, Some(20));
        view.select(&[2, 7, 20], false);
        assert_eq!(view.slot, Some(2));
        view.select(&[2, 7, 20], true);
        assert_eq!(view.slot, Some(20));
        view.retain_slot(&[2, 7]);
        assert_eq!(view.slot, Some(2));
        view.retain_slot(&[]);
        assert_eq!(view.slot, None);
        assert_eq!(Page::Summary.next(true), Page::Raw);
        assert_eq!(Page::Raw.next(false), Page::Summary);
        view.scroll = 5;
        view.turn_page(false);
        assert_eq!((view.page, view.scroll), (Page::Abilities, 0));
    }

    #[test]
    fn form_ids_account_for_the_two_legacy_forms_before_normal_types() {
        assert_eq!(stats::form(6), "Hero Normal");
        assert_eq!(stats::form(20), "Neutral Chaos");
        assert_eq!(stats::form(25), "Amy");
        assert!(stats::form(255).contains("Unknown"));
    }

    #[test]
    fn raw_view_includes_the_entire_record_through_the_final_byte() {
        let mut c = sample();
        for (i, byte) in c.bytes.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let rows = lines(&c, Page::Raw, &Battle::default());
        assert_eq!(rows.len(), 2 + crate::chao::CHAO_SIZE / 16);
        assert!(rows[2].text().starts_with("000: 00 01 02"));
        assert!(rows.last().unwrap().text().starts_with("7F0: F0 F1 F2"));
        assert!(rows.last().unwrap().text().ends_with("FD FE FF"));
    }

    #[test]
    fn the_screen_fits_small_windows_and_large_displays() {
        for (width, height) in [
            (320.0, 240.0),
            (640.0, 480.0),
            (1280.0, 720.0),
            (1920.0, 1080.0),
            (2560.0, 1440.0),
            (3840.0, 2160.0),
        ] {
            let (font, visible) = layout(width, height);
            assert!(PANEL_COLUMNS as f32 * font <= width);
            let mut view = View {
                page: Page::Raw,
                scroll: 9999,
                ..Default::default()
            };
            let c = sample();
            let screen = screen(
                Some(&c),
                "Chao (Hero garden, slot 1)",
                Some(0),
                3,
                &mut view,
                visible,
            );
            // The scroll is kept in range and the last rows shown.
            assert_eq!(view.scroll, 2 + crate::chao::CHAO_SIZE / 16 - visible);
            // A row above the panel and its two margin rows.
            assert!(
                (screen.rows.len() + 3) as f32 * font <= height,
                "{width}x{height}: {} rows of {font}",
                screen.rows.len()
            );
            for row in &screen.rows {
                assert!(
                    row.cells().iter().all(|&(_, end)| end <= COLUMNS),
                    "{}",
                    row.text()
                );
            }
        }
        // The readable sizes: 27 px at 1080p, 36 at 1440p, 18 at 720p.
        assert_eq!(layout(1920.0, 1080.0).0, 27.0);
        assert_eq!(layout(2560.0, 1440.0).0, 36.0);
        assert_eq!(layout(1280.0, 720.0).0, 18.0);
    }

    #[test]
    fn the_tab_strip_fits_and_marks_the_current_page() {
        for page in Page::ALL {
            let (line, (col, width)) = tab_strip(page);
            assert!(line.cells().iter().all(|&(_, end)| end <= COLUMNS));
            assert_eq!(&line.text()[col..col + width], page.tab());
            assert_eq!(
                line.spans.iter().filter(|s| s.tone == Tone::Head).count(),
                1
            );
        }
    }

    #[test]
    fn bars_stay_inside_their_cells() {
        for gauge in [
            Gauge::Fill(1.5, Tone::Accent),
            Gauge::Fill(f32::NAN, Tone::Accent),
            Gauge::Signed {
                value: -3.0,
                minus: Tone::Dark,
                plus: Tone::Hero,
                marks: true,
            },
            Gauge::Signed {
                value: 0.4,
                minus: Tone::Dark,
                plus: Tone::Hero,
                marks: false,
            },
        ] {
            let bar = Bar {
                col: 3,
                cells: 10,
                gauge,
            };
            let rects = bar_rects(&bar, 100.0, 50.0, 20.0);
            for ([x1, y1, x2, y2], _) in &rects {
                assert!(x1 <= x2 && y1 <= y2);
                assert!(*x1 >= 99.0 && *x2 <= 301.0, "{gauge:?}: {rects:?}");
                assert!(*y1 >= 50.0 && *y2 <= 70.0, "{gauge:?}: {rects:?}");
            }
        }
        // A signed value fills from the middle toward its side.
        let bar = Bar {
            col: 0,
            cells: 10,
            gauge: Gauge::Signed {
                value: -0.5,
                minus: Tone::Dark,
                plus: Tone::Hero,
                marks: false,
            },
        };
        let rects = bar_rects(&bar, 0.0, 0.0, 10.0);
        assert_eq!(rects[1], ([25.0, 2.5, 50.0, 7.5], Tone::Dark.colour()));
    }
}
