use plugin_toolkit::prelude::*;

use crate::api::Book;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    plugin_toolkit::serde::Serialize,
    plugin_toolkit::serde::Deserialize,
    plugin_toolkit::schemars::JsonSchema,
)]
#[serde(crate = "plugin_toolkit::serde")]
#[schemars(crate = "plugin_toolkit::schemars")]
#[serde(rename_all = "lowercase")]
pub enum Availability {
    Have,
    Wanted,
    Skipped,
    Ignored,
}

pub fn availability(status: Option<&str>) -> Availability {
    match status.unwrap_or_default() {
        "Open" | "Have" => Availability::Have,
        "Wanted" | "Snatched" | "Matched" => Availability::Wanted,
        "Ignored" => Availability::Ignored,
        _ => Availability::Skipped,
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    plugin_toolkit::serde::Serialize,
    plugin_toolkit::serde::Deserialize,
    plugin_toolkit::schemars::JsonSchema,
)]
#[serde(crate = "plugin_toolkit::serde")]
#[schemars(crate = "plugin_toolkit::schemars")]
#[serde(rename_all = "snake_case")]
pub enum ParityGap {
    AudiobookMissing,
    EbookMissing,
    StatusDiffers,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct ParityRow {
    pub book: Book,
    pub ebook: Availability,
    pub audiobook: Availability,
    pub gap: ParityGap,
}

pub fn parity(books: Vec<Book>, include_ignored: bool) -> Vec<ParityRow> {
    books
        .into_iter()
        .filter_map(|book| {
            let ebook = availability(book.ebook_status.as_deref());
            let audiobook = availability(book.audio_status.as_deref());
            if ebook == audiobook {
                return None;
            }
            if !include_ignored
                && (ebook == Availability::Ignored || audiobook == Availability::Ignored)
            {
                return None;
            }
            let gap = match (ebook, audiobook) {
                (Availability::Have, _) => ParityGap::AudiobookMissing,
                (_, Availability::Have) => ParityGap::EbookMissing,
                _ => ParityGap::StatusDiffers,
            };
            Some(ParityRow {
                book,
                ebook,
                audiobook,
                gap,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(id: &str, ebook: &str, audio: &str) -> Book {
        Book {
            id: id.into(),
            title: id.into(),
            subtitle: None,
            author_id: None,
            author: None,
            isbn: None,
            language: None,
            published: None,
            ebook_status: Some(ebook.into()),
            audio_status: Some(audio.into()),
        }
    }

    #[test]
    fn statuses_bucket_into_availability() {
        assert_eq!(availability(Some("Open")), Availability::Have);
        assert_eq!(availability(Some("Snatched")), Availability::Wanted);
        assert_eq!(availability(Some("Ignored")), Availability::Ignored);
        assert_eq!(availability(None), Availability::Skipped);
    }

    #[test]
    fn parity_reports_only_mismatched_formats() {
        let rows = parity(
            vec![
                book("same", "Open", "Have"),
                book("no_audio", "Open", "Skipped"),
                book("no_ebook", "Wanted", "Open"),
                book("differs", "Wanted", "Skipped"),
                book("ignored", "Open", "Ignored"),
            ],
            false,
        );
        let got: Vec<(&str, ParityGap)> =
            rows.iter().map(|r| (r.book.id.as_str(), r.gap)).collect();
        assert_eq!(
            got,
            vec![
                ("no_audio", ParityGap::AudiobookMissing),
                ("no_ebook", ParityGap::EbookMissing),
                ("differs", ParityGap::StatusDiffers),
            ]
        );
    }

    #[test]
    fn parity_can_include_ignored() {
        let rows = parity(vec![book("ignored", "Open", "Ignored")], true);
        assert_eq!(rows[0].gap, ParityGap::AudiobookMissing);
    }
}
