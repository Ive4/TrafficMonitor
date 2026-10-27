//! Calendar-aware history aggregation for the native desktop.
//! Calendar-correct, read-only aggregation with unknown directions kept separate.
use std::collections::BTreeMap;
use traffic_monitor_config::Language;
use traffic_monitor_desktop_ui::{HistoryGrouping, HistoryRow};
use traffic_monitor_history::{Date, HistoryDocument};

pub fn parse_date(input: &str) -> Result<Date, String> {
    let value = input.trim();
    let separator = if value.contains('/') { '/' } else { '-' };
    let parts: Vec<_> = value.split(separator).collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || !parts.iter().all(|p| p.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("日期应为 YYYY/MM/DD 或 YYYY-MM-DD".into());
    }
    let year = parts[0].parse::<u16>().map_err(|_| "无效年份")?;
    let month = parts[1].parse::<u8>().map_err(|_| "无效月份")?;
    let day = parts[2].parse::<u8>().map_err(|_| "无效日期")?;
    Date::new(year, month, day).map_err(|_| "日期超出 1900–3000 年范围或不是真实公历日期".into())
}
pub fn format_date(date: Date) -> String {
    date.to_string()
}
#[derive(Default, Clone, Copy)]
struct Totals {
    upload: u128,
    download: u128,
    mixed: u128,
    count: usize,
    known_count: usize,
}
impl Totals {
    fn add(
        &mut self,
        upload: u128,
        download: u128,
        mixed: u128,
        known: bool,
    ) -> Result<(), String> {
        self.upload = self.upload.checked_add(upload).ok_or("上传总量溢出")?;
        self.download = self.download.checked_add(download).ok_or("下载总量溢出")?;
        self.mixed = self.mixed.checked_add(mixed).ok_or("未知方向总量溢出")?;
        self.count += 1;
        self.known_count += usize::from(known);
        Ok(())
    }
    fn total(self) -> Result<u128, String> {
        self.upload
            .checked_add(self.download)
            .and_then(|v| v.checked_add(self.mixed))
            .ok_or_else(|| "流量总量溢出".into())
    }
}
fn bytes(kib: u128) -> Result<u128, String> {
    kib.checked_mul(1024)
        .ok_or_else(|| "流量字节总量溢出".into())
}
/// Exact integer arithmetic keeps large totals precise; no f64 conversion or truncation.
pub fn format_bytes(value: u128) -> String {
    let units = ["KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let mut scale = 1024u128;
    let mut index = 0;
    while index + 1 < units.len() && value >= scale * 1024 {
        scale *= 1024;
        index += 1;
    }
    let mut whole = value / scale;
    let mut fraction = ((value % scale) * 100 + scale / 2) / scale;
    if fraction == 100 {
        whole += 1;
        fraction = 0;
    }
    format!("{whole}.{fraction:02} {}", units[index])
}
pub fn rows(
    document: &HistoryDocument,
    from: &str,
    to: &str,
    grouping: HistoryGrouping,
    week_start_sunday: bool,
) -> Result<(Vec<HistoryRow>, String), String> {
    aggregate(document, from, to, grouping, week_start_sunday, None)
}
pub fn rows_localized(
    document: &HistoryDocument,
    from: &str,
    to: &str,
    grouping: HistoryGrouping,
    week_start_sunday: bool,
    language: &Language,
) -> Result<(Vec<HistoryRow>, String), String> {
    aggregate(
        document,
        from,
        to,
        grouping,
        week_start_sunday,
        Some(language),
    )
}
fn caption(language: Option<&Language>, key: &str, english: &str, chinese: &str) -> String {
    language
        .map(|l| l.text("text", key, english))
        .unwrap_or_else(|| chinese.into())
}
fn aggregate(
    document: &HistoryDocument,
    from: &str,
    to: &str,
    grouping: HistoryGrouping,
    week_start_sunday: bool,
    language: Option<&Language>,
) -> Result<(Vec<HistoryRow>, String), String> {
    let requested_from = if from.trim().is_empty() {
        None
    } else {
        Some(parse_date(from)?)
    };
    let requested_to = if to.trim().is_empty() {
        None
    } else {
        Some(parse_date(to)?)
    };
    if requested_from
        .zip(requested_to)
        .is_some_and(|(start, end)| start > end)
    {
        return Err("开始日期不能晚于结束日期".into());
    }
    let first = document.records().iter().map(|r| r.date()).min();
    let last = document.records().iter().map(|r| r.date()).max();
    let Some(start) = requested_from.or(first) else {
        return Ok((
            Vec::new(),
            caption(
                language,
                "RUST_NO_HISTORY_DATA",
                "No history data",
                "无历史数据",
            ),
        ));
    };
    let Some(end) = requested_to.or(last) else {
        return Ok((
            Vec::new(),
            caption(
                language,
                "RUST_NO_HISTORY_DATA",
                "No history data",
                "无历史数据",
            ),
        ));
    };
    if start > end {
        return Err("开始日期不能晚于结束日期".into());
    }
    let mut groups: BTreeMap<i64, Totals> = BTreeMap::new();
    let mut total = Totals::default();
    for record in document.records() {
        let date = record.date();
        if date < start || date > end {
            continue;
        }
        let (upload, download, mixed) = if record.is_mixed() {
            (0, 0, u128::from(record.total_kib()))
        } else {
            (
                u128::from(record.upload_kib()),
                u128::from(record.download_kib()),
                0,
            )
        };
        let (upload, download, mixed) = (bytes(upload)?, bytes(download)?, bytes(mixed)?);
        let day = ordinal(i32::from(date.year()), date.month(), date.day());
        let key = match grouping {
            HistoryGrouping::Day => day,
            HistoryGrouping::Week => {
                day - if week_start_sunday {
                    (day + 1).rem_euclid(7)
                } else {
                    day.rem_euclid(7)
                }
            }
            HistoryGrouping::Month => ordinal(i32::from(date.year()), date.month(), 1),
            HistoryGrouping::Quarter => {
                ordinal(i32::from(date.year()), ((date.month() - 1) / 3) * 3 + 1, 1)
            }
            HistoryGrouping::Year => ordinal(i32::from(date.year()), 1, 1),
        };
        groups
            .entry(key)
            .or_default()
            .add(upload, download, mixed, !record.is_mixed())?;
        total.add(upload, download, mixed, !record.is_mixed())?;
    }
    let mut rows = Vec::new();
    for (day, totals) in groups.into_iter().rev() {
        let (year, month, date) = civil(day);
        let label = match grouping {
            HistoryGrouping::Day => format!("{year:04}/{month:02}/{date:02}"),
            HistoryGrouping::Week => {
                let (y, m, d) = civil(day + 6);
                format!("{year:04}/{month:02}/{date:02}–{y:04}/{m:02}/{d:02}")
            }
            HistoryGrouping::Month => format!("{year:04}/{month:02}"),
            HistoryGrouping::Quarter => format!("{year:04}/Q{}", (month - 1) / 3 + 1),
            HistoryGrouping::Year => format!("{year:04}"),
        };
        rows.push(HistoryRow {
            date: label,
            upload: if totals.known_count == 0 {
                "--".into()
            } else {
                format_bytes(totals.upload)
            },
            download: if totals.known_count == 0 {
                "--".into()
            } else {
                format_bytes(totals.download)
            },
            mixed: format_bytes(totals.mixed),
            total: format_bytes(totals.total()?),
            total_bytes: totals.total()?,
        });
    }
    let summary = format!(
        "{} {}–{} | {} {} | {} {} | {} {} | {} {} | {} {}",
        caption(language, "IDS_DATE", "Date", "范围"),
        format_date(start),
        format_date(end),
        caption(language, "IDS_TRAFFIC_USED", "Total traffic", "总量"),
        format_bytes(total.total()?),
        caption(language, "IDS_UPLOAD", "Upload", "上传"),
        format_bytes(total.upload),
        caption(language, "IDS_DOWNLOAD", "Download", "下载"),
        format_bytes(total.download),
        caption(
            language,
            "RUST_UNKNOWN_DIRECTION",
            "Unknown direction",
            "未知方向"
        ),
        format_bytes(total.mixed),
        total.count,
        caption(language, "RUST_RECORDS", "records", "条记录")
    );
    Ok((rows, summary))
}
fn leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
fn month_days(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap(year) {
                29
            } else {
                28
            }
        }
    }
}
// Day zero is Monday, 0001/01/01. Gregorian arithmetic is independent of timezone/DST.
fn ordinal(year: i32, month: u8, day: u8) -> i64 {
    let year_before = i64::from(year - 1);
    let mut days = 365 * year_before + year_before / 4 - year_before / 100
        + year_before / 400
        + i64::from(day)
        - 1;
    for m in 1..month {
        days += i64::from(month_days(year, m));
    }
    days
}
fn civil(day: i64) -> (i32, u8, u8) {
    let mut low = 1;
    let mut high = 4001;
    while low + 1 < high {
        let middle = (low + high) / 2;
        if ordinal(middle, 1, 1) <= day {
            low = middle;
        } else {
            high = middle;
        }
    }
    let mut remaining = day - ordinal(low, 1, 1);
    let mut month = 1;
    while remaining >= i64::from(month_days(low, month)) {
        remaining -= i64::from(month_days(low, month));
        month += 1;
    }
    (low, month, (remaining + 1) as u8)
}
