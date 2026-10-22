//! Gen 3 GBA western character set.

/// Decodes a Gen 3 western string to UTF-8. Stops at the 0xFF terminator; unmapped bytes become '?'.
pub fn decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0xFF)
        .map(|&b| char_of(b).unwrap_or('?'))
        .collect()
}

/// Character for one byte, or `None` for control codes and unmapped glyphs.
fn char_of(b: u8) -> Option<char> {
    /// 0x01..=0x2B: accented Latin letters; '\0' marks unmapped slots.
    const ACCENTED: [char; 0x2B] = [
        'À', 'Á', 'Â', 'Ç', 'È', 'É', 'Ê', 'Ë', 'Ì', '\0', 'Î', 'Ï', 'Ò', 'Ó', 'Ô', 'Œ', 'Ù', 'Ú',
        'Û', 'Ñ', 'ß', 'à', 'á', '\0', 'ç', 'è', 'é', 'ê', 'ë', 'ì', '\0', 'î', 'ï', 'ò', 'ó', 'ô',
        'œ', 'ù', 'ú', 'û', 'ñ', 'º', 'ª',
    ];
    /// 0xA1..=0xBA: digits and punctuation.
    const PUNCT: [char; 0x1A] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '!', '?', '.', '-', '·', '…', '“', '”',
        '‘', '’', '♂', '♀', '¥', ',', '×', '/',
    ];
    /// 0xEF..=0xF6.
    const TAIL: [char; 8] = ['▶', ':', 'Ä', 'Ö', 'Ü', 'ä', 'ö', 'ü'];
    let c = match b {
        0x00 => ' ',
        0x01..=0x2B => ACCENTED[usize::from(b - 0x01)],
        0x2D => '&',
        0x2E => '+',
        0x35 => '=',
        0x36 => ';',
        0x51 => '¿',
        0x52 => '¡',
        0x5A => 'Í',
        0x5B => '%',
        0x5C => '(',
        0x5D => ')',
        0x68 => 'â',
        0x6F => 'í',
        0x85 => '<',
        0x86 => '>',
        0xA1..=0xBA => PUNCT[usize::from(b - 0xA1)],
        0xBB..=0xD4 => char::from(b'A' + (b - 0xBB)),
        0xD5..=0xEE => char::from(b'a' + (b - 0xD5)),
        0xEF..=0xF6 => TAIL[usize::from(b - 0xEF)],
        _ => return None,
    };
    (c != '\0').then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_terminator() {
        assert_eq!(
            decode(&[0xBB, 0xD4, 0xD5, 0xEE, 0x00, 0xA1, 0xAA, 0xFF, 0xBB]),
            "AZaz 09"
        );
    }

    #[test]
    fn symbols_and_unknown() {
        assert_eq!(
            decode(&[0xB5, 0xB6, 0xAB, 0xAC, 0xAD, 0xAE, 0xB8, 0xBA]),
            "♂♀!?.-,/"
        );
        assert_eq!(decode(&[0x01, 0x1B, 0xF6, 0xB4]), "Àéü’");
        assert_eq!(decode(&[0x0A, 0x40, 0xFC]), "???");
        assert_eq!(decode(&[]), "");
    }
}
