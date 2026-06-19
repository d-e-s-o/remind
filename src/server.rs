// Copyright (C) 2026 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::remove_file;
use std::io;
use std::io::ErrorKind;
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

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::ensure;

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

use crate::reminder::Reminder;
use crate::reminders::Reminders;


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
  remind_send: SyncSender<Reminder>,
  server_thread: Thread,
  exiting: &AtomicBool,
) -> Result<()> {
  for result in listener.incoming() {
    if exiting.load(Ordering::SeqCst) {
      break
    }

    match result {
      Ok(mut stream) => {
        let reminder =
          Reminder::read_from(&mut stream).context("failed to read Reminder object")?;
        // TODO: Must not unwrap.
        let () = remind_send.send(reminder).unwrap();
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

fn run_now(listener: UnixListener, reminder: Reminder) -> Result<()> {
  let server_thread = thread::current();
  let () = install_signal_handlers(&server_thread)?;

  // The channel used for transferring `Reminder` objects to enqueue.
  let (remind_send, remind_recv) = sync_channel::<Reminder>(1);

  let mut reminders = Reminders::new(remind_recv, reminder);
  let addr = listener
    .local_addr()
    .context("failed to retrieve UNIX socket address")?;

  let () = thread::scope(|scope| {
    let listener_thread =
      scope.spawn(|| listen_incoming(listener, remind_send, server_thread, &EXITING));

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
            // TODO: Send D-Bus notification.
            println!("{}", next.message);
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
      return run(socket_path, reminder, foreground)
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


fn signal_server(mut stream: UnixStream, reminder: Reminder) -> Result<()> {
  let () = reminder
    .write_to(&mut stream)
    .context("failed to send Reminder object to server")?;
  Ok(())
}

pub(crate) fn run(socket_path: &Path, reminder: Reminder, foreground: bool) -> Result<()> {
  match UnixStream::connect(socket_path) {
    Ok(stream) => signal_server(stream, reminder),
    Err(_) => run_server(socket_path, reminder, foreground),
  }
}
