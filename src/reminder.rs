// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Read;
use std::io::Write;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context as _;
use anyhow::Result;


/// The representation of a reminder to schedule.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Reminder {
  /// The time at which the reminder should show.
  pub time: SystemTime,
  /// The remind message.
  pub message: String,
}

impl Reminder {
  pub fn write_to<W>(&self, mut w: W) -> Result<()>
  where
    W: Write,
  {
    let nanos = self
      .time
      .duration_since(UNIX_EPOCH)
      .context("reminder time is before UNIX epoch")?
      .as_nanos();
    let nanos = u64::try_from(nanos).context("reminder time is too large")?;
    let () = w.write_all(&nanos.to_ne_bytes())?;

    let msg = self.message.as_bytes();
    let len = msg.len();

    let () = w.write_all(&len.to_ne_bytes())?;
    let () = w.write_all(msg)?;

    Ok(())
  }

  pub fn read_from<R>(mut r: R) -> Result<Self>
  where
    R: Read,
  {
    let mut nanos_buf = 0u64.to_ne_bytes();
    let () = r.read_exact(&mut nanos_buf)?;
    let nanos = u64::from_le_bytes(nanos_buf);
    let time = UNIX_EPOCH
      .checked_add(Duration::from_nanos(nanos))
      .context("reminder time is too large")?;

    let mut len_buf = 0usize.to_ne_bytes();
    let () = r.read_exact(&mut len_buf)?;
    let len = usize::from_le_bytes(len_buf);

    let mut msg_buf = vec![0u8; len];
    let () = r.read_exact(&mut msg_buf)?;

    let message = String::from_utf8(msg_buf).context("reminder message is not valid UTF-8")?;

    let reminder = Self { time, message };
    Ok(reminder)
  }

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

  use std::io::Cursor;


  /// Test that we can serialize a [`Reminder`] and then deserialize it
  /// again.
  #[test]
  fn round_trip_after_epoch() {
    let reminder = Reminder {
      time: UNIX_EPOCH + Duration::from_secs(1_234_567),
      message: "Buy milk".to_string(),
    };

    let mut buf = Vec::new();
    reminder.write_to(&mut buf).unwrap();

    let decoded = Reminder::read_from(Cursor::new(buf)).unwrap();
    assert_eq!(decoded, reminder);
  }

  /// Check that we can properly serialize & deserialize Unicode.
  #[test]
  fn round_trip_unicode_message() {
    let reminder = Reminder {
      time: SystemTime::now(),
      message: "☕ Привет 世界".to_string(),
    };

    let mut buf = Vec::new();
    let () = reminder.write_to(&mut buf).unwrap();

    let decoded = Reminder::read_from(Cursor::new(buf)).unwrap();
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

    let mut buf = Vec::new();
    let () = reminder.write_to(&mut buf).unwrap();
    let _byte = buf.pop();

    let result = Reminder::read_from(Cursor::new(buf));
    assert!(result.is_err(), "{result:?}");
  }
}
