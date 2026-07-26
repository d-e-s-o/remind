// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use anyhow::Result;
use anyhow::ensure;

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
  /// Set a reminder after a given amount of time has elapsed.
  In(RemindIn),
  /// List scheduled reminders.
  #[clap(name = "ls", alias = "list")]
  List,
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
}
