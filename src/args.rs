// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;
use std::time::SystemTime;

use anyhow::Result;
use anyhow::anyhow;

use clap::Args as Arguments;
use clap::Parser;
use clap::Subcommand;


/// Parse a duration from a string.
fn parse_duration(s: &str) -> Result<Duration> {
  let durations = [("s", 1), ("m", 60), ("h", 3600)];

  for (suffix, multiplier) in &durations {
    if let Some(base) = s.strip_suffix(suffix)
      && let Ok(count) = base.parse::<u64>()
    {
      return Ok(Duration::from_secs(count * multiplier))
    }
  }

  Err(anyhow!("invalid duration provided: {s}"))
}


/// A program for capture and transcription of audio.
#[derive(Debug, Parser)]
pub struct Args {
  #[command(subcommand)]
  pub command: Command,
  /// The reminder message.
  pub message: String,
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
}

/// A type representing the `in` command.
#[derive(Debug, Arguments)]
pub(crate) struct RemindIn {
  #[clap(value_parser = parse_duration)]
  duration: Duration,
}

impl RemindIn {
  pub fn target_time(&self) -> SystemTime {
    SystemTime::now() + self.duration
  }
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
    assert!(
      parse_duration("xxx")
        .unwrap_err()
        .to_string()
        .contains("invalid duration provided")
    );
  }
}
