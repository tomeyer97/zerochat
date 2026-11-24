//! The daemon: the one writer of a feature, running the scheduler until the feature is done or
//! stopped. How the CLI hands it requests is in [`client`] ([`submit`]); starting and stopping
//! it (in its own session or as a systemd user unit) is in [`service`].

mod client;
mod service;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};

pub use client::submit;
pub use service::{install_service, spawn, spawn_with, stop, unit_path, unit_text};

use crate::engine::Engine;
use crate::model::EventKind;
use crate::store::Feature;
use crate::zeroshot::{Zeroshot, ZeroshotCli};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn install_signals() {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
}

/// The real Zeroshot for a feature (its program and its state directory).
pub fn zeroshot_for(feature: &Feature) -> Arc<dyn Zeroshot> {
    let program = crate::store::expand_home(feature.config.launch.program());
    let env = feature
        .config
        .env
        .iter()
        .map(|(k, v)| (k.clone(), crate::store::expand_home(v)))
        .collect();
    Arc::new(
        ZeroshotCli::new(
            program,
            crate::store::zeroshot_state_root(&feature.config.env),
        )
        .with_env(env),
    )
}

/// Runs the scheduler until the feature is done, stopped, or the process is signalled.
/// Holds `writer.lock` the whole time: while it runs, it is the only writer.
pub fn run(feature: Feature, zeroshot: Arc<dyn Zeroshot>) -> Result<()> {
    let _lock = match feature.try_writer_lock()? {
        Some(lock) => lock,
        None => {
            if let Some(pid) = feature.daemon_pid() {
                bail!(
                    "the daemon of {} is already running (pid {pid})",
                    feature.name()
                );
            }
            feature.writer_lock()?
        }
    };
    std::fs::write(feature.pid_path(), std::process::id().to_string())?;
    install_signals();
    let result = run_locked(feature.clone(), zeroshot);
    let _ = std::fs::remove_file(feature.pid_path());
    result
}

fn run_locked(feature: Feature, zeroshot: Arc<dyn Zeroshot>) -> Result<()> {
    let mut engine = Engine::open(feature, zeroshot)?;
    engine.log_event(
        EventKind::DaemonStarted,
        format!("pid {}", std::process::id()),
    );
    engine.recover();
    loop {
        let tick = match engine.tick() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("zsf: tick failed: {e:#}");
                Default::default()
            }
        };
        // Requests that arrived meanwhile are answered before the daemon goes.
        if (STOP.load(Ordering::SeqCst) || tick.done) && !engine.feature.has_pending_requests() {
            break;
        }
        if !engine.state.active
            && engine.jobs_running() == 0
            && !engine.feature.has_pending_requests()
        {
            break;
        }
        let poll = Duration::from_secs(engine.feature.config.limits.poll_secs.max(1));
        engine.wait(poll, &|| STOP.load(Ordering::SeqCst));
    }
    // Finish work in flight (a launch must not go unrecorded), but start nothing new.
    engine.stopping = true;
    engine.drain_jobs()?;
    engine.log_event(EventKind::DaemonStopped, String::new());
    engine.save()
}
