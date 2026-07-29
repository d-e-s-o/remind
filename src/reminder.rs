// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::time::Duration;
use std::time::SystemTime;

use chrono::DateTime;
use chrono::Local;

use serde::Deserialize;
use serde::Serialize;

use crate::args::RemindAt;
use crate::args::RemindIn;


/// The representation of a reminder to schedule.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub(crate) struct Reminder {
  /// The time at which the reminder should show.
  pub time: SystemTime,
  /// The remind message.
  pub message: String,
}

impl Reminder {
  /// Calculate the duration until the reminder is due.
  ///
  /// If the reminder is overdue, `None` will be returned.
  #[inline]
  pub fn duration(&self) -> Option<Duration> {
    self.time.duration_since(SystemTime::now()).ok()
  }
}

impl From<RemindAt> for Reminder {
  fn from(other: RemindAt) -> Self {
    let RemindAt { time, message } = other;

    Self { time, message }
  }
}

impl From<RemindIn> for Reminder {
  fn from(other: RemindIn) -> Self {
    let RemindIn { duration, message } = other;

    Self {
      time: SystemTime::now() + duration,
      message,
    }
  }
}

// TODO: We shouldn't need this conversion, except that because of our
//       recursive connection fallback logic we do.
impl From<Reminder> for RemindIn {
  fn from(other: Reminder) -> Self {
    let Reminder { time, message } = other;
    let now = SystemTime::now();

    RemindIn {
      duration: time
        .duration_since(now)
        .unwrap_or_else(|_err| Duration::from_secs(0)),
      message,
    }
  }
}


pub fn format_reminders(reminders: &mut [Reminder]) -> String {
  format_reminders_at(Local::now(), reminders)
}

#[derive(Debug)]
struct FormattedReminder {
  time: DateTime<Local>,
  relative: String,
  message: String,
}

fn format_reminders_at(now: DateTime<Local>, reminders: &mut [Reminder]) -> String {
  use chrono::Duration;

  let today = now.date_naive();
  let () = reminders.sort_by_key(|r| r.time);

  let grouped = reminders
    .iter()
    .map(|r| DateTime::<Local>::from(r.time).date_naive())
    .collect::<BTreeSet<_>>()
    .len()
    > 1;

  let reminders = reminders
    .iter()
    .map(|r| {
      let time = DateTime::<Local>::from(r.time);

      FormattedReminder {
        relative: format_relative(now, time),
        time,
        message: r.message.clone(),
      }
    })
    .collect::<Vec<_>>();

  let relative_width = reminders
    .iter()
    .map(|r| r.relative.len())
    .max()
    .unwrap_or(0);

  let mut out = String::new();
  let mut current_day = None;

  for reminder in reminders {
    let day = reminder.time.date_naive();

    if grouped && current_day != Some(day) {
      current_day = Some(day);

      if !out.is_empty() {
        let () = out.push('\n');
      }

      let header = if day == today {
        Cow::Borrowed("Today")
      } else if day == today + Duration::days(1) {
        Cow::Borrowed("Tomorrow")
      } else {
        Cow::Owned(reminder.time.format("%A").to_string())
      };

      let () = out.push_str(&header);
      let () = out.push('\n');
    }

    if grouped {
      let () = out.push_str("  ");
    }

    let () = out.push_str(&reminder.time.format("%H:%M").to_string());
    let () = out.push_str(" (");
    let () = out.push_str(&format!(
      "in {:>width$}",
      reminder.relative,
      width = relative_width,
    ));
    let () = out.push_str("): ");
    let () = out.push_str(&reminder.message);
    let () = out.push('\n');
  }

  out
}

fn format_relative(now: DateTime<Local>, target: DateTime<Local>) -> String {
  let delta = target - now;

  let secs = delta.num_seconds();
  let mins = delta.num_minutes();
  let hours = delta.num_hours();
  let days = delta.num_days();

  if secs < 60 {
    format!("{secs}s")
  } else if mins < 60 {
    format!("{mins}m")
  } else if hours < 24 {
    let m = mins % 60;

    if m == 0 {
      format!("{hours}h")
    } else {
      format!("{hours}h {m}m")
    }
  } else {
    let h = hours % 24;

    if h == 0 {
      format!("{days}d")
    } else {
      format!("{days}d {h}h")
    }
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use std::time::UNIX_EPOCH;

  use chrono::TimeZone as _;

  use postcard::from_bytes;
  use postcard::to_allocvec;


  /// Test that we can serialize a [`Reminder`] and then deserialize it
  /// again.
  #[test]
  fn round_trip_after_epoch() {
    let reminder = Reminder {
      time: UNIX_EPOCH + Duration::from_secs(1_234_567),
      message: "Buy milk".to_string(),
    };

    let buf = to_allocvec(&reminder).unwrap();

    let decoded = from_bytes::<Reminder>(&buf).unwrap();
    assert_eq!(decoded, reminder);
  }

  /// Check that we can properly serialize & deserialize Unicode.
  #[test]
  fn round_trip_unicode_message() {
    let reminder = Reminder {
      time: SystemTime::now(),
      message: "☕ Привет 世界".to_string(),
    };

    let buf = to_allocvec(&reminder).unwrap();
    let decoded = from_bytes::<Reminder>(&buf).unwrap();
    assert_eq!(decoded.message, reminder.message);
    assert_eq!(decoded.time, reminder.time);
  }

  /// Make sure that deserializing from a truncated stream fails.
  #[test]
  fn truncated_input_fails() {
    let reminder = Reminder {
      time: UNIX_EPOCH,
      message: "hello".to_string(),
    };

    let mut buf = to_allocvec(&reminder).unwrap();
    let _byte = buf.pop();

    let result = from_bytes::<Reminder>(&buf);
    assert!(result.is_err(), "{result:?}");
  }

  fn reminder(dt: DateTime<Local>, message: &str) -> Reminder {
    Reminder {
      time: dt.into(),
      message: message.to_owned(),
    }
  }

  /// Check that we can format relative durations properly.
  #[test]
  fn relative_duration_formatting() {
    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 6, 23, 10, 15, 0).unwrap();
    assert_eq!(format_relative(now, target), "15m");

    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 6, 23, 12, 30, 0).unwrap();
    assert_eq!(format_relative(now, target), "2h 30m");

    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 6, 23, 13, 0, 0).unwrap();
    assert_eq!(format_relative(now, target), "3h");

    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 6, 25, 15, 0, 0).unwrap();
    assert_eq!(format_relative(now, target), "2d 5h");

    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 6, 25, 10, 0, 0).unwrap();
    assert_eq!(format_relative(now, target), "2d");
  }

  /// Make sure that we do not emit day "markers" when all reminders are
  /// within a single day.
  #[test]
  fn no_same_day_grouping() {
    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();

    let mut reminders = vec![
      reminder(
        Local.with_ymd_and_hms(2026, 6, 23, 11, 0, 0).unwrap(),
        "foo",
      ),
      reminder(
        Local.with_ymd_and_hms(2026, 6, 23, 12, 0, 0).unwrap(),
        "bar",
      ),
    ];

    let output = format_reminders_at(now, &mut reminders);
    assert!(!output.contains("Today"));
  }

  /// Verify the fully formatted output for multiple reminders spanning
  /// multiple days.
  #[test]
  fn full_output_formatting() {
    let now = Local.with_ymd_and_hms(2026, 6, 23, 10, 0, 0).unwrap();

    let mut reminders = vec![
      reminder(
        Local.with_ymd_and_hms(2026, 6, 23, 10, 15, 0).unwrap(),
        "Coffee",
      ),
      reminder(
        Local.with_ymd_and_hms(2026, 6, 23, 13, 0, 0).unwrap(),
        "Lunch",
      ),
      reminder(
        Local.with_ymd_and_hms(2026, 6, 24, 9, 0, 0).unwrap(),
        "Meeting",
      ),
    ];

    let output = format_reminders_at(now, &mut reminders);
    let expected = "\
Today
  10:15 (in 15m): Coffee
  13:00 (in  3h): Lunch

Tomorrow
  09:00 (in 23h): Meeting
";
    assert_eq!(output, expected);
  }
}
