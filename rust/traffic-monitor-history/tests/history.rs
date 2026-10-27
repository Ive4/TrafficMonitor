use traffic_monitor_history::{
    Date, DiagnosticKind, HistoryDocument, HistoryError, HistoryRecord, MergeMode,
};

fn date(day: u8) -> Date {
    Date::new(2026, 10, day).unwrap()
}

#[test]
fn dates_validate_legacy_year_range_and_calendar_days() {
    assert!(Date::new(1900, 1, 1).is_ok());
    assert!(Date::new(3000, 12, 31).is_ok());
    for (year, month, day) in [
        (1899, 12, 31),
        (3001, 1, 1),
        (2026, 0, 1),
        (2026, 13, 1),
        (2026, 1, 0),
        (2026, 4, 31),
        (2026, 2, 29),
        (1900, 2, 29),
        (2100, 2, 29),
    ] {
        assert!(Date::new(year, month, day).is_err(), "{year}/{month}/{day}");
    }
}

#[test]
fn leap_dates_follow_gregorian_century_rule() {
    for year in [1904, 2000, 2024, 2400] {
        assert!(Date::new(year, 2, 29).is_ok());
    }
}

#[test]
fn dates_sort_chronologically_and_format_for_legacy_reader() {
    let first = Date::new(2026, 9, 30).unwrap();
    let next = Date::new(2026, 10, 1).unwrap();
    let year = Date::new(2027, 1, 1).unwrap();
    assert!(first < next && next < year);
    assert_eq!(next.to_string(), "2026/10/01");
    assert_eq!((next.year(), next.month(), next.day()), (2026, 10, 1));
}

#[test]
fn parses_mixed_and_directional_kib_without_32_bit_truncation() {
    let document = HistoryDocument::parse(
        "lines: \"2\"\n2026/10/27 4294967296/8589934592\n2026/10/26 12884901888\n",
    )
    .unwrap();
    assert_eq!(document.records().len(), 2);
    let split = &document.records()[0];
    assert_eq!(split.date(), date(27));
    assert!(!split.is_mixed());
    assert_eq!(
        (split.upload_kib(), split.download_kib(), split.total_kib()),
        (4294967296, 8589934592, 12884901888)
    );
    let mixed = &document.records()[1];
    assert!(mixed.is_mixed());
    assert_eq!(
        (mixed.upload_kib(), mixed.download_kib(), mixed.total_kib()),
        (0, 12884901888, 12884901888)
    );
}

#[test]
fn parses_crlf_blank_lines_bom_and_zero_records() {
    let document = HistoryDocument::parse(
        "\u{feff}lines: \"2\"\r\n\r\n2026/10/27 0/0\r\n \t\r\n2026/10/26 0\r\n",
    )
    .unwrap();
    assert_eq!(document.records().len(), 2);
    assert_eq!(document.records()[0].total_kib(), 0);
    assert!(document.records()[1].is_mixed());
}

#[test]
fn empty_documents_round_trip_with_zero_header() {
    let document = HistoryDocument::parse("lines: \"0\"\n\n").unwrap();
    assert!(document.records().is_empty());
    assert_eq!(document.to_legacy_string(), "lines: \"0\"\r\n");
}

#[test]
fn serialization_preserves_format_order_and_u64_maximum() {
    let document = HistoryDocument::new(vec![
        HistoryRecord::split(date(27), u64::MAX - 1, 1).unwrap(),
        HistoryRecord::mixed(date(26), u64::MAX),
    ]);
    let text = document.to_legacy_string();
    assert_eq!(
        text,
        "lines: \"2\"\r\n2026/10/27 18446744073709551614/1\r\n2026/10/26 18446744073709551615\r\n"
    );
    assert_eq!(HistoryDocument::parse(&text).unwrap(), document);
    assert_eq!(
        HistoryDocument::parse_bytes(text.as_bytes()).unwrap(),
        document
    );
}

#[test]
fn rejects_malformed_header_and_wrong_declared_count() {
    for header in [
        "",
        "lines: 1",
        "lines: \"-1\"",
        "lines: \"1\" garbage",
        "other: \"1\"",
    ] {
        let error = HistoryDocument::parse(&format!("{header}\n2026/10/27 1\n")).unwrap_err();
        assert_eq!(error.diagnostics[0].line, 1);
        assert_eq!(error.diagnostics[0].kind, DiagnosticKind::InvalidHeader);
    }
    let error = HistoryDocument::parse("lines: \"2\"\n2026/10/27 1\n").unwrap_err();
    assert!(error.diagnostics.iter().any(|item| item.kind
        == DiagnosticKind::RecordCountMismatch {
            declared: 2,
            actual: 1
        }));
}

#[test]
fn reports_all_bad_rows_using_physical_line_numbers() {
    let error = HistoryDocument::parse(
        "lines: \"4\"\r\n\r\n2026/02/29 8\r\n2026/10/27 -1/2\r\nnot a row\r\n2026/10/26 1/2/3\r\n",
    )
    .unwrap_err();
    assert_eq!(
        error
            .diagnostics
            .iter()
            .map(|item| item.line)
            .collect::<Vec<_>>(),
        vec![3, 4, 5, 6]
    );
    assert_eq!(error.diagnostics[0].kind, DiagnosticKind::InvalidDate);
}

#[test]
fn rejects_overflowing_or_non_decimal_counters_and_total() {
    for value in [
        "18446744073709551616",
        "1/18446744073709551615",
        "+1",
        "-1",
        "1x",
        "1/",
        "/1",
        "1/2 3",
        "0x10",
    ] {
        let input = format!("lines: \"1\"\n2026/10/27 {value}\n");
        let error = HistoryDocument::parse(&input).unwrap_err();
        assert_eq!(error.diagnostics.len(), 1, "{value}: {error}");
        assert_eq!(error.diagnostics[0].line, 2);
    }
    assert!(HistoryRecord::split(date(27), u64::MAX, 1).is_err());
}

#[test]
fn rejects_date_syntax_that_legacy_offsets_cannot_read() {
    for value in [
        "2026-10-27",
        "2026/1/27",
        "26/10/27",
        "2026/10/027",
        "２０２６/10/27",
    ] {
        assert!(HistoryDocument::parse(&format!("lines: \"1\"\n{value} 1\n")).is_err());
    }
}

#[test]
fn rejects_invalid_utf8_without_replacement_characters() {
    let error = HistoryDocument::parse_bytes(b"lines: \"1\"\n2026/10/27 \xff\n").unwrap_err();
    assert_eq!(error.diagnostics[0].line, 2);
    assert_eq!(error.diagnostics[0].kind, DiagnosticKind::InvalidUtf8);
}

fn split(day: u8, upload: u64, download: u64) -> HistoryRecord {
    HistoryRecord::split(date(day), upload, download).unwrap()
}

#[test]
fn normalizes_descending_dates_duplicates_and_today_slot() {
    let mut document = HistoryDocument::new(vec![
        split(24, 1, 2),
        split(26, 10, 20),
        split(24, 3, 4),
        split(25, 5, 6),
    ]);
    let report = document.normalize(date(27)).unwrap();
    assert!(report.discarded_future.is_empty());
    assert_eq!(
        document.records(),
        &[
            split(27, 0, 0),
            split(26, 10, 20),
            split(25, 5, 6),
            split(24, 4, 6)
        ]
    );
    let normalized = document.clone();
    document.normalize(date(27)).unwrap();
    assert_eq!(document, normalized);
}

#[test]
fn normalizing_never_loses_old_first_record_when_today_occurs_later() {
    let mut document =
        HistoryDocument::new(vec![split(25, 1, 2), split(27, 3, 4), split(26, 5, 6)]);
    document.normalize(date(27)).unwrap();
    assert_eq!(
        document.records(),
        &[split(27, 3, 4), split(26, 5, 6), split(25, 1, 2)]
    );
}

#[test]
fn reports_future_records_including_the_first_slot_before_removing_them() {
    let future = vec![split(29, 1, 2), split(28, 3, 4)];
    let mut document = HistoryDocument::new(future.clone());
    let report = document.normalize(date(27)).unwrap();
    assert_eq!(report.discarded_future, future);
    assert_eq!(document.records(), &[split(27, 0, 0)]);
}

#[test]
fn same_day_mixed_and_split_merge_is_order_independent_and_preserves_total() {
    let mixed = HistoryRecord::mixed(date(26), 100);
    let directional = split(26, 7, 13);
    for records in [
        vec![mixed.clone(), directional.clone()],
        vec![directional, mixed],
    ] {
        let mut document = HistoryDocument::new(records);
        document.normalize(date(27)).unwrap();
        assert_eq!(
            document.records(),
            &[split(27, 0, 0), HistoryRecord::mixed(date(26), 120)]
        );
        assert_eq!(
            HistoryDocument::parse(&document.to_legacy_string()).unwrap(),
            document
        );
    }
}

#[test]
fn mixed_today_keeps_unknown_direction_instead_of_inventing_download() {
    let mut document =
        HistoryDocument::new(vec![HistoryRecord::mixed(date(27), 100), split(27, 7, 13)]);
    document.normalize(date(27)).unwrap();
    assert_eq!(document.records(), &[HistoryRecord::mixed(date(27), 120)]);
}

#[test]
fn normalization_overflow_leaves_document_unchanged() {
    for records in [
        vec![HistoryRecord::mixed(date(26), u64::MAX), split(26, 1, 0)],
        vec![split(26, u64::MAX, 0), split(26, 0, 1)],
    ] {
        let mut document = HistoryDocument::new(records);
        let original = document.clone();
        assert_eq!(
            document.normalize(date(27)),
            Err(HistoryError::CounterOverflow)
        );
        assert_eq!(document, original);
    }
}

#[test]
fn accumulated_merge_combines_both_days_and_leaves_source_unchanged() {
    let mut destination = HistoryDocument::new(vec![split(27, 1, 2), split(26, 3, 4)]);
    let source = HistoryDocument::new(vec![split(27, 5, 6), split(25, 7, 8), split(26, 9, 10)]);
    let original_source = source.clone();
    destination
        .merge(&source, date(27), MergeMode::Accumulate)
        .unwrap();
    assert_eq!(
        destination.records(),
        &[split(27, 6, 8), split(26, 12, 14), split(25, 7, 8)]
    );
    assert_eq!(source, original_source);
}

#[test]
fn backup_merge_uses_today_maxima_and_keeps_existing_past_days() {
    let mut destination = HistoryDocument::new(vec![split(27, 10, 20), split(26, 30, 40)]);
    let source = HistoryDocument::new(vec![split(27, 15, 5), split(26, 300, 400), split(25, 7, 8)]);
    destination
        .merge(&source, date(27), MergeMode::RestoreBackup)
        .unwrap();
    assert_eq!(
        destination.records(),
        &[split(27, 15, 20), split(26, 30, 40), split(25, 7, 8)]
    );
    let restored = destination.clone();
    destination
        .merge(&source, date(27), MergeMode::RestoreBackup)
        .unwrap();
    assert_eq!(destination, restored);
}

#[test]
fn backup_mixed_today_uses_total_maximum_without_guessing_direction() {
    for (left, right) in [
        (HistoryRecord::mixed(date(27), 100), split(27, 40, 80)),
        (split(27, 40, 80), HistoryRecord::mixed(date(27), 100)),
    ] {
        let mut destination = HistoryDocument::new(vec![left]);
        destination
            .merge(
                &HistoryDocument::new(vec![right]),
                date(27),
                MergeMode::RestoreBackup,
            )
            .unwrap();
        assert_eq!(
            destination.records(),
            &[HistoryRecord::mixed(date(27), 120)]
        );
    }
}

#[test]
fn merge_reports_future_records_from_both_documents() {
    let mut destination = HistoryDocument::new(vec![split(28, 1, 2)]);
    let source = HistoryDocument::new(vec![split(29, 3, 4)]);
    let report = destination
        .merge(&source, date(27), MergeMode::Accumulate)
        .unwrap();
    assert_eq!(
        report.discarded_future,
        vec![split(28, 1, 2), split(29, 3, 4)]
    );
}

#[test]
fn merge_overflow_is_atomic_in_accumulate_and_backup_modes() {
    for mode in [MergeMode::Accumulate, MergeMode::RestoreBackup] {
        let mut destination = HistoryDocument::new(vec![split(27, u64::MAX, 0)]);
        let original = destination.clone();
        let source = HistoryDocument::new(vec![split(27, 0, u64::MAX)]);
        assert_eq!(
            destination.merge(&source, date(27), mode),
            Err(HistoryError::CounterOverflow)
        );
        assert_eq!(destination, original);
    }
}

#[test]
fn rollover_injects_new_today_without_duplicating_or_losing_usage() {
    let mut document =
        HistoryDocument::new(vec![split(27, 10, 20), HistoryRecord::mixed(date(26), 30)]);
    document.rollover(date(28)).unwrap();
    assert_eq!(
        document.records(),
        &[
            split(28, 0, 0),
            split(27, 10, 20),
            HistoryRecord::mixed(date(26), 30)
        ]
    );
    let rolled = document.clone();
    document.rollover(date(28)).unwrap();
    assert_eq!(document, rolled);
    assert_eq!(
        HistoryDocument::parse(&document.to_legacy_string()).unwrap(),
        document
    );
}

#[test]
fn rollover_crosses_year_and_leap_day_and_omits_empty_old_days() {
    for (before, after) in [
        (
            Date::new(2024, 2, 29).unwrap(),
            Date::new(2024, 3, 1).unwrap(),
        ),
        (
            Date::new(2026, 12, 31).unwrap(),
            Date::new(2027, 1, 1).unwrap(),
        ),
    ] {
        let previous = HistoryRecord::split(before, 1, 2).unwrap();
        let mut document = HistoryDocument::new(vec![previous.clone()]);
        document.rollover(after).unwrap();
        assert_eq!(
            document.records(),
            &[HistoryRecord::split(after, 0, 0).unwrap(), previous]
        );
    }
    let mut empty = HistoryDocument::new(vec![split(26, 0, 0)]);
    empty.rollover(date(27)).unwrap();
    assert_eq!(empty.records(), &[split(27, 0, 0)]);
}

#[test]
fn clock_rollback_reports_removed_future_usage() {
    let previous = split(27, 10, 20);
    let mut document = HistoryDocument::new(vec![previous.clone()]);
    let report = document.rollover(date(26)).unwrap();
    assert_eq!(report.discarded_future, vec![previous]);
    assert_eq!(document.records(), &[split(26, 0, 0)]);
}

#[test]
fn range_aggregation_is_inclusive_and_keeps_unknown_direction_separate() {
    let document = HistoryDocument::new(vec![
        split(27, u64::MAX - 1, 1),
        HistoryRecord::mixed(date(26), u64::MAX),
        split(25, 7, 8),
    ]);
    let totals = document.aggregate_range(date(26), date(27)).unwrap();
    assert_eq!(totals.upload_kib(), u128::from(u64::MAX - 1));
    assert_eq!(totals.download_kib(), 1);
    assert_eq!(totals.mixed_kib(), u128::from(u64::MAX));
    assert_eq!(totals.total_kib(), 2 * u128::from(u64::MAX));
    assert_eq!(
        document
            .aggregate_range(date(25), date(25))
            .unwrap()
            .total_kib(),
        15
    );
    assert_eq!(
        document
            .aggregate_range(date(1), date(2))
            .unwrap()
            .total_kib(),
        0
    );
    assert_eq!(
        document.aggregate_range(date(27), date(26)),
        Err(HistoryError::InvalidRange)
    );
}

#[test]
fn legacy_reader_serialization_checks_signed_atoll_limit() {
    let document = HistoryDocument::new(vec![
        split(27, i64::MAX as u64, i64::MAX as u64),
        HistoryRecord::mixed(date(26), i64::MAX as u64),
    ]);
    let output = document.to_legacy_compatible_string().unwrap();
    assert_eq!(HistoryDocument::parse(&output).unwrap(), document);
}

#[test]
fn legacy_reader_serialization_rejects_unsigned_values_it_cannot_load() {
    for record in [
        split(27, i64::MAX as u64 + 1, 0),
        HistoryRecord::mixed(date(26), u64::MAX),
    ] {
        let document = HistoryDocument::new(vec![record]);
        assert_eq!(
            document.to_legacy_compatible_string(),
            Err(HistoryError::LegacyCounterOutOfRange)
        );
        assert_eq!(
            HistoryDocument::parse(&document.to_legacy_string()).unwrap(),
            document
        );
    }
}

#[test]
fn checked_in_compatibility_fixture_normalizes_and_round_trips() {
    let mut document = HistoryDocument::parse(include_str!("fixtures/history_legacy.txt")).unwrap();
    document.normalize(date(27)).unwrap();
    assert_eq!(document.records().len(), 3);
    assert_eq!(document.records()[2], HistoryRecord::mixed(date(25), 120));
    let output = document.to_legacy_compatible_string().unwrap();
    assert_eq!(HistoryDocument::parse(&output).unwrap(), document);
}
