//! Opening the browser at startup, only where one can open: an interactive terminal with a
//! display, not over SSH or in CI. Otherwise zerochat says in one line why it did not.

use std::io::IsTerminal;

/// What the environment says about opening a browser (read once from the process; tests build
/// their own).
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub no_open: bool,
    pub stdout_tty: bool,
    /// `$BROWSER`.
    pub browser: Option<String>,
    pub ci: bool,
    pub ssh: bool,
    pub os: Os,
    /// `$DISPLAY` or `$WAYLAND_DISPLAY` is set.
    pub display: bool,
    /// Running under WSL (`$WSL_DISTRO_NAME`).
    pub wsl: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Os {
    #[default]
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Run this program with these arguments, then the URL.
    Open {
        program: &'static str,
        args: &'static [&'static str],
    },
    /// Do not open; the reason, shown to the user.
    Skip(&'static str),
}

impl Env {
    pub fn from_process(no_open: bool) -> Env {
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let ci = std::env::var("CI")
            .is_ok_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"));
        Env {
            no_open,
            stdout_tty: std::io::stdout().is_terminal(),
            browser: std::env::var("BROWSER").ok(),
            ci,
            ssh: set("SSH_CONNECTION") || set("SSH_TTY"),
            os: if cfg!(target_os = "macos") {
                Os::Macos
            } else if cfg!(windows) {
                Os::Windows
            } else {
                Os::Linux
            },
            display: set("DISPLAY") || set("WAYLAND_DISPLAY"),
            wsl: set("WSL_DISTRO_NAME"),
        }
    }
}

pub fn decide(env: &Env) -> Decision {
    if env.no_open {
        return Decision::Skip("--no-open");
    }
    if env
        .browser
        .as_deref()
        .is_some_and(|b| b.trim().eq_ignore_ascii_case("none"))
    {
        return Decision::Skip("BROWSER=none");
    }
    if env.ci {
        return Decision::Skip("CI is set");
    }
    if env.ssh {
        return Decision::Skip("SSH session");
    }
    if !env.stdout_tty {
        return Decision::Skip("not an interactive terminal");
    }
    match env.os {
        Os::Macos => Decision::Open {
            program: "open",
            args: &[],
        },
        // `start` is a cmd built-in; its first quoted argument is a window title.
        Os::Windows => Decision::Open {
            program: "cmd",
            args: &["/c", "start", ""],
        },
        // Under WSL, xdg-open has no Linux browser to hand the URL to; wslview opens it in the
        // Windows browser.
        Os::Linux if env.wsl => Decision::Open {
            program: "wslview",
            args: &[],
        },
        Os::Linux if !env.display => Decision::Skip("no display"),
        Os::Linux => Decision::Open {
            program: "xdg-open",
            args: &[],
        },
    }
}

/// The banner's note on the browser, e.g. `(opening your browser; --no-open to skip)`.
pub fn note(decision: &Decision) -> String {
    match decision {
        Decision::Open { .. } => "(opening your browser; --no-open to skip)".to_owned(),
        Decision::Skip(why) => format!("(not opening a browser: {why})"),
    }
}

/// Opens `url` when `decision` says so; a launcher that cannot start is reported in one line.
pub fn open(decision: &Decision, url: &str) {
    let Decision::Open { program, args } = decision else {
        return;
    };
    let started = std::process::Command::new(program)
        .args(*args)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(error) = started {
        println!("  could not open a browser ({program}: {error}); open {url}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop() -> Env {
        Env {
            stdout_tty: true,
            display: true,
            ..Env::default()
        }
    }

    #[test]
    fn opens_on_an_interactive_desktop() {
        assert_eq!(
            decide(&desktop()),
            Decision::Open {
                program: "xdg-open",
                args: &[]
            }
        );
        assert_eq!(
            decide(&Env {
                os: Os::Macos,
                display: false,
                ..desktop()
            }),
            Decision::Open {
                program: "open",
                args: &[]
            }
        );
        assert_eq!(
            decide(&Env {
                os: Os::Windows,
                display: false,
                ..desktop()
            }),
            Decision::Open {
                program: "cmd",
                args: &["/c", "start", ""]
            }
        );
        assert_eq!(
            decide(&Env {
                wsl: true,
                display: false,
                ..desktop()
            }),
            Decision::Open {
                program: "wslview",
                args: &[]
            }
        );
        assert_eq!(
            decide(&Env {
                browser: Some("firefox".into()),
                ..desktop()
            }),
            Decision::Open {
                program: "xdg-open",
                args: &[]
            }
        );
    }

    #[test]
    fn stays_closed_and_says_why() {
        let cases: [(Env, &str); 7] = [
            (
                Env {
                    no_open: true,
                    ..desktop()
                },
                "--no-open",
            ),
            (
                Env {
                    browser: Some("none".into()),
                    ..desktop()
                },
                "BROWSER=none",
            ),
            (
                Env {
                    ci: true,
                    ..desktop()
                },
                "CI is set",
            ),
            (
                Env {
                    ssh: true,
                    ..desktop()
                },
                "SSH session",
            ),
            (
                Env {
                    stdout_tty: false,
                    ..desktop()
                },
                "not an interactive terminal",
            ),
            (
                Env {
                    display: false,
                    ..desktop()
                },
                "no display",
            ),
            // The first reason wins: --no-open over everything else.
            (
                Env {
                    no_open: true,
                    ssh: true,
                    ci: true,
                    stdout_tty: false,
                    ..desktop()
                },
                "--no-open",
            ),
        ];
        for (env, why) in cases {
            assert_eq!(decide(&env), Decision::Skip(why), "{env:?}");
        }
        assert_eq!(
            note(&Decision::Skip("SSH session")),
            "(not opening a browser: SSH session)"
        );
        assert_eq!(
            note(&decide(&desktop())),
            "(opening your browser; --no-open to skip)"
        );
    }

    #[test]
    fn ssh_and_ci_stop_it_on_every_os() {
        for os in [Os::Linux, Os::Macos, Os::Windows] {
            assert_eq!(
                decide(&Env {
                    os,
                    ssh: true,
                    ..desktop()
                }),
                Decision::Skip("SSH session")
            );
            assert_eq!(
                decide(&Env {
                    os,
                    ci: true,
                    ..desktop()
                }),
                Decision::Skip("CI is set")
            );
            assert_eq!(
                decide(&Env {
                    os,
                    stdout_tty: false,
                    ..desktop()
                }),
                Decision::Skip("not an interactive terminal")
            );
        }
    }
}
