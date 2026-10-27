use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Sample { count: u32, interval_ms: u64 },
    TaskbarProbe,
    HistoryCheck(PathBuf),
}

pub fn parse_command(args: &[OsString]) -> Result<Command, String> {
    let Some(command) = args.first() else {
        return Ok(Command::Help);
    };
    match command.to_str() {
        Some("--help" | "-h") if args.len() == 1 => Ok(Command::Help),
        Some("taskbar-probe") if args.len() == 1 => Ok(Command::TaskbarProbe),
        Some("history-check") if args.len() == 2 => {
            Ok(Command::HistoryCheck(PathBuf::from(&args[1])))
        }
        Some("sample") if args.len() <= 3 => {
            let count = match args.get(1) {
                Some(value) => value.to_str().and_then(|value| value.parse::<u32>().ok()),
                None => Some(3),
            }
            .filter(|value| (1..=1000).contains(value))
            .ok_or("Sample count must be between 1 and 1000.")?;
            let interval_ms = match args.get(2) {
                Some(value) => value.to_str().and_then(|value| value.parse::<u64>().ok()),
                None => Some(1000),
            }
            .filter(|value| (1..=60_000).contains(value))
            .ok_or("Sample interval must be between 1 and 60000 milliseconds.")?;
            Ok(Command::Sample { count, interval_ms })
        }
        _ => Err("Invalid arguments. Use --help for usage.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_command(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn sample_has_bounded_defaults() {
        assert_eq!(
            parse(&["sample"]),
            Ok(Command::Sample {
                count: 3,
                interval_ms: 1000
            })
        );
    }

    #[test]
    fn sample_accepts_an_explicit_count_and_interval() {
        assert_eq!(
            parse(&["sample", "2", "250"]),
            Ok(Command::Sample {
                count: 2,
                interval_ms: 250
            })
        );
    }

    #[test]
    fn invalid_ranges_do_not_start_a_sampling_loop() {
        for args in [
            vec!["sample", "0"],
            vec!["sample", "1001"],
            vec!["sample", "2", "0"],
            vec!["sample", "2", "60001"],
            vec!["sample", "wrong"],
            vec!["sample", "1", "100", "extra"],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }

    #[test]
    fn history_path_preserves_unicode_and_spaces() {
        assert_eq!(
            parse(&["history-check", "C:\\流量记录\\history data.dat"]),
            Ok(Command::HistoryCheck(PathBuf::from(
                "C:\\流量记录\\history data.dat"
            )))
        );
    }

    #[test]
    fn probe_and_help_reject_extra_arguments() {
        assert_eq!(parse(&["taskbar-probe"]), Ok(Command::TaskbarProbe));
        assert_eq!(parse(&[]), Ok(Command::Help));
        assert_eq!(parse(&["--help"]), Ok(Command::Help));
        assert!(parse(&["taskbar-probe", "extra"]).is_err());
        assert!(parse(&["history-check"]).is_err());
        assert!(parse(&["unknown"]).is_err());
    }
}
