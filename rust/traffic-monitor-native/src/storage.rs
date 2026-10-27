//! Transactional legacy history persistence; unknown/corrupt data is never discarded.
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use traffic_monitor_history::{Date, HistoryDocument, HistoryRecord, MergeMode};

#[derive(Clone)]
pub struct HistoryStore {
    path: PathBuf,
    document: HistoryDocument,
    today: Date,
    upload_bytes: u128,
    download_bytes: u128,
    mixed: bool,
    pub notices: Vec<String>,
}
impl HistoryStore {
    pub fn open(path: impl AsRef<Path>, today: Date) -> io::Result<Self> {
        let path = path.as_ref().to_owned();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let backup = path.with_extension("dat.bak");
        let primary = match fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let backup_doc = match fs::read(&backup) {
            Ok(b) => HistoryDocument::parse_bytes(&b).ok(),
            Err(_) => None,
        };
        let mut notices = Vec::new();
        let mut document = match primary.as_ref().map(|b| HistoryDocument::parse_bytes(b)) {
            Some(Ok(d)) => d,
            Some(Err(error)) => {
                let d = backup_doc
                    .clone()
                    .ok_or_else(|| invalid(format!("历史文件损坏，原文件已保留：{error}")))?;
                let preserved = archive(&path, "rejected", primary.as_ref().unwrap())?;
                notices.push(format!(
                    "已从备份恢复历史；损坏原文件保存在 {}",
                    preserved.display()
                ));
                d
            }
            None => backup_doc
                .clone()
                .unwrap_or_else(|| HistoryDocument::new(Vec::new())),
        };
        if let Some(backup) = backup_doc {
            let report = document
                .merge(&backup, today, MergeMode::RestoreBackup)
                .map_err(invalid)?;
            preserve_future(&path, &report.discarded_future, &mut notices)?;
        }
        let report = document.normalize(today).map_err(invalid)?;
        preserve_future(&path, &report.discarded_future, &mut notices)?;
        let mut store = Self {
            path,
            document,
            today,
            upload_bytes: 0,
            download_bytes: 0,
            mixed: false,
            notices,
        };
        store.reload_today();
        Ok(store)
    }
    pub fn document(&self) -> &HistoryDocument {
        &self.document
    }
    pub fn total_today_bytes(&self) -> u128 {
        self.upload_bytes + self.download_bytes
    }
    /// Mixed legacy records contain no trustworthy direction breakdown.
    pub fn today_directions(&self) -> Option<(u128, u128)> {
        (!self.mixed).then_some((self.upload_bytes, self.download_bytes))
    }
    pub fn add(&mut self, today: Date, received: u64, sent: u64) -> io::Result<()> {
        if today != self.today {
            self.flush()?;
            let mut next = self.document.clone();
            let report = next.rollover(today).map_err(invalid)?;
            preserve_future(&self.path, &report.discarded_future, &mut self.notices)?;
            self.document = next;
            self.today = today;
            self.reload_today();
        }
        let up = self
            .upload_bytes
            .checked_add(u128::from(sent))
            .ok_or_else(|| invalid("上传累计溢出"))?;
        let down = self
            .download_bytes
            .checked_add(u128::from(received))
            .ok_or_else(|| invalid("下载累计溢出"))?;
        let record = if self.mixed {
            HistoryRecord::mixed(today, u64::try_from((up + down) / 1024).map_err(invalid)?)
        } else {
            HistoryRecord::split(
                today,
                u64::try_from(up / 1024).map_err(invalid)?,
                u64::try_from(down / 1024).map_err(invalid)?,
            )
            .map_err(invalid)?
        };
        let mut records = self
            .document
            .records()
            .iter()
            .filter(|r| r.date() != today)
            .cloned()
            .collect::<Vec<_>>();
        records.insert(0, record);
        self.document = HistoryDocument::new(records);
        self.upload_bytes = up;
        self.download_bytes = down;
        Ok(())
    }
    pub fn flush(&mut self) -> io::Result<()> {
        let document = self.document.clone();
        self.persist(&document)
    }
    pub fn import(&mut self, path: &Path, mode: MergeMode) -> io::Result<()> {
        let incoming = HistoryDocument::parse_bytes(&fs::read(path)?).map_err(invalid)?;
        let (up_remainder, down_remainder) = if self.mixed {
            (0, (self.upload_bytes + self.download_bytes) % 1024)
        } else {
            (self.upload_bytes % 1024, self.download_bytes % 1024)
        };
        let mut candidate = self.clone();
        let report = candidate
            .document
            .merge(&incoming, self.today, mode)
            .map_err(invalid)?;
        preserve_future(
            &candidate.path,
            &report.discarded_future,
            &mut candidate.notices,
        )?;
        candidate.reload_today();
        // Carry byte-level measurements through the KiB document merge exactly once.
        candidate.add(candidate.today, down_remainder as u64, up_remainder as u64)?;
        candidate.flush()?;
        *self = candidate;
        Ok(())
    }
    pub fn export(&self, path: &Path) -> io::Result<()> {
        let serialized = self
            .document
            .to_legacy_compatible_string()
            .map_err(invalid)?;
        traffic_monitor_config::save_bytes_atomic(path, serialized.as_bytes())
    }
    fn persist(&mut self, document: &HistoryDocument) -> io::Result<()> {
        let backup = self.path.with_extension("dat.bak");
        let bytes = document.to_legacy_string();
        traffic_monitor_config::save_bytes_atomic(&self.path, bytes.as_bytes())?;
        // The primary is committed. Backup failure must not pretend the import failed
        // and leave memory behind its newly committed primary; expose that partial
        // redundancy as a notice while retaining the previous valid backup.
        if let Err(e) = traffic_monitor_config::save_bytes_atomic(&backup, bytes.as_bytes()) {
            self.notices
                .push(format!("History saved; backup update failed: {e}"));
        }
        Ok(())
    }
    fn reload_today(&mut self) {
        if let Some(record) = self
            .document
            .records()
            .iter()
            .find(|r| r.date() == self.today)
        {
            self.upload_bytes = u128::from(record.upload_kib()) * 1024;
            self.download_bytes = u128::from(record.download_kib()) * 1024;
            self.mixed = record.is_mixed();
        } else {
            self.upload_bytes = 0;
            self.download_bytes = 0;
            self.mixed = false;
        }
    }
}
fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
fn archive(path: &Path, reason: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let destination = path.with_file_name(format!(
        "{}.{}-{stamp}-{}-{}.dat",
        path.file_name().unwrap_or_default().to_string_lossy(),
        reason,
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(destination)
}
fn preserve_future(
    path: &Path,
    records: &[HistoryRecord],
    notices: &mut Vec<String>,
) -> io::Result<()> {
    if !records.is_empty() {
        let document = HistoryDocument::new(records.to_vec());
        let destination = archive(path, "future", document.to_legacy_string().as_bytes())?;
        notices.push(format!(
            "系统日期之前的未来记录已单独保存：{}",
            destination.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let base =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/history-io-tests");
            fs::create_dir_all(&base).unwrap();
            let p = base.join(format!(
                "{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn file(&self) -> PathBuf {
            self.0.join("history_traffic.dat")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/history-io-tests")
                .canonicalize()
                .unwrap();
            if let Ok(p) = self.0.canonicalize() {
                assert!(p.starts_with(base) && p != Path::new(env!("CARGO_MANIFEST_DIR")));
                let _ = fs::remove_dir_all(p);
            }
        }
    }
    fn day(n: u8) -> Date {
        Date::new(2026, 10, n).unwrap()
    }
    #[test]
    fn saves_byte_remainders_and_legacy_units() {
        let t = Temp::new();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        s.add(day(27), 256, 512).unwrap();
        s.add(day(27), 768, 512).unwrap();
        s.flush().unwrap();
        let d = HistoryDocument::parse_bytes(&fs::read(t.file()).unwrap()).unwrap();
        assert_eq!(d.records()[0].upload_kib(), 1);
        assert_eq!(d.records()[0].download_kib(), 1);
        assert_eq!(s.total_today_bytes(), 2048);
    }
    #[test]
    fn corrupt_primary_is_preserved_without_a_backup() {
        let t = Temp::new();
        fs::write(t.file(), b"unknown user data").unwrap();
        assert!(HistoryStore::open(t.file(), day(27)).is_err());
        assert_eq!(fs::read(t.file()).unwrap(), b"unknown user data");
    }
    #[test]
    fn valid_backup_recovers_without_destroying_bad_primary() {
        let t = Temp::new();
        fs::write(t.file(), b"damaged history").unwrap();
        fs::write(
            t.file().with_extension("dat.bak"),
            b"lines: \"1\"\n2026/10/27 2/3\n",
        )
        .unwrap();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        assert_eq!(s.total_today_bytes(), 5120);
        assert!(!s.notices.is_empty());
        s.flush().unwrap();
        let preserved = fs::read_dir(&t.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|e| fs::read(e.path()).ok())
            .any(|b| b == b"damaged history");
        assert!(preserved);
    }
    #[test]
    fn rollover_preserves_previous_day_and_does_not_double_count() {
        let t = Temp::new();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        s.add(day(27), 2048, 1024).unwrap();
        s.add(day(28), 1024, 0).unwrap();
        s.add(day(28), 0, 0).unwrap();
        s.flush().unwrap();
        assert_eq!(s.document().records().len(), 2);
        assert_eq!(s.document().records()[0].date(), day(28));
        assert_eq!(s.document().records()[1].total_kib(), 3);
    }
    #[test]
    fn mixed_baseline_does_not_invent_a_download_breakdown() {
        let t = Temp::new();
        fs::write(t.file(), b"lines: \"1\"\n2026/10/27 10\n").unwrap();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        s.add(day(27), 1024, 2048).unwrap();
        s.flush().unwrap();
        let r = &s.document().records()[0];
        assert!(r.is_mixed());
        assert_eq!(r.total_kib(), 13);
    }
    #[test]
    fn future_records_are_archived_before_normalization_is_persisted() {
        let t = Temp::new();
        fs::write(t.file(), b"lines: \"2\"\n2026/10/28 2/3\n2026/10/26 1/1\n").unwrap();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        assert!(!s.notices.is_empty());
        s.flush().unwrap();
        let archived = fs::read_dir(&t.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("future"))
            .any(|e| {
                HistoryDocument::parse_bytes(&fs::read(e.path()).unwrap())
                    .unwrap()
                    .records()
                    .iter()
                    .any(|r| r.date() == day(28))
            });
        assert!(archived);
    }
    #[test]
    fn backup_tracks_the_latest_valid_committed_history() {
        let t = Temp::new();
        let mut store = HistoryStore::open(t.file(), day(27)).unwrap();
        store.add(day(27), 1024, 2048).unwrap();
        store.flush().unwrap();
        store.add(day(27), 2048, 4096).unwrap();
        store.flush().unwrap();
        let total = store.total_today_bytes();
        fs::write(t.file(), b"corrupt primary").unwrap();
        let recovered = HistoryStore::open(t.file(), day(27)).unwrap();
        assert_eq!(recovered.total_today_bytes(), total);
        assert!(!recovered.notices.is_empty());
    }
    #[test]
    fn importing_another_day_preserves_unflushed_byte_remainders() {
        let t = Temp::new();
        let incoming = t.0.join("import.dat");
        fs::write(&incoming, b"lines: \"1\"\n2026/10/26 1/1\n").unwrap();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        s.add(day(27), 512, 512).unwrap();
        s.import(&incoming, MergeMode::Accumulate).unwrap();
        assert_eq!(s.total_today_bytes(), 1024);
        s.add(day(27), 512, 512).unwrap();
        assert_eq!(
            s.document()
                .records()
                .iter()
                .find(|r| r.date() == day(27))
                .unwrap()
                .total_kib(),
            2
        );
    }
    #[test]
    fn importing_mixed_today_carries_split_remainders_once() {
        let t = Temp::new();
        let incoming = t.0.join("import.dat");
        fs::write(&incoming, b"lines: \"1\"\n2026/10/27 10\n").unwrap();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        s.add(day(27), 512, 512).unwrap();
        s.import(&incoming, MergeMode::Accumulate).unwrap();
        assert_eq!(s.total_today_bytes(), 11 * 1024);
        assert!(s.document().records()[0].is_mixed());
        assert_eq!(s.document().records()[0].total_kib(), 11);
        assert_eq!(
            HistoryDocument::parse_bytes(&fs::read(t.file()).unwrap())
                .unwrap()
                .records()[0]
                .total_kib(),
            11
        );
        let yesterday = t.0.join("yesterday.dat");
        fs::write(&yesterday, b"lines: \"1\"\n2026/10/26 1/1\n").unwrap();
        s.add(day(27), 500, 600).unwrap();
        let before = s.total_today_bytes();
        s.import(&yesterday, MergeMode::Accumulate).unwrap();
        assert_eq!(s.total_today_bytes(), before);
    }
    #[test]
    fn import_failure_does_not_mutate_memory_or_disk() {
        let t = Temp::new();
        let mut s = HistoryStore::open(t.file(), day(27)).unwrap();
        s.add(day(27), 1024, 2048).unwrap();
        s.flush().unwrap();
        let before = fs::read(t.file()).unwrap();
        let original = s.document().clone();
        let bad = t.0.join("bad.dat");
        fs::write(&bad, b"not history").unwrap();
        assert!(s.import(&bad, MergeMode::Accumulate).is_err());
        assert_eq!(s.document(), &original);
        assert_eq!(fs::read(t.file()).unwrap(), before);
        let exported = t.0.join("export.dat");
        s.export(&exported).unwrap();
        assert_eq!(
            HistoryDocument::parse_bytes(&fs::read(exported).unwrap()).unwrap(),
            original
        );
    }
}
