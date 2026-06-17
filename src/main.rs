// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

mod args;
mod reminder;
mod reminders;

use std::env;
use std::env::args_os;
use std::process::ExitCode;

use anyhow::Result;

use clap::Parser as _;

use crate::args::Args;


fn main_impl(args: Args) -> Result<()> {
  let Args {
    command,
    message,
    foreground,
  } = args;

  Ok(())
}

fn main() -> ExitCode {
  let args = match args::Args::try_parse_from(args_os()) {
    Ok(args) => args,
    Err(err) => {
      let _result = err.print();
      return u8::try_from(err.exit_code())
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
    },
  };

  main_impl(args)
    .map(|()| ExitCode::SUCCESS)
    .map_err(|e| eprintln!("{e:?}"))
    .unwrap_or(ExitCode::FAILURE)
}
