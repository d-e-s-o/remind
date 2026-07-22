// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;
use serde::Serialize;


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


#[cfg(test)]
mod tests {
  use super::*;

  use std::time::UNIX_EPOCH;

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
}
