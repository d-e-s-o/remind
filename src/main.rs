// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

mod args;
mod reminder;
mod reminders;
mod request;
mod server;

use std::env;
use std::env::args_os;
use std::env::temp_dir;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;

use clap::Parser as _;

use crate::args::Args;


fn socket_path() -> PathBuf {
  let socket = format!("{}.sock", env!("CARGO_PKG_NAME"));
  // Prefer `XDG_RUNTIME_DIR` for the socket if it exists, otherwise
  // fall back to temp dir.
  if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
    let path = PathBuf::from(&runtime_dir);
    if path.exists() {
      return path.join(socket);
    }
  }

  temp_dir().join(socket)
}

fn main_impl(args: Args) -> Result<()> {
  let Args {
    command,
    foreground,
  } = args;

  let socket_path = socket_path();
  let () = server::run(&socket_path, command, foreground)?;
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
