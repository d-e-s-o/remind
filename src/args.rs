// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;
use std::time::SystemTime;

use anyhow::Result;
use anyhow::ensure;

use chrono::DateTime;
use chrono::Local;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::NaiveTime;
use chrono::TimeZone as _;
use chrono::offset::LocalResult;

use clap::Args as Arguments;
use clap::Parser;
use clap::Subcommand;


/// Parse a duration from a string.
fn parse_duration(s: &str) -> Result<Duration> {
  let mut remaining = s;
  let mut total = 0u64;

  ensure!(!s.is_empty(), "duration cannot be empty");

  for (suffix, multiplier) in [("h", 3600), ("m", 60), ("s", 1)] {
    if let Some(pos) = remaining.find(suffix) {
      let (count, rest) = remaining.split_at(pos);

      ensure!(!count.is_empty(), "invalid duration provided: {s}");

      total += count.parse::<u64>()? * multiplier;
      remaining = &rest[suffix.len()..];
    }
  }

  ensure!(remaining.is_empty(), "invalid duration provided: {s}");

  let duration = Duration::from_secs(total);
  Ok(duration)
}


fn parse_time(s: &str) -> Result<SystemTime, String> {
  parse_time_at(s, Local::now().date_naive())
}

fn parse_time_at(s: &str, today: NaiveDate) -> Result<SystemTime, String> {
  // RFC3339
  if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
    return Ok(dt.into())
  }

  // YYYY-MM-DD HH:MM
  if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
    return local_datetime(dt)
  }

  // YYYY-MM-DD
  if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
    let dt = date.and_hms_opt(0, 0, 0).unwrap();
    return local_datetime(dt)
  }

  // HH:MM
  if let Ok(time) = NaiveTime::parse_from_str(s, "%H:%M") {
    let dt = today.and_time(time);
    return local_datetime(dt)
  }

  Err(format!("invalid time: `{s}`"))
}

fn local_datetime(dt: NaiveDateTime) -> Result<SystemTime, String> {
  match Local.from_local_datetime(&dt) {
    LocalResult::Single(dt) => Ok(dt.into()),
    LocalResult::Ambiguous(_, _) => Err("ambiguous local time".into()),
    LocalResult::None => Err("invalid local time".into()),
  }
}


/// A program for capture and transcription of audio.
#[derive(Debug, Parser)]
#[clap(version = env!("VERSION"))]
pub struct Args {
  #[command(subcommand)]
  pub command: Command,
  /// Stay in the foreground instead of daemonizing.
  ///
  /// Note that this only flag affects the very first process created.
  /// Subsequent ones just send a message to said process, which is
  /// never a long running blocking operation.
  #[clap(short = 'f', long = "foreground", global = true)]
  pub foreground: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
  /// Set a reminder at a given time.
  At(RemindAt),
  /// Set a reminder after a given amount of time has elapsed.
  In(RemindIn),
  /// List scheduled reminders.
  #[clap(name = "ls", alias = "list")]
  List,
}


/// A type representing the `in` command.
#[derive(Debug, Arguments)]
pub(crate) struct RemindAt {
  /// The duration after which to send the reminder message.
  #[clap(value_parser = parse_time)]
  pub time: SystemTime,
  /// The reminder message.
  pub message: String,
}


/// A type representing the `in` command.
#[derive(Debug, Arguments)]
pub(crate) struct RemindIn {
  /// The duration after which to send the reminder message.
  #[clap(value_parser = parse_duration)]
  pub duration: Duration,
  /// The reminder message.
  pub message: String,
}


#[cfg(test)]
mod tests {
  use super::*;

  use chrono::Datelike as _;
  use chrono::Timelike as _;
  use chrono::Utc;


  /// Make sure that we can parse durations properly.
  #[test]
  fn duration_parsing() {
    assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
    assert_eq!(parse_duration("35s").unwrap(), Duration::from_secs(35));
    assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
    assert_eq!(
      parse_duration("5h").unwrap(),
      Duration::from_secs(5 * 60 * 60)
    );

    assert_eq!(
      parse_duration("1h2m3s").unwrap(),
      Duration::from_secs(3600 + 120 + 3)
    );
    assert_eq!(
      parse_duration("1h30m").unwrap(),
      Duration::from_secs(3600 + 1800)
    );
    assert_eq!(
      parse_duration("1h30s").unwrap(),
      Duration::from_secs(3600 + 30)
    );
    assert_eq!(parse_duration("5m10s").unwrap(), Duration::from_secs(310));

    // Wrong ordering.
    assert!(parse_duration("30m1h").is_err());
    assert!(parse_duration("1s2m").is_err());
    assert!(parse_duration("1h2h").is_err());

    assert!(
      parse_duration("xxx")
        .unwrap_err()
        .to_string()
        .contains("invalid duration provided")
    );
    assert!(parse_duration("1x").is_err());
    assert!(parse_duration("").is_err());
  }

  #[test]
  fn time_parsing() {
    // HH:MM
    let today = NaiveDate::from_ymd_opt(2026, 6, 25).unwrap();
    let ts = parse_time_at("09:30", today).unwrap();
    let dt = DateTime::<Local>::from(ts);

    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 6);
    assert_eq!(dt.day(), 25);
    assert_eq!(dt.hour(), 9);
    assert_eq!(dt.minute(), 30);

    // YYYY-MM-DD
    let ts = parse_time_at("2026-06-25", NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()).unwrap();
    let dt = DateTime::<Local>::from(ts);

    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 6);
    assert_eq!(dt.day(), 25);
    assert_eq!(dt.hour(), 0);
    assert_eq!(dt.minute(), 0);

    // YYYY-MM-DD HH:MM
    let ts = parse_time_at(
      "2026-06-25 13:45",
      NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
    )
    .unwrap();

    let dt = DateTime::<Local>::from(ts);

    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 6);
    assert_eq!(dt.day(), 25);
    assert_eq!(dt.hour(), 13);
    assert_eq!(dt.minute(), 45);

    // RFC3339
    let ts = parse_time_at(
      "2026-06-25T13:45:00Z",
      NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
    )
    .unwrap();

    let dt = DateTime::<Utc>::from(ts);

    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 6);
    assert_eq!(dt.day(), 25);
    assert_eq!(dt.hour(), 13);
    assert_eq!(dt.minute(), 45);

    // Invalid time.
    assert!(
      parse_time_at(
        "definitely not a timestamp",
        NaiveDate::from_ymd_opt(2026, 6, 25).unwrap(),
      )
      .is_err()
    );
  }
}
