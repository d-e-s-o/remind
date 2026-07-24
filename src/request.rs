// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Deserialize;
use serde::Serialize;

use crate::args::RemindIn;
use crate::reminder::Reminder;


#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum Request {
  Remind(Reminder),
}

impl From<RemindIn> for Request {
  fn from(other: RemindIn) -> Self {
    Self::Remind(Reminder::from(other))
  }
}
