//! zerochat: observe, steer and replay Zeroshot runs.
//!
//! How data flows: Zeroshot's files (state directories, ledgers) and its CLI → `source` reads a
//! run → `projection` folds its events into the run view, `transcript` into chat items →
//! `server` answers the web UI's HTTP API → the React app in `web/`. Actions on a run go the
//! other way, through the `zeroshot` CLI (`actions`).
//!
//! Two optional companion tools have pages of their own: pb (the ProgramBench runner, a separate
//! benchmarking tool behind "Benchmarks") and zsf (Zeroshot features, the slice scheduler in
//! this workspace behind "Features"). The modules are grouped below by layer.

// Process shell: settings, where runs come from, the embedded web UI, startup.
mod assets; // the web UI, embedded at build time
mod browser; // opening the browser at startup, only where one can open
mod config; // zerochat's config.toml: extra run sources, pb and zsf settings
mod discovery; // the targets runs come from, and the setup check (Zeroshot, state dir, zsf, pb)
mod doctor; // `zerochat doctor`: the setup check, exit code on problems
mod paths; // where things live on disk (state and config directories, a run's layout)
mod port; // the listen address: the default port, the next free one, a zerochat already running

// HTTP: the API the web UI calls.
mod server; // routes, request boundary, shared state, run listings

// Runs: Zeroshot's data, read and turned into views.
mod engine_lines; // Zeroshot's log lines about the provider (retries, rate limits)
mod harness_name;
mod ledger; // queries over a run's ledger database for readers outside `source`
mod pricing; // estimated cost from token usage
mod projection; // the run view: nodes, executions, time, tokens, cost, failures
mod source; // reads a run's definition and ledger events, locally or from a direct target
mod transcript; // a node execution's conversation as chat items // the agent CLIs Zeroshot runs nodes with

// Changing runs and reading their workspaces.
mod actions; // the zeroshot CLI client (steer, stop, resume, replay) and read-only git views
mod workspace; // reading files from a run's workspace without leaving it

// pb integration (Benchmarks).
mod benchmarks; // the Benchmarks page's data, from pb's database
mod context;
mod harness; // what a pb harness is: agent nodes, models, loops
mod pb_db; // opens pb's database read-only
mod pb_exec; // runs pb for the actions only pb's code can do // which benchmark batch or feature slice a run belongs to

// zsf integration (Features).
mod features; // features, slices and their actions, through the zsf library

// Shared helpers.
mod clock; // now in Unix milliseconds (fixed in golden tests)
mod text; // text cut to a byte budget
mod validate; // safe ids and arguments for paths and command lines

// Test-only.
// The generated TypeScript twins of the API types (web/src/generated/, `make types`).
#[cfg(test)]
mod api_types;
#[cfg(test)]
mod test_events; // ledger events for tests, in Zeroshot 16.1's shape

// Golden/snapshot tests (tests/GOLDENS.md). zerochat is a binary crate, so they live in the
// crate as a test-only module whose source sits under tests/goldens/.
#[cfg(test)]
#[path = "../tests/goldens/mod.rs"]
mod goldens;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "zerochat",
    version,
    about = "Observe, steer and replay Zeroshot runs in your browser.",
    after_help = "Serves on http://127.0.0.1:4380/ (the next free port, up to 4390, when it is busy) and opens your\nbrowser when there is one (an interactive terminal with a display, not over SSH or in CI).\nIf a zerochat already runs there, it opens that one instead.\n\nCheck your setup (Zeroshot, its state directory, zsf, pb) with `zerochat doctor`."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// Address to serve on [default: 127.0.0.1:4380, else the next free port up to 4390]. Given
    /// here or in ZEROCHAT_LISTEN, it is used as it is or not at all. Only loopback addresses are
    /// allowed unless --allow-remote is set.
    #[arg(long, env = "ZEROCHAT_LISTEN")]
    listen: Option<SocketAddr>,
    /// Fail when the port is busy instead of trying the next ones.
    #[arg(long)]
    strict_port: bool,
    /// The zeroshot executable used for actions (status, stop, resume, steer, replay).
    #[arg(
        long,
        default_value = "zeroshot",
        env = "ZEROCHAT_ZEROSHOT",
        global = true
    )]
    zeroshot: PathBuf,
    /// Serve on a non-loopback address (for containers). Anyone who can reach it can steer
    /// and stop runs, so only use this behind your own network boundary.
    #[arg(long)]
    allow_remote: bool,
    /// Extra Host header values to accept, repeatable: an exact `name[:port]` (e.g. a
    /// container's service name `zerochat:4317`) or a suffix pattern `*.localhost:8090`. A name
    /// without a port matches any port. There is no catch-all: `*` is refused.
    #[arg(long = "allowed-host")]
    allowed_hosts: Vec<String>,
    /// Answer requests for any Host header (turns off the DNS-rebinding check). Only for when
    /// zerochat is reached through a proxy that logs you in, and no browser runs on this machine.
    #[arg(long, env = "ZEROCHAT_ANY_HOST")]
    any_host: bool,
    /// Do not open a browser. zerochat also does not open one without an interactive terminal,
    /// without a display, over SSH, in CI (`CI`), or with BROWSER=none.
    #[arg(long)]
    no_open: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Check the setup and exit: Zeroshot (found, version 16.1+, stop/resume and steering), the
    /// state directory zerochat watches and its runs, zsf (Zeroshot features) and pb (ProgramBench
    /// runner). Exits with 1 when Zeroshot is missing or too old, or the state directory cannot
    /// be read; each problem names its fix.
    Doctor {
        /// Print the check as JSON (the answer of GET /api/setup).
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let log_filter = tracing_subscriber::EnvFilter::try_from_env("ZEROCHAT_LOG")
        .unwrap_or_else(|_| "warn".into());
    tracing_subscriber::fmt().with_env_filter(log_filter).init();
    let args = Args::parse();
    match args.command {
        Some(Command::Doctor { json }) => doctor(&args.zeroshot, json).await,
        None => serve(args).await,
    }
}

fn app_state(zeroshot: &PathBuf, hosts: server::HostAllowlist) -> server::AppState {
    server::AppState::new(
        paths::zeroshot_state_dir(),
        paths::zeroshot_config_dir(),
        paths::zerochat_config_dir(),
        actions::Cli {
            program: resolve_program(zeroshot),
        },
        hosts,
    )
}

async fn doctor(zeroshot: &PathBuf, json: bool) -> Result<ExitCode> {
    let state = app_state(zeroshot, server::HostAllowlist::any());
    let setup = server::setup_check(&state).await;
    let (text, ok) = doctor::report(&setup, &state.home, json);
    print!("{text}");
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn serve(args: Args) -> Result<ExitCode> {
    let (listen, strictness) = match args.listen {
        Some(listen) => (listen, port::Strictness::Explicit),
        None if args.strict_port => (port::DEFAULT_LISTEN, port::Strictness::StrictPort),
        None => (port::DEFAULT_LISTEN, port::Strictness::Fallback),
    };
    if !listen.ip().is_loopback() && !args.allow_remote {
        bail!(
            "{listen} is not a loopback address; zerochat can stop and steer runs, so it only listens on loopback. Pass --allow-remote to override."
        );
    }
    let browser = browser::decide(&browser::Env::from_process(args.no_open));
    let listener = match port::bind(listen, strictness).await? {
        port::Bound::Listening(listener) => listener,
        port::Bound::AlreadyRunning(url) => {
            println!(
                "zerochat is already running at {url}   {}",
                browser::note(&browser)
            );
            browser::open(&browser, &url);
            return Ok(ExitCode::SUCCESS);
        }
    };
    let listen = listener.local_addr().context("the listen address")?;
    let hosts = server::HostAllowlist::for_listen(listen, &args.allowed_hosts, args.any_host)
        .map_err(anyhow::Error::msg)?;
    let state = Arc::new(app_state(&args.zeroshot, hosts));
    let url = port::url(listen);
    print_banner(&url, &state, &browser).await;
    browser::open(&browser, &url);
    axum::serve(listener, server::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(ExitCode::SUCCESS)
}

fn resolve_program(program: &PathBuf) -> PathBuf {
    if program.components().count() > 1 {
        return program.clone();
    }
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(program))
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| program.clone())
}

/// The URL, then the setup check in aligned lines (a ✗ line names its fix; the server starts
/// anyway), then where zerochat's targets and sources are configured.
async fn print_banner(url: &str, state: &server::AppState, browser: &browser::Decision) {
    let setup = server::setup_check(state).await;
    let tilde = |path: PathBuf| discovery::shorten_home(&path.display().to_string(), &state.home);
    println!(
        "zerochat {} → {url}   {}",
        setup.version,
        browser::note(browser)
    );
    for line in setup.lines(&state.home) {
        println!("{line}");
    }
    println!(
        "  {:<11}{}",
        "Targets",
        tilde(state.config_root.join("targets.json"))
    );
    println!(
        "  {:<11}{}",
        "Sources",
        tilde(state.zerochat_config_root.join("config.toml"))
    );
}
