// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::remove_file;
use std::io;
use std::io::ErrorKind;
use std::io::Read as _;
use std::io::Write as _;
use std::io::stdout;
use std::mem::MaybeUninit;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::exit;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::thread;
use std::thread::ScopedJoinHandle;
use std::thread::Thread;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::ensure;

use dbus::arg::PropMap;
use dbus::blocking::Connection;

use libc::O_RDWR;
use libc::SIGINT;
use libc::SIGTERM;
use libc::STDERR_FILENO;
use libc::STDIN_FILENO;
use libc::STDOUT_FILENO;
use libc::c_int;
use libc::chdir;
use libc::close;
use libc::dup2;
use libc::fork;
use libc::open;
use libc::setsid;
use libc::sigaction;
use libc::sigemptyset;
use libc::umask;

use postcard::from_bytes as postcard_from_bytes;
use postcard::to_io as postcard_to_io;

use crate::args::Command;
use crate::args::RemindAt;
use crate::channel::Channel;
use crate::reminder::Reminder;
use crate::reminder::format_reminders;
use crate::reminders::Reminders;
use crate::request::Request;


/// The thread we wake up when we received an "exit signal".
static THREAD: OnceLock<Thread> = OnceLock::new();
/// Whether or not we should shut down and exit.
static EXITING: AtomicBool = AtomicBool::new(false);


extern "C" fn handle_exit_signal(_signum: c_int) {
  let () = EXITING.store(true, Ordering::SeqCst);

  if let Some(thread) = THREAD.get() {
    let () = thread.unpark();
  }
}


fn daemonize() -> Result<()> {
  match unsafe { fork() } {
    0 => (),
    ..=-1 => return Err(io::Error::last_os_error()).context("failed to fork into background"),
    // NB: Exit the parent immediately to prevent any potential issues
    //     with held locks on the shut down path.
    _pid => exit(0),
  }

  // Become session leader.
  if unsafe { setsid() } < 0 {
    return Err(io::Error::last_os_error()).context("setsid failed")
  }

  // Perform a second fork to prevent us from ever acquiring a
  // controlling TTY again.
  match unsafe { fork() } {
    0 => (),
    ..=-1 => return Err(io::Error::last_os_error()).context("failed to fork second time"),
    _pid => exit(0),
  }

  // Change working directory to a "neutral" one in case we live past
  // the user's session, to not keep a potential mount busy, for example.
  if unsafe { chdir(c"/".as_ptr()) } < 0 {
    return Err(io::Error::last_os_error()).context("chdir failed")
  }

  // Reset file mode mask to not inherit behavior from current user.
  let _mode = unsafe { umask(0) };

  // Redirect stdio to `/dev/null`.
  let fd = unsafe { open(c"/dev/null".as_ptr(), O_RDWR) };
  if fd >= 0 {
    let _rc = unsafe { dup2(fd, STDIN_FILENO) };
    let _rc = unsafe { dup2(fd, STDOUT_FILENO) };
    let _rc = unsafe { dup2(fd, STDERR_FILENO) };
    if fd > STDERR_FILENO {
      let _rc = unsafe { close(fd) };
    }
  }
  Ok(())
}


fn install_signal_handlers(signal_thread: &Thread) -> Result<()> {
  // SANITY: Nobody else should ever set `THREAD`.
  let () = THREAD.set(Thread::clone(signal_thread)).unwrap();

  // SAFETY: `sigaction` is valid for any bit pattern.
  let mut action = unsafe { MaybeUninit::<sigaction>::zeroed().assume_init() };
  action.sa_sigaction = handle_exit_signal as *const () as usize;
  let rc = unsafe { sigemptyset(&mut action.sa_mask) };
  ensure!(rc == 0, "failed to clear sigaction mask");

  let rc = unsafe { sigaction(SIGINT, &action, null_mut()) };
  ensure!(rc == 0, "failed to install SIGINT handler");

  let rc = unsafe { sigaction(SIGTERM, &action, null_mut()) };
  ensure!(rc == 0, "failed to install SIGTERM handler");
  Ok(())
}


/// Listen for requests on `listener`, containing a [`Reminder`]
/// object and wake up `server_thread` on receipt.
fn listen_incoming(
  listener: UnixListener,
  request_send: SyncSender<Request>,
  server_thread: Thread,
  exiting: &AtomicBool,
) -> Result<()> {
  for result in listener.incoming() {
    if exiting.load(Ordering::SeqCst) {
      break
    }

    match result {
      Ok(mut stream) => {
        let mut buf = Vec::new();
        let _cnt = stream
          .read_to_end(&mut buf)
          .context("failed to read Request object data")?;
        let request =
          postcard_from_bytes::<Request>(&buf).context("failed to deserialize Request object")?;
        // TODO: Must not unwrap.
        let () = request_send.send(request).unwrap();
        let () = server_thread.unpark();
      },
      Err(e) => return Err(e).context("failed to accept new incoming connection"),
    }
  }
  Ok(())
}


fn join_thread(thread: ScopedJoinHandle<'_, Result<()>>) -> Result<()> {
  thread.join().unwrap_or_else(|payload| {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
      Err(anyhow!("encountered unexpected panic: {s}"))
    } else if let Some(s) = payload.downcast_ref::<String>() {
      Err(anyhow!("encountered unexpected panic: {s}"))
    } else {
      Err(anyhow!("encountered unexpected panic"))
    }
  })
}

/// Send a D-Bus notification using the given message.
///
/// This function does not wait for the user to acknowledge the message
/// in any form -- it just sends.
fn send_notification(message: &str) -> Result<()> {
  let appname = env!("CARGO_BIN_NAME");
  // Don't replace any notification -- always create a new one.
  let replaces_id = 0u32;
  let icon = "alarm";
  let body = "";
  // Never expire.
  let timeout = 0i32;

  let connection = Connection::new_session().context("failed to connect to session bus")?;

  let proxy = connection.with_proxy(
    "org.freedesktop.Notifications",
    "/org/freedesktop/Notifications",
    Duration::from_secs(5),
  );

  let (_msg_id,) = proxy
    .method_call::<(u32,), _, _, _>(
      "org.freedesktop.Notifications",
      "Notify",
      (
        appname,
        replaces_id,
        icon,
        message,
        body,
        Vec::<String>::new(),
        PropMap::new(),
        timeout,
      ),
    )
    .context("failed to call D-Bus Notify")?;

  Ok(())
}

fn run_now(listener: UnixListener, reminder: Reminder) -> Result<()> {
  let server_thread = thread::current();
  let () = install_signal_handlers(&server_thread)?;

  // The channel used for transferring `Request` objects.
  let (request_send, request_recv) = sync_channel::<Request>(1);

  let mut reminders = Reminders::new(request_recv, reminder);
  let addr = listener
    .local_addr()
    .context("failed to retrieve UNIX socket address")?;

  let () = thread::scope(|scope| {
    let listener_thread =
      scope.spawn(|| listen_incoming(listener, request_send, server_thread, &EXITING));

    let run = || {
      loop {
        if listener_thread.is_finished() {
          return join_thread(listener_thread)
        }

        if EXITING.load(Ordering::SeqCst) {
          return Ok(())
        }

        if let Some(next) = reminders.next_reminder() {
          if let Some(duration) = next.duration() {
            let () = thread::park_timeout(duration);
          }

          // We have no knowledge why we got woken up. It could be because
          // we reached the timeout (i.e., reached the reminder time) or
          // to schedule a new reminder. Start by checking whether we
          // reached the reminder time.
          if next.duration().is_none() {
            if let Err(err) = send_notification(&next.message) {
              eprintln!("failed to send D-Bus notification: {err:#}");
            }
            let () = reminders.remove_next();
          }
        } else {
          // No more reminders scheduled, just exit.
          return Ok(())
        }

        if let Err(()) = reminders.try_recv() {
          // If we failed to read a new reminder we should exit.
          return Ok(())
        }
      }
    };

    let result = run();

    // We are about to exit, but the scope will block us until all
    // threads have exited. So make sure to wake up our listener thread
    // and tell it to exit immediately.
    let () = EXITING.store(true, Ordering::SeqCst);
    let _ignored = UnixStream::connect_addr(&addr);
    // The "remind" thread will exit by virtue of `remind_send` being
    // dropped as part of our `Server` instance.
    let () = drop(reminders);

    result
  })?;

  Ok(())
}

fn run_server(socket_path: &Path, reminder: Reminder, foreground: bool) -> Result<()> {
  // Clean up stale socket if it exists.
  let _result = remove_file(socket_path);

  let listener = match UnixListener::bind(socket_path) {
    Ok(listener) => listener,
    Err(err) if err.kind() == ErrorKind::AddrInUse => {
      // We may have raced trying to create the listener -- and lost.
      // Start over.
      return run(
        socket_path,
        Command::At(RemindAt::from(reminder)),
        foreground,
      )
    },
    Err(err) => {
      return Err(err)
        .with_context(|| format!("failed to bind to Unix socket `{}`", socket_path.display()))
    },
  };

  if !foreground {
    let () = daemonize()?;
  }

  run_now(listener, reminder)
}


fn signal_server(stream: UnixStream, command: Command) -> Result<()> {
  let send = |request| -> Result<()> {
    let _stream =
      postcard_to_io(request, stream).context("failed to send Request object to server")?;
    Ok(())
  };

  match command {
    Command::At(remind_at) => {
      let request = Request::from(remind_at);
      let () = send(&request)?;
      Ok(())
    },
    Command::In(remind_in) => {
      let request = Request::from(remind_in);
      let () = send(&request)?;
      Ok(())
    },
    Command::List => {
      let (read, write) = Channel::oneshot().unwrap();
      let request = Request::List(write);

      let () = send(&request)?;

      // TODO: Must not unwrap.
      let mut list = read
        .read()
        .context("failed to read Request::List response")
        .unwrap();

      let _result = stdout().write(format_reminders(&mut list).as_bytes());
      Ok(())
    },
  }
}

pub(crate) fn run(socket_path: &Path, command: Command, foreground: bool) -> Result<()> {
  match UnixStream::connect(socket_path) {
    Ok(stream) => signal_server(stream, command),
    Err(_) => match command {
      Command::At(remind_at) => {
        let reminder = Reminder::from(remind_at);
        let () = run_server(socket_path, reminder, foreground)?;
        Ok(())
      },
      Command::In(remind_in) => {
        let reminder = Reminder::from(remind_in);
        let () = run_server(socket_path, reminder, foreground)?;
        Ok(())
      },
      Command::List => {
        // We couldn't connect to a server so we assume there is none.
        // Thus, there are also no reminders to list.
        Ok(())
      },
    },
  }
}
