// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Deserialize;
use serde::Serialize;

use crate::args::RemindAt;
use crate::args::RemindIn;
use crate::channel::Writer;
use crate::reminder::Reminder;


#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum Request {
  Remind(Reminder),
  List(Writer<Vec<Reminder>>),
}

impl From<RemindAt> for Request {
  fn from(other: RemindAt) -> Self {
    Self::Remind(Reminder::from(other))
  }
}

impl From<RemindIn> for Request {
  fn from(other: RemindIn) -> Self {
    Self::Remind(Reminder::from(other))
  }
}
