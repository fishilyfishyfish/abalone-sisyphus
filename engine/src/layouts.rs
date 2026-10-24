//! 시작 배치. 흑은 A행 쪽(아래), 백은 I행 쪽(위)에서 시작한다.

use crate::board::*;
use crate::notation::parse_cell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    BelgianDaisy,
    Standard,
    GermanDaisy,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::BelgianDaisy => "belgian",
            Layout::Standard => "standard",
            Layout::GermanDaisy => "german",
        }
    }

    pub fn parse(s: &str) -> Option<Layout> {
        match s {
            "belgian" | "belgian-daisy" => Some(Layout::BelgianDaisy),
            "standard" => Some(Layout::Standard),
            "german" | "german-daisy" => Some(Layout::GermanDaisy),
            _ => None,
        }
    }

    pub fn cells(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Layout::Standard => (
                &[
                    "A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3", "B4", "B5", "B6", "C3", "C4",
                    "C5",
                ],
                &[
                    "I5", "I6", "I7", "I8", "I9", "H4", "H5", "H6", "H7", "H8", "H9", "G5", "G6",
                    "G7",
                ],
            ),
            Layout::BelgianDaisy => (
                &[
                    "A1", "A2", "B1", "B2", "B3", "C2", "C3", "I8", "I9", "H7", "H8", "H9", "G7",
                    "G8",
                ],
                &[
                    "A4", "A5", "B4", "B5", "B6", "C5", "C6", "I5", "I6", "H4", "H5", "H6", "G4",
                    "G5",
                ],
            ),
            Layout::GermanDaisy => (
                &[
                    "B1", "B2", "C1", "C2", "C3", "D2", "D3", "F7", "F8", "G7", "G8", "G9", "H8",
                    "H9",
                ],
                &[
                    "B5", "B6", "C5", "C6", "C7", "D6", "D7", "F3", "F4", "G3", "G4", "G5", "H4",
                    "H5",
                ],
            ),
        }
    }

    pub fn position(self) -> Position {
        let (b, w) = self.cells();
        Position::new(mask_of(b), mask_of(w), Color::Black)
    }
}

pub fn mask_of(cells: &[&str]) -> u128 {
    cells
        .iter()
        .map(|c| bit(parse_cell(c).unwrap_or_else(|| panic!("bad cell {c}"))))
        .fold(0, |a, b| a | b)
}
