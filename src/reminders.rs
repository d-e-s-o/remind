// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::mpsc::Receiver;
use std::sync::mpsc::TryRecvError;

use crate::reminder::Reminder;


/// A container for a set of reminders, ordered by due-time.
#[derive(Debug)]
pub(crate) struct Reminders {
  /// The receiver end of a channel to receive new reminders from.
  remind_recv: Receiver<Reminder>,
  /// The sorted list of reminders.
  ///
  /// The next one due is always the last one.
  reminders: Vec<Reminder>,
}

impl Reminders {
  /// Create a new `Reminders` object with a single initial managed
  /// reminder.
  pub fn new(remind_recv: Receiver<Reminder>, reminder: Reminder) -> Self {
    Self {
      remind_recv,
      reminders: vec![reminder],
    }
  }

  /// Try to receive a new reminder and enqueue it.
  ///
  /// This method returns `Ok(())`, except when the channel to receive
  /// reminders on has been closed.
  pub fn try_recv(&mut self) -> Result<(), ()> {
    let result = self.remind_recv.try_recv();
    let reminder = match result {
      Ok(reminder) => reminder,
      Err(TryRecvError::Empty) => {
        // No data? Nothing for us to do.
        return Ok(())
      },
      Err(TryRecvError::Disconnected) => {
        // If the sender has disconnected we should be shutting down.
        return Err(())
      },
    };

    let result = self
      .reminders
      .binary_search_by(|r| r.cmp(&reminder).reverse());
    match result {
      Ok(..) => {
        // It would seem we have the exact reminder scheduled already.
        // No point in enqueuing it again; just ignore the request.
      },
      Err(idx) => {
        let () = self.reminders.insert(idx, reminder);
      },
    }
    Ok(())
  }

  /// Query the next due [`Reminder`].
  #[inline]
  pub fn next_reminder(&self) -> Option<&Reminder> {
    self.reminders.last()
  }

  /// Remove the next due [`Reminder`].
  #[inline]
  pub fn remove_next(&mut self) {
    let _reminder = self.reminders.pop();
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use std::sync::mpsc::channel;
  use std::time::Duration;
  use std::time::UNIX_EPOCH;


  fn reminder(secs: u64, msg: &str) -> Reminder {
    Reminder {
      time: UNIX_EPOCH + Duration::from_secs(secs),
      message: msg.to_string(),
    }
  }


  /// Check that [`Reminders`] correctly reports the initial
  /// [`Reminder`] provided.
  #[test]
  fn initial_reminder_retrieval() {
    let (_tx, rx) = channel();

    let r = reminder(10, "initial");
    let reminders = Reminders::new(rx, r.clone());

    assert_eq!(reminders.next_reminder(), Some(&r));
  }

  /// Make sure that [`Reminders::try_recv`] gracefully handles the case
  /// of no new [`Reminder`] objects being available.
  #[test]
  fn no_reminder_receive() {
    let (_tx, rx) = channel();

    let mut reminders = Reminders::new(rx, reminder(10, "initial"));

    assert_eq!(reminders.try_recv(), Ok(()));
    assert_eq!(reminders.next_reminder().unwrap().message, "initial");
  }

  /// Test that a disconnected channel causes [`Reminders::try_recv`] to
  /// report an error.
  #[test]
  fn disconnected_channel_receive() {
    let (tx, rx) = channel::<Reminder>();

    let mut reminders = Reminders::new(rx, reminder(10, "initial"));
    drop(tx);

    assert_eq!(reminders.try_recv(), Err(()));
  }

  /// Verify that reminders are reported in the correct order.
  #[test]
  fn reminder_ordering() {
    let (tx, rx) = channel();

    let initial = reminder(20, "20");
    let mut reminders = Reminders::new(rx, initial);

    let () = tx.send(reminder(10, "10")).unwrap();
    let () = reminders.try_recv().unwrap();

    let () = tx.send(reminder(30, "30")).unwrap();
    let () = reminders.try_recv().unwrap();

    // Earliest due reminder should be returned.
    assert_eq!(reminders.next_reminder().unwrap().message, "10");

    let () = reminders.remove_next();
    assert_eq!(reminders.next_reminder().unwrap().message, "20");

    let () = reminders.remove_next();
    assert_eq!(reminders.next_reminder().unwrap().message, "30");
  }

  /// Check that duplicate reminders are ignored when received.
  #[test]
  fn duplicate_reminder_ignoring() {
    let (tx, rx) = channel();

    let r = reminder(10, "duplicate");
    let mut reminders = Reminders::new(rx, r.clone());

    let () = tx.send(r).unwrap();
    let () = reminders.try_recv().unwrap();

    // Remove first copy.
    let () = reminders.remove_next();

    // If a duplicate had been inserted, we'd still have one left.
    assert_eq!(reminders.next_reminder(), None);
  }

  /// Make sure that calling [`Reminders::remove_next`] on an empty
  /// container does nothing.
  #[test]
  fn remove_next_on_empty_container_is_safe() {
    let (_tx, rx) = channel();

    let mut reminders = Reminders::new(rx, reminder(10, "10"));

    let () = reminders.remove_next();
    let () = reminders.remove_next();

    assert_eq!(reminders.next_reminder(), None);
  }
}
