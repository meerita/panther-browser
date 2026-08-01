// @file products/panther/localization/src/regional-formatter.rs
// @description Formats values for one region locale through ICU4X.
// @created Diego Martín Lafuente <meerita@icloud.com>

use fixed_decimal::{Decimal, Sign, UnsignedDecimal};
use icu::datetime::fieldsets::{T, YMD};
use icu::datetime::input::{Date, Time};
use icu::datetime::{DateTimeFormatter, NoCalendarFormatter};
use icu::decimal::DecimalFormatter;
use icu::list::ListFormatter;
use icu::list::options::{ListFormatterOptions, ListLength};
use locale::Locale;

use crate::localization_error::LocalizationError;

/// The number of bytes in the next larger binary unit.
const BINARY_UNIT_STEP: f64 = 1024.0;

/// The binary size unit symbols, smallest first.
const SIZE_UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

/// Formats values for one region locale.
///
/// The formatter instances are built once for a region locale and reused for
/// every value, so no formatter is constructed per call. The region locale, not
/// the user-interface language, drives the output, so numbers, dates, times, and
/// lists follow regional convention. Duration and file size compose on the
/// decimal formatter, so their digits and separators localize with the region;
/// their unit symbols are not localized, because a stable ICU4X unit formatter
/// is not yet available.
pub struct RegionalFormatter {
    decimal: DecimalFormatter,
    list: ListFormatter,
    date: DateTimeFormatter<YMD>,
    time: NoCalendarFormatter<T>,
}

impl RegionalFormatter {
    /// Builds the formatter instances for a region locale.
    ///
    /// Construction is fail-safe for the caller: a locale without formatting
    /// data yields [`LocalizationError::FormatterUnavailable`], and the cache
    /// falls back to the reference locale rather than crash.
    pub fn new(region: &Locale) -> Result<Self, LocalizationError> {
        let decimal = DecimalFormatter::try_new(region.as_icu().into(), Default::default())
            .map_err(|_| LocalizationError::FormatterUnavailable)?;
        let list = ListFormatter::try_new_and(
            region.as_icu().into(),
            ListFormatterOptions::default().with_length(ListLength::Wide),
        )
        .map_err(|_| LocalizationError::FormatterUnavailable)?;
        let date = DateTimeFormatter::try_new(region.as_icu().into(), YMD::medium())
            .map_err(|_| LocalizationError::FormatterUnavailable)?;
        let time = NoCalendarFormatter::try_new(region.as_icu().into(), T::hm())
            .map_err(|_| LocalizationError::FormatterUnavailable)?;
        Ok(Self {
            decimal,
            list,
            date,
            time,
        })
    }

    /// Formats an integer with regional grouping separators.
    pub fn integer(&self, value: i64) -> String {
        self.decimal.format_to_string(&Decimal::from(value))
    }

    /// Formats a date given as an ISO year, month, and day.
    ///
    /// An out-of-range date yields an empty string rather than a panic, so an
    /// invalid input can never crash a caller.
    pub fn date(&self, year: i32, month: u8, day: u8) -> String {
        match Date::try_new_iso(year, month, day) {
            Ok(date) => self.date.format(&date).to_string(),
            Err(_) => String::new(),
        }
    }

    /// Formats a time given as an hour and minute.
    ///
    /// An out-of-range time yields an empty string rather than a panic.
    pub fn time(&self, hour: u8, minute: u8) -> String {
        match Time::try_new(hour, minute, 0, 0) {
            Ok(time) => self.time.format(&time).to_string(),
            Err(_) => String::new(),
        }
    }

    /// Formats a list of items with the regional conjunction.
    pub fn list(&self, items: &[String]) -> String {
        self.list.format_to_string(items.iter().map(String::as_str))
    }

    /// Formats a duration as an hour, minute, and second clock value.
    pub fn duration(&self, total_seconds: u64) -> String {
        let hours = self
            .decimal
            .format_to_string(&Decimal::from((total_seconds / 3600) as i64));
        let minutes = self.clock_component((total_seconds % 3600) / 60);
        let seconds = self.clock_component(total_seconds % 60);
        format!("{hours}:{minutes}:{seconds}")
    }

    fn clock_component(&self, value: u64) -> String {
        let padded = UnsignedDecimal::from(value).padded_start(2);
        self.decimal
            .format_to_string(&Decimal::new(Sign::None, padded))
    }

    /// Formats a byte count as a binary file size.
    ///
    /// The numeric part localizes with the region. The unit symbol is not
    /// localized, because a stable ICU4X unit formatter is not yet available.
    pub fn file_size(&self, bytes: u64) -> String {
        let mut value = bytes as f64;
        let mut unit = 0;
        while value >= BINARY_UNIT_STEP && unit < SIZE_UNITS.len() - 1 {
            value /= BINARY_UNIT_STEP;
            unit += 1;
        }
        let number = if unit == 0 {
            self.integer(bytes as i64)
        } else {
            let text = format!("{value:.1}");
            match text.parse::<Decimal>() {
                Ok(decimal) => self.decimal.format_to_string(&decimal),
                Err(_) => text,
            }
        };
        format!("{number} {}", SIZE_UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::RegionalFormatter;
    use locale::Locale;

    fn formatter(identifier: &str) -> RegionalFormatter {
        RegionalFormatter::new(&Locale::parse(identifier).expect("valid identifier"))
            .expect("compiled formatting data")
    }

    #[test]
    fn integer_grouping_differs_by_region() {
        let english = formatter("en-US");
        let german = formatter("de-DE");
        assert_ne!(english.integer(1_234_567), german.integer(1_234_567));
    }

    #[test]
    fn date_differs_by_region() {
        let english = formatter("en-US");
        let german = formatter("de-DE");
        assert_ne!(english.date(2025, 1, 15), german.date(2025, 1, 15));
    }

    #[test]
    fn file_size_decimal_separator_differs_by_region() {
        let english = formatter("en-US");
        let german = formatter("de-DE");
        assert_ne!(english.file_size(1536), german.file_size(1536));
    }

    #[test]
    fn list_conjunction_differs_by_region() {
        let english = formatter("en-US");
        let spanish = formatter("es-ES");
        let items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_ne!(english.list(&items), spanish.list(&items));
    }

    #[test]
    fn duration_uses_padded_clock_components() {
        let english = formatter("en-US");
        assert_eq!(english.duration(3723), "1:02:03");
    }

    #[test]
    fn out_of_range_date_returns_empty_without_panic() {
        let english = formatter("en-US");
        assert!(english.date(2025, 13, 40).is_empty());
    }
}
