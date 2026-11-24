use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use zsf::model::{Action, RequestResult, SliceEdit};
use zsf::store::{self, Feature};

/// Drive a feature cut into slices through Zeroshot, one run per slice.
#[derive(Parser)]
#[command(name = "zsf", version)]
struct Cli {
    /// Where features live (default $XDG_STATE_HOME/zsf, else ~/.local/state/zsf).
    #[arg(long, global = true, env = "ZSF_STATE_DIR")]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a feature: its directory, feature branch (from BASE) and plan.
    Init {
        name: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        base: String,
        /// Import this refined spec (slices/INDEX.md + slices/NN-*.md).
        #[arg(long, conflicts_with = "plan")]
        refined: Option<PathBuf>,
        /// Use this plan.toml.
        #[arg(long)]
        plan: Option<PathBuf>,
        /// Checkpoint slices, e.g. 02,06: each waits, not merged, for your review.
        #[arg(long, value_delimiter = ',')]
        checkpoints: Vec<String>,
        #[arg(long)]
        graph: Option<PathBuf>,
        #[arg(long)]
        runtime: Option<PathBuf>,
        #[arg(long)]
        max_parallel: Option<u32>,
        /// The zeroshot program (default `zeroshot` on PATH).
        #[arg(long)]
        zeroshot: Option<String>,
    },
    /// Convert a refined spec into plan.toml (of a feature, or a file).
    Import {
        dir: PathBuf,
        #[arg(long)]
        feature: Option<String>,
        /// Without --feature: where to write the plan.
        #[arg(long, short, default_value = "plan.toml")]
        out: PathBuf,
        #[arg(long, value_delimiter = ',')]
        checkpoints: Vec<String>,
    },
    /// Start scheduling (starts the daemon).
    Start {
        feature: Option<String>,
        /// Spawn the daemon directly even when the systemd unit is installed.
        #[arg(long)]
        no_systemd: bool,
    },
    /// Stop the daemon; running Zeroshot runs continue and are picked up by the next start.
    Stop { feature: Option<String> },
    /// Start the daemon of every started feature whose daemon is not running (for a shell's
    /// startup file on machines without systemd). Quiet and idempotent.
    Autostart,
    /// One line per slice (all features when none is given and there are several).
    Status {
        feature: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Events, oldest first.
    Log {
        feature: Option<String>,
        #[arg(long)]
        slice: Option<String>,
        #[arg(long)]
        json: bool,
        /// Only the last N events.
        #[arg(short = 'n', long)]
        lines: Option<usize>,
        #[arg(short, long)]
        follow: bool,
    },
    /// Approve a checkpoint slice waiting for review: check (the gate) and merge it; its
    /// dependents start after the merge.
    Approve {
        #[command(flatten)]
        target: SliceTarget,
    },
    /// Run a stopped slice again (same worktree, or --fresh from the branch tip). On a slice
    /// waiting for review: request changes (say which with --notes).
    Retry {
        #[command(flatten)]
        target: SliceTarget,
        /// Appended to this and every later task of the slice.
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        fresh: bool,
    },
    /// You finished a stopped slice by hand in its worktree: squash and merge it.
    Accept {
        #[command(flatten)]
        target: SliceTarget,
    },
    /// Drop a slice; its dependents no longer wait for it.
    Skip {
        #[command(flatten)]
        target: SliceTarget,
    },
    /// Continue after a pause (repeated infrastructure failures).
    Unpause { feature: Option<String> },
    /// Change a slice that has not started.
    Edit {
        #[command(flatten)]
        target: SliceTarget,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        spec: Option<PathBuf>,
        /// Comma-separated slice ids ("" for none).
        #[arg(long)]
        deps: Option<String>,
        #[arg(long)]
        checkpoint: Option<bool>,
        /// Schedule it right before this slice.
        #[arg(long, conflicts_with = "after")]
        before: Option<String>,
        #[arg(long)]
        after: Option<String>,
    },
    /// Change how many Zeroshot runs go at once.
    MaxParallel { feature: Option<String>, n: u32 },
    /// Write the systemd user unit zsf@.service (does not enable it).
    InstallService,
    #[command(hide = true)]
    Daemon { feature: String },
}

// The slice commands' `[FEATURE] SLICE` (clap cannot put an optional positional before a
// required one, so both are one list of one or two values).
#[derive(Args)]
struct SliceTarget {
    #[arg(num_args = 1..=2, value_names = ["FEATURE", "SLICE"])]
    args: Vec<String>,
}

impl SliceTarget {
    /// The feature (named, or the only one) and the slice id.
    fn open(&self, root: &Path) -> Result<(Feature, String)> {
        match self.args.as_slice() {
            [slice] => Ok((open(root, None)?, slice.clone())),
            [name, slice] => Ok((open(root, Some(name.clone()))?, slice.clone())),
            _ => bail!("give FEATURE SLICE"),
        }
    }
}

/// How long a command waits for the daemon (or itself, as the writer) to answer its request
/// before it reports the request as queued.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long `zsf stop` waits for the daemon to exit, and for the answer when no daemon runs:
/// shorter than [`REQUEST_TIMEOUT`] because a daemon that does not exit by then is stopped by
/// signal ([`zsf::daemon::stop`]) instead of waited for.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// How often `zsf stop` looks whether the daemon exited.
const STOP_POLL: Duration = Duration::from_millis(100);
/// How often `zsf log --follow` reads the event log again.
const FOLLOW_POLL: Duration = Duration::from_secs(1);

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("zsf: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let root = cli.state_dir.clone().unwrap_or_else(store::state_root);
    match cli.command {
        Command::Init {
            name,
            repo,
            branch,
            base,
            refined,
            plan,
            checkpoints,
            graph,
            runtime,
            max_parallel,
            zeroshot,
        } => {
            let launch = LaunchOptions {
                graph,
                runtime,
                zeroshot,
                max_parallel,
            };
            let options = zsf::setup::InitOptions {
                name,
                repo,
                branch,
                base,
                plan,
                refined,
                checkpoints,
                configure: Box::new(move |c| launch.apply(c)),
            };
            cmd_init(&root, options)
        }
        Command::Import {
            dir,
            feature: Some(name),
            out: _,
            checkpoints,
        } => cmd_import_into(&root, &name, &dir, &checkpoints),
        Command::Import {
            dir,
            feature: None,
            out,
            checkpoints,
        } => cmd_import_file(&dir, &out, &checkpoints),
        Command::Start {
            feature,
            no_systemd,
        } => {
            let feature = open(&root, feature)?;
            feature.queue_request(Action::Start)?;
            println!("{}", zsf::daemon::spawn(&feature, &root, !no_systemd)?);
            Ok(())
        }
        Command::Autostart => {
            cmd_autostart(&root);
            Ok(())
        }
        Command::Stop { feature } => cmd_stop(&open(&root, feature)?),
        Command::Status { feature, json } => cmd_status(&root, feature, json),
        Command::Log {
            feature,
            slice,
            json,
            lines,
            follow,
        } => cmd_log(&open(&root, feature)?, slice, json, lines, follow),
        Command::Approve { target } => mutate(&root, &target, |slice| Action::Approve { slice }),
        Command::Retry {
            target,
            notes,
            fresh,
        } => mutate(&root, &target, |slice| Action::Retry {
            slice,
            notes,
            fresh,
        }),
        Command::Accept { target } => mutate(&root, &target, |slice| Action::Accept { slice }),
        Command::Skip { target } => mutate(&root, &target, |slice| Action::Skip { slice }),
        Command::Unpause { feature } => submit(&open(&root, feature)?, Action::Unpause),
        Command::MaxParallel { feature, n } => submit(
            &open(&root, feature)?,
            Action::SetMaxParallel { max_parallel: n },
        ),
        Command::Edit {
            target,
            title,
            spec,
            deps,
            checkpoint,
            before,
            after,
        } => {
            let edit = SliceEdit {
                title,
                spec: spec.map(abs),
                deps: deps.map(|d| parse_ids(&d)),
                checkpoint,
                before,
                after,
            };
            mutate(&root, &target, |slice| Action::Edit { slice, edit })
        }
        Command::InstallService => cmd_install_service(&root),
        Command::Daemon { feature } => {
            let feature = Feature::open(&root, &feature)?;
            let zeroshot = zsf::daemon::zeroshot_for(&feature);
            zsf::daemon::run(feature, zeroshot)
        }
    }
}

/// `zsf init`'s launch flags, written into the new `feature.toml`.
struct LaunchOptions {
    graph: Option<PathBuf>,
    runtime: Option<PathBuf>,
    zeroshot: Option<String>,
    max_parallel: Option<u32>,
}

impl LaunchOptions {
    fn apply(self, config: &mut zsf::model::FeatureConfig) {
        config.launch.graph = self.graph.map(abs);
        config.launch.runtime = self.runtime.map(abs);
        config.launch.zeroshot = self.zeroshot;
        if let Some(n) = self.max_parallel {
            config.limits.max_parallel = n;
        }
    }
}

fn cmd_init(root: &Path, options: zsf::setup::InitOptions) -> Result<()> {
    let done = zsf::setup::init(root, options)?;
    for m in done.messages {
        println!("{m}");
    }
    println!(
        "next: review {}/feature.toml and plan.toml, then `zsf start {}`",
        done.feature.dir.display(),
        done.feature.name()
    );
    Ok(())
}

/// `zsf import DIR --feature NAME`: the feature's plan (and context copy).
fn cmd_import_into(root: &Path, name: &str, dir: &Path, checkpoints: &[String]) -> Result<()> {
    let feature = Feature::open(root, name)?;
    let imported = zsf::import::import_into(&feature, dir, checkpoints)?;
    zsf::setup::mark_merged_from_branch(&feature)?;
    print_imported(&imported, &feature.plan_path());
    Ok(())
}

/// `zsf import DIR [--out FILE]`: a plan file only.
fn cmd_import_file(dir: &Path, out: &Path, checkpoints: &[String]) -> Result<()> {
    let mut imported = zsf::import::parse_refined(dir)?;
    imported.plan.mark_checkpoints(checkpoints);
    store::write_atomic(out, toml::to_string_pretty(&imported.plan)?.as_bytes())?;
    print_imported(&imported, out);
    Ok(())
}

fn print_imported(imported: &zsf::import::Imported, plan_path: &Path) {
    for w in &imported.warnings {
        println!("warning: {w}");
    }
    println!(
        "{} slices into {}",
        imported.plan.slices.len(),
        plan_path.display()
    );
}

/// Starts the daemon of every active feature whose daemon is not running; quiet.
fn cmd_autostart(root: &Path) {
    for name in store::list_features(root) {
        let Ok(feature) = Feature::open(root, &name) else {
            continue;
        };
        let active = feature.load_state().map(|s| s.active).unwrap_or(false);
        if active && feature.daemon_pid().is_none() {
            let _ = zsf::daemon::spawn(&feature, root, false);
        }
    }
}

fn cmd_stop(feature: &Feature) -> Result<()> {
    if feature.daemon_pid().is_some() {
        // The daemon applies the stop, finishes the step it is in and exits.
        feature.queue_request(Action::Stop)?;
        let deadline = Instant::now() + STOP_TIMEOUT;
        while feature.daemon_pid().is_some() && Instant::now() < deadline {
            std::thread::sleep(STOP_POLL);
        }
        if feature.daemon_pid().is_none() {
            println!(
                "daemon stopped; running Zeroshot runs continue and are picked up by the next start"
            );
            return Ok(());
        }
    } else {
        answer(zsf::daemon::submit(feature, Action::Stop, STOP_TIMEOUT)?)?;
    }
    println!("{}", zsf::daemon::stop(feature)?);
    Ok(())
}

fn cmd_status(root: &Path, feature: Option<String>, json: bool) -> Result<()> {
    let names = match feature {
        Some(name) => vec![name],
        None => store::list_features(root),
    };
    if names.is_empty() {
        bail!("no features in {} (zsf init)", root.display());
    }
    let mut reports = Vec::new();
    for name in &names {
        reports.push(zsf::status::report(&Feature::open(root, name)?)?);
    }
    if json {
        let value = if reports.len() == 1 {
            serde_json::to_value(&reports[0])?
        } else {
            serde_json::to_value(&reports)?
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        let text: Vec<String> = reports.iter().map(zsf::status::render).collect();
        print!("{}", text.join("\n"));
    }
    Ok(())
}

/// Prints the events (of one slice), the last `lines` of them, then with `follow` every new one.
fn cmd_log(
    feature: &Feature,
    slice: Option<String>,
    json: bool,
    lines: Option<usize>,
    follow: bool,
) -> Result<()> {
    let path = feature.events_path();
    let print = |events: &[zsf::model::Event]| -> Result<()> {
        for e in events
            .iter()
            .filter(|e| slice.is_none() || e.slice == slice)
        {
            if json {
                println!("{}", serde_json::to_string(e)?);
            } else {
                println!("{}", zsf::status::log_line(e));
            }
        }
        Ok(())
    };
    let events = store::read_events(&path);
    let filtered: Vec<_> = events
        .iter()
        .filter(|e| slice.is_none() || e.slice == slice)
        .cloned()
        .collect();
    let skip = lines.map(|n| filtered.len().saturating_sub(n)).unwrap_or(0);
    print(&filtered[skip..])?;
    if !follow {
        return Ok(());
    }
    let mut seen = events.len();
    loop {
        std::thread::sleep(FOLLOW_POLL);
        let events = store::read_events(&path);
        if events.len() > seen {
            print(&events[seen..])?;
            seen = events.len();
        }
    }
}

fn cmd_install_service(root: &Path) -> Result<()> {
    let path = zsf::daemon::install_service(root)?;
    println!("wrote {}", path.display());
    println!(
        "`zsf start NAME` now runs the daemon as zsf@NAME.service (enabled, so it comes back after a reboot)."
    );
    println!("to keep it running while you are logged out: loginctl enable-linger $USER");
    println!("reload systemd once: systemctl --user daemon-reload");
    Ok(())
}

fn abs(path: PathBuf) -> PathBuf {
    std::path::absolute(&path).unwrap_or(path)
}

/// `--deps 03, 04` → `["03", "04"]` (`""` for none).
fn parse_ids(list: &str) -> Vec<String> {
    list.split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The named feature, or the only one there is.
fn open(root: &Path, name: Option<String>) -> Result<Feature> {
    match name {
        Some(name) => Feature::open(root, &name),
        None => {
            let names = store::list_features(root);
            match names.as_slice() {
                [one] => Feature::open(root, one),
                [] => bail!("no features in {} (zsf init)", root.display()),
                _ => bail!("several features ({}); name one", names.join(", ")),
            }
        }
    }
}

/// A slice command: `action` for the target's slice, submitted to its feature.
fn mutate(root: &Path, target: &SliceTarget, action: impl FnOnce(String) -> Action) -> Result<()> {
    let (feature, slice) = target.open(root)?;
    answer(zsf::daemon::submit(&feature, action(slice), REQUEST_TIMEOUT).context("cannot submit")?)
}

/// A feature command: `action` submitted to the feature.
fn submit(feature: &Feature, action: Action) -> Result<()> {
    answer(zsf::daemon::submit(feature, action, REQUEST_TIMEOUT)?)
}

fn answer(result: RequestResult) -> Result<()> {
    if result.ok {
        println!("{}", result.message);
        Ok(())
    } else {
        bail!("{}", result.message)
    }
}
