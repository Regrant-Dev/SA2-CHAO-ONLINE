//! How the importer (`sa2co-import`) tells the mod what went wrong: one `error[code]: message`
//! line, the only line on its stderr. The mod reads the code back to choose what the player sees,
//! so the codes must stay stable.

/// What went wrong, for the mod's message to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The image file isn't there or can't be opened.
    Missing,
    /// Not a cartridge image (.3ds): a CIA installable, a bare NCCH or another file.
    Format,
    /// Encrypted, or damaged: a section's digest doesn't match its plaintext.
    Encrypted,
    /// A different title.
    WrongTitle,
    /// Update data, or a base image with an update applied.
    Update,
    /// The image changed while it was being read.
    Changed,
    /// Not enough free disk space for the cache.
    Space,
    /// The cache folder can't be used or written.
    Cache,
    /// The command line is wrong.
    Usage,
    /// Unreadable or unexpected data in the image.
    Invalid,
    /// The image read fine, but preparing the cache from it failed (a conversion, a write).
    Failed,
}

impl ErrorKind {
    pub const ALL: [Self; 11] = [
        Self::Missing,
        Self::Format,
        Self::Encrypted,
        Self::WrongTitle,
        Self::Update,
        Self::Changed,
        Self::Space,
        Self::Cache,
        Self::Usage,
        Self::Invalid,
        Self::Failed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Format => "format",
            Self::Encrypted => "encrypted",
            Self::WrongTitle => "wrong-title",
            Self::Update => "update",
            Self::Changed => "changed",
            Self::Space => "space",
            Self::Cache => "cache",
            Self::Usage => "usage",
            Self::Invalid => "invalid",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == code)
    }
}

/// The importer's error line for `kind`.
pub fn line(kind: ErrorKind, message: &str) -> String {
    format!("error[{}]: {message}", kind.code())
}

/// The kind in the importer's first error line, or none (no such line: it didn't finish).
pub fn parse(stderr: &str) -> Option<ErrorKind> {
    stderr
        .lines()
        .find_map(|l| l.trim().strip_prefix("error[")?.split_once(']'))
        .and_then(|(code, _)| ErrorKind::from_code(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reads_back_from_its_line() {
        for kind in ErrorKind::ALL {
            assert_eq!(parse(&line(kind, "details [with] brackets")), Some(kind));
            assert_eq!(
                parse(&format!("progress\n{}\n", line(kind, "x"))),
                Some(kind)
            );
        }
        let mut codes: Vec<_> = ErrorKind::ALL.iter().map(|k| k.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), ErrorKind::ALL.len());
        assert_eq!(parse("thread 'main' panicked at src/bin/importer.rs"), None);
        assert_eq!(parse("error[unknown]: x"), None);
    }
}
