//! History traffic in KiB, with checked legacy text parsing and explicit-date operations.
//!
//! This library does not read the system clock or touch files. Parse before normalization;
//! use [`HistoryDocument::to_legacy_compatible_string`] when exporting for the old C++ loader.
//! A mixed record contains an unknown-direction total, never a measured download value.

#![doc = include_str!("../README.md")]

use std::collections::BTreeMap;
use std::fmt::{self, Write};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// A valid Gregorian date in the legacy supported year range, 1900 through 3000.
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Invalid calendar day or a year outside the legacy supported range.
pub struct InvalidDate;

impl Date {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, InvalidDate> {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(InvalidDate),
        };
        if !(1900..=3000).contains(&year) || day == 0 || day > days_in_month {
            return Err(InvalidDate);
        }
        Ok(Self { year, month, day })
    }

    pub fn year(self) -> u16 {
        self.year
    }
    pub fn month(self) -> u8 {
        self.month
    }
    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for InvalidDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a Gregorian date in 1900..=3000")
    }
}

impl std::error::Error for InvalidDate {}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}/{:02}/{:02}", self.year, self.month, self.day)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A day's traffic. All stored counters and their sum fit in `u64` KiB.
pub struct HistoryRecord {
    date: Date,
    upload_kib: u64,
    download_kib: u64,
    mixed: bool,
}

impl HistoryRecord {
    /// Store a total whose upload/download directions are unknown.
    pub fn mixed(date: Date, total_kib: u64) -> Self {
        Self {
            date,
            upload_kib: 0,
            download_kib: total_kib,
            mixed: true,
        }
    }
    /// Store measured upload/download counters, rejecting a total that would overflow.
    pub fn split(date: Date, upload_kib: u64, download_kib: u64) -> Result<Self, HistoryError> {
        upload_kib
            .checked_add(download_kib)
            .ok_or(HistoryError::CounterOverflow)?;
        Ok(Self {
            date,
            upload_kib,
            download_kib,
            mixed: false,
        })
    }
    pub fn date(&self) -> Date {
        self.date
    }
    pub fn upload_kib(&self) -> u64 {
        self.upload_kib
    }
    /// Legacy storage field: for a mixed record this is the unknown-direction total.
    /// Check [`Self::is_mixed`] before treating it as a download measurement.
    pub fn download_kib(&self) -> u64 {
        self.download_kib
    }
    pub fn is_mixed(&self) -> bool {
        self.mixed
    }
    pub fn total_kib(&self) -> u64 {
        self.upload_kib + self.download_kib
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Failed operations leave the original document unchanged.
pub enum HistoryError {
    CounterOverflow,
    InvalidRange,
    LegacyCounterOutOfRange,
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CounterOverflow => "traffic counters or their sum exceed u64::MAX KiB",
            Self::InvalidRange => "range start must not be after range end",
            Self::LegacyCounterOutOfRange => {
                "a counter exceeds the signed 64-bit limit of the C++ atoll loader"
            }
        })
    }
}
impl std::error::Error for HistoryError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    InvalidHeader,
    RecordCountMismatch { declared: u64, actual: u64 },
    InvalidDate,
    InvalidRecord,
    InvalidCounter,
    CounterOverflow,
    InvalidUtf8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A parse diagnostic, located by physical one-based line number.
pub struct Diagnostic {
    pub line: usize,
    pub kind: DiagnosticKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Parsing is strict: an error returns every row diagnostic and no partial document.
pub struct ParseError {
    pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index != 0 {
                f.write_str("; ")?;
            }
            write!(f, "line {}: {}", diagnostic.line, diagnostic.kind)?;
        }
        Ok(())
    }
}
impl std::error::Error for ParseError {}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHeader => f.write_str("expected lines: \"N\" header"),
            Self::RecordCountMismatch { declared, actual } => {
                write!(f, "header declares {declared} records but found {actual}")
            }
            Self::InvalidDate => fmt::Display::fmt(&InvalidDate, f),
            Self::InvalidRecord => {
                f.write_str("expected YYYY/MM/DD total or YYYY/MM/DD upload/download")
            }
            Self::InvalidCounter => f.write_str("expected unsigned decimal KiB counters"),
            Self::CounterOverflow => fmt::Display::fmt(&HistoryError::CounterOverflow, f),
            Self::InvalidUtf8 => f.write_str("input is not valid UTF-8/ASCII"),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Parsed records retain input order until an explicit normalization or merge.
pub struct HistoryDocument {
    records: Vec<HistoryRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeMode {
    /// Add same-day traffic with checked arithmetic.
    Accumulate,
    /// Today uses maxima; an existing past day takes precedence over the backup.
    RestoreBackup,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NormalizationReport {
    /// Records later than the supplied today, returned so their removal is reviewable.
    pub discarded_future: Vec<HistoryRecord>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Range totals separate measured directions from old unknown-direction traffic.
pub struct TrafficTotals {
    upload_kib: u128,
    download_kib: u128,
    mixed_kib: u128,
}

impl TrafficTotals {
    pub fn upload_kib(self) -> u128 {
        self.upload_kib
    }
    pub fn download_kib(self) -> u128 {
        self.download_kib
    }
    pub fn mixed_kib(self) -> u128 {
        self.mixed_kib
    }
    pub fn total_kib(self) -> u128 {
        self.upload_kib + self.download_kib + self.mixed_kib
    }
}

impl HistoryDocument {
    pub fn new(records: Vec<HistoryRecord>) -> Self {
        Self { records }
    }
    pub fn records(&self) -> &[HistoryRecord] {
        &self.records
    }
    /// Export canonical CRLF text, rejecting counters beyond the C++ `atoll` range.
    /// This checks text-loader limits, not later signed byte arithmetic in the old UI.
    pub fn to_legacy_compatible_string(&self) -> Result<String, HistoryError> {
        if self.records.iter().any(|record| {
            record.upload_kib > i64::MAX as u64 || record.download_kib > i64::MAX as u64
        }) {
            return Err(HistoryError::LegacyCounterOutOfRange);
        }
        Ok(self.to_legacy_string())
    }
    /// Sort newest first, combine duplicates, and ensure exactly one today record.
    /// Empty past days are omitted. Future records are returned in the report.
    /// Any mixed input makes that day's combined total mixed (no direction is inferred).
    pub fn normalize(&mut self, today: Date) -> Result<NormalizationReport, HistoryError> {
        let mut by_date: BTreeMap<Date, HistoryRecord> = BTreeMap::new();
        let mut report = NormalizationReport::default();
        for record in &self.records {
            if record.date > today {
                report.discarded_future.push(record.clone());
                continue;
            }
            let merged = match by_date.get(&record.date) {
                Some(existing) => combine_records(existing, record, MergeMode::Accumulate)?,
                None => record.clone(),
            };
            by_date.insert(record.date, merged);
        }
        by_date.retain(|date, record| *date == today || record.total_kib() != 0);
        by_date
            .entry(today)
            .or_insert(HistoryRecord::split(today, 0, 0)?);
        self.records = by_date.into_values().rev().collect();
        Ok(report)
    }
    /// Normalize both documents for the supplied today, then merge without modifying `other`.
    /// Overflow is atomic: the destination also stays unchanged.
    pub fn merge(
        &mut self,
        other: &Self,
        today: Date,
        mode: MergeMode,
    ) -> Result<NormalizationReport, HistoryError> {
        let mut destination = self.clone();
        let mut report = destination.normalize(today)?;
        let mut source = other.clone();
        report
            .discarded_future
            .extend(source.normalize(today)?.discarded_future);
        let mut by_date: BTreeMap<Date, HistoryRecord> = destination
            .records
            .into_iter()
            .map(|record| (record.date, record))
            .collect();
        for record in source.records {
            let merged = match by_date.get(&record.date) {
                Some(existing) if mode == MergeMode::Accumulate || record.date == today => {
                    combine_records(existing, &record, mode)?
                }
                Some(_) => continue,
                None => record,
            };
            by_date.insert(merged.date, merged);
        }
        self.records = by_date.into_values().rev().collect();
        Ok(report)
    }
    /// Advance to an explicit date; repeating the same date is idempotent.
    /// Clock rollback returns removed future usage in the normalization report.
    pub fn rollover(&mut self, today: Date) -> Result<NormalizationReport, HistoryError> {
        self.normalize(today)
    }
    /// Sum all records in an inclusive calendar range, without normalizing or mutating them.
    /// Totals use `u128` so aggregating multiple `u64` days cannot truncate to 32/64 bits.
    pub fn aggregate_range(&self, start: Date, end: Date) -> Result<TrafficTotals, HistoryError> {
        if start > end {
            return Err(HistoryError::InvalidRange);
        }
        let mut totals = TrafficTotals::default();
        for record in self
            .records
            .iter()
            .filter(|record| start <= record.date && record.date <= end)
        {
            if record.mixed {
                totals.mixed_kib = totals
                    .mixed_kib
                    .checked_add(u128::from(record.total_kib()))
                    .ok_or(HistoryError::CounterOverflow)?;
            } else {
                totals.upload_kib = totals
                    .upload_kib
                    .checked_add(u128::from(record.upload_kib))
                    .ok_or(HistoryError::CounterOverflow)?;
                totals.download_kib = totals
                    .download_kib
                    .checked_add(u128::from(record.download_kib))
                    .ok_or(HistoryError::CounterOverflow)?;
            }
        }
        Ok(totals)
    }
    /// Parse a `lines: "N"` header followed by old totals or upload/download rows.
    /// Blank lines do not count toward N; malformed nonblank rows do.
    /// The parser preserves zero records, directions, full `u64` counters and input order.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let mut lines = input.lines();
        let header = lines
            .next()
            .unwrap_or("")
            .trim_start_matches('\u{feff}')
            .trim();
        let declared = header
            .strip_prefix("lines: \"")
            .and_then(|value| value.strip_suffix('"'))
            .and_then(|value| parse_counter(value).ok());
        let mut diagnostics = Vec::new();
        if declared.is_none() {
            diagnostics.push(Diagnostic {
                line: 1,
                kind: DiagnosticKind::InvalidHeader,
            });
        }
        let mut records = Vec::new();
        let mut actual = 0;
        for (index, line) in lines.enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            actual += 1;
            match parse_record(line) {
                Ok(record) => records.push(record),
                Err(kind) => diagnostics.push(Diagnostic {
                    line: index + 2,
                    kind,
                }),
            }
        }
        if let Some(declared) = declared {
            if declared != actual {
                diagnostics.push(Diagnostic {
                    line: 1,
                    kind: DiagnosticKind::RecordCountMismatch { declared, actual },
                });
            }
        }
        if diagnostics.is_empty() {
            Ok(Self { records })
        } else {
            Err(ParseError { diagnostics })
        }
    }
    /// Parse ASCII/UTF-8 bytes without replacing invalid bytes or skipping bad rows.
    pub fn parse_bytes(input: &[u8]) -> Result<Self, ParseError> {
        let input = std::str::from_utf8(input).map_err(|error| ParseError {
            diagnostics: vec![Diagnostic {
                line: input[..error.valid_up_to()]
                    .iter()
                    .filter(|byte| **byte == b'\n')
                    .count()
                    + 1,
                kind: DiagnosticKind::InvalidUtf8,
            }],
        })?;
        Self::parse(input)
    }
    /// Export canonical legacy-shaped CRLF text with full `u64` precision.
    /// For old C++ import, use [`Self::to_legacy_compatible_string`] to check its signed limit.
    pub fn to_legacy_string(&self) -> String {
        let mut output = format!("lines: \"{}\"\r\n", self.records.len());
        for record in &self.records {
            if record.mixed {
                write!(output, "{} {}\r\n", record.date, record.download_kib)
                    .expect("writing to String cannot fail");
            } else {
                write!(
                    output,
                    "{} {}/{}\r\n",
                    record.date, record.upload_kib, record.download_kib
                )
                .expect("writing to String cannot fail");
            }
        }
        output
    }
}

fn combine_records(
    left: &HistoryRecord,
    right: &HistoryRecord,
    mode: MergeMode,
) -> Result<HistoryRecord, HistoryError> {
    let combine = |left: u64, right: u64| match mode {
        MergeMode::Accumulate => left.checked_add(right).ok_or(HistoryError::CounterOverflow),
        MergeMode::RestoreBackup => Ok(left.max(right)),
    };
    if left.mixed || right.mixed {
        Ok(HistoryRecord::mixed(
            left.date,
            combine(left.total_kib(), right.total_kib())?,
        ))
    } else {
        HistoryRecord::split(
            left.date,
            combine(left.upload_kib, right.upload_kib)?,
            combine(left.download_kib, right.download_kib)?,
        )
    }
}

fn parse_counter(input: &str) -> Result<u64, DiagnosticKind> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DiagnosticKind::InvalidCounter);
    }
    input.parse().map_err(|_| DiagnosticKind::CounterOverflow)
}

fn parse_record(line: &str) -> Result<HistoryRecord, DiagnosticKind> {
    let mut parts = line.split_ascii_whitespace();
    let date_text = parts.next().ok_or(DiagnosticKind::InvalidRecord)?;
    let traffic_text = parts.next().ok_or(DiagnosticKind::InvalidRecord)?;
    if parts.next().is_some() {
        return Err(DiagnosticKind::InvalidRecord);
    }
    let bytes = date_text.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'/'
        || bytes[7] != b'/'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err(DiagnosticKind::InvalidDate);
    }
    let date = Date::new(
        date_text[..4]
            .parse()
            .map_err(|_| DiagnosticKind::InvalidDate)?,
        date_text[5..7]
            .parse()
            .map_err(|_| DiagnosticKind::InvalidDate)?,
        date_text[8..10]
            .parse()
            .map_err(|_| DiagnosticKind::InvalidDate)?,
    )
    .map_err(|_| DiagnosticKind::InvalidDate)?;
    match traffic_text.split_once('/') {
        Some((upload, download)) => {
            HistoryRecord::split(date, parse_counter(upload)?, parse_counter(download)?)
                .map_err(|_| DiagnosticKind::CounterOverflow)
        }
        None => Ok(HistoryRecord::mixed(date, parse_counter(traffic_text)?)),
    }
}
