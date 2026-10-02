//! `waystow`: a transient file shelf and action surface for Wayland.

// GTK futures run on the main thread and hold `Rc`s by design.
#![allow(clippy::future_not_send)]

mod daemon;
mod doctor;
mod files;
mod ipc;
mod recent;
mod system;
mod ui;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use waystow_core::ipc::{Request, Response};
use waystow_core::model::{Content, JobStatus, RecentKind};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon (normally via the systemd user service).
    Daemon,
    /// Open the shelf, or close it if open. Bind this to a key in your compositor.
    Toggle,
    /// Open the shelf.
    Show,
    /// Close the shelf.
    Hide,
    /// Put files on the shelf (or text with --text, or the clipboard with --clipboard).
    Add {
        /// Files or folders.
        paths: Vec<PathBuf>,
        /// Add this text (a snippet, or a web address) instead; `-` reads stdin.
        #[arg(long, conflicts_with_all = ["paths", "clipboard"])]
        text: Option<String>,
        /// Add the current clipboard instead.
        #[arg(long, conflicts_with = "paths")]
        clipboard: bool,
        /// Collection name (default: Temporary).
        #[arg(long, short)]
        collection: Option<String>,
    },
    /// List shelf items.
    List {
        /// Only this collection.
        #[arg(long, short)]
        collection: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Take items off the shelf (the files themselves are kept).
    Remove {
        /// Item ids (from `list`).
        ids: Vec<i64>,
    },
    /// Empty a collection (default: Temporary), keeping pinned items.
    Clear {
        /// Collection name.
        #[arg(long, short)]
        collection: Option<String>,
    },
    /// Pin an item so it never expires (or `--off` to unpin).
    Pin {
        /// Item id.
        id: i64,
        /// Unpin instead.
        #[arg(long)]
        off: bool,
    },
    /// Manage collections.
    #[command(subcommand)]
    Collection(CollectionCmd),
    /// List the actions that apply to items, or run one.
    Run {
        /// Action id; omit to list the applicable actions.
        action: Option<String>,
        /// Item ids.
        #[arg(long = "item", short)]
        items: Vec<i64>,
        /// Files not on the shelf.
        #[arg(long = "file", short)]
        files: Vec<PathBuf>,
        /// Param values, `name=value`.
        #[arg(long = "set", short)]
        params: Vec<String>,
        /// Wait for the jobs and print their outputs.
        #[arg(long)]
        wait: bool,
    },
    /// Show recent jobs.
    Jobs {
        /// How many.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Cancel a job.
    Cancel {
        /// Job id.
        job: i64,
    },
    /// Undo a finished job (moves its outputs to the Trash).
    Undo {
        /// Job id.
        job: i64,
    },
    /// Show recent files.
    Recent {
        /// downloaded, screenshot, copied, opened, created or modified.
        #[arg(long)]
        kind: Option<String>,
        /// How many.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print daemon status.
    Status,
    /// Re-read the config and action files.
    Reload,
    /// Stop the daemon.
    Quit,
    /// Check the environment: protocols, tools, config.
    Doctor,
}

#[derive(Subcommand)]
enum CollectionCmd {
    /// List collections.
    List,
    /// Create a collection.
    New {
        /// Name.
        name: String,
        /// An emoji or symbol for its chip.
        #[arg(long)]
        emoji: Option<String>,
    },
    /// Delete a collection and its items (the files are kept).
    Delete {
        /// Name.
        name: String,
    },
    /// Move items into a collection.
    Move {
        /// Collection name.
        name: String,
        /// Item ids.
        ids: Vec<i64>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let request = match cli.command {
        Command::Daemon => return daemon::run(),
        Command::Doctor => return doctor::run(),
        Command::Toggle => Request::Toggle,
        Command::Show => Request::Show,
        Command::Hide => Request::Hide,
        Command::Add { paths, text, clipboard, collection } => {
            if clipboard {
                Request::AddClipboard { collection }
            } else if let Some(text) = text {
                let text = if text == "-" {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s).context("reading stdin")?;
                    s
                } else {
                    text
                };
                Request::AddText { text, collection }
            } else {
                if paths.is_empty() {
                    bail!("nothing to add: give files, --text or --clipboard");
                }
                Request::Add { paths: absolute(paths)?, collection }
            }
        }
        Command::List { collection, json } => {
            return print_items(&send(&Request::List { collection })?, json);
        }
        Command::Remove { ids } => Request::Remove { ids },
        Command::Clear { collection } => Request::Clear { collection },
        Command::Pin { id, off } => Request::Pin { id, pinned: !off },
        Command::Collection(c) => match c {
            CollectionCmd::List => {
                if let Response::Items { collections, .. } = send(&Request::List { collection: None })? {
                    for c in collections {
                        println!("{}\t{}", c.name, c.emoji);
                    }
                }
                return Ok(());
            }
            CollectionCmd::New { name, emoji } => Request::NewCollection { name, emoji },
            CollectionCmd::Delete { name } => Request::DeleteCollection { name },
            CollectionCmd::Move { name, ids } => Request::Move { ids, collection: name },
        },
        Command::Run { action, items, files, params, wait } => {
            let Some(action) = action else {
                if let Response::Actions { actions } = expect(send(&Request::Actions { ids: items })?)? {
                    for a in actions {
                        println!("{}\t{}{}", a.id, a.title, if a.primary { "" } else { "  (more)" });
                    }
                }
                return Ok(());
            };
            let params = parse_params(&params)?;
            let resp = expect(send(&Request::Run { action, ids: items, paths: absolute(files)?, params })?)?;
            let Response::Started { jobs } = resp else { return Ok(()) };
            if wait {
                return wait_for(&jobs);
            }
            for j in jobs {
                println!("job {j}");
            }
            return Ok(());
        }
        Command::Jobs { limit, json } => return print_jobs(&send(&Request::Jobs { limit })?, json),
        Command::Cancel { job } => Request::Cancel { job },
        Command::Undo { job } => Request::Undo { job },
        Command::Recent { kind, limit, json } => {
            let kind =
                kind.map(|k| RecentKind::parse(&k).with_context(|| format!("unknown kind {k:?}"))).transpose()?;
            return print_recent(&send(&Request::Recent { kind, limit })?, json);
        }
        Command::Status => Request::Status,
        Command::Reload => Request::Reload,
        Command::Quit => Request::Quit,
    };
    match expect(send(&request)?)? {
        Response::Added { ids } => {
            for id in ids {
                println!("{id}");
            }
        }
        Response::Status(s) => {
            println!("waystow {}", s.version);
            println!("panel: {}", s.panel);
            println!("items: {}", s.items);
            println!("jobs running: {}", s.jobs);
            println!("monitors: {}", s.monitors);
            println!("actions: {} of {} available (see `waystow doctor`)", s.actions.0, s.actions.1);
        }
        _ => {}
    }
    Ok(())
}

fn send(req: &Request) -> anyhow::Result<Response> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(ipc::send(req))
}

fn expect(r: Response) -> anyhow::Result<Response> {
    match r {
        Response::Error { message } => bail!("{message}"),
        r => Ok(r),
    }
}

fn absolute(paths: Vec<PathBuf>) -> anyhow::Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir().context("current directory")?;
    paths
        .into_iter()
        .map(|p| {
            let p = if p.is_absolute() { p } else { cwd.join(p) };
            if !p.exists() {
                bail!("{} doesn't exist", p.display());
            }
            Ok(p)
        })
        .collect()
}

fn parse_params(raw: &[String]) -> anyhow::Result<BTreeMap<String, String>> {
    raw.iter()
        .map(|kv| {
            let (k, v) = kv.split_once('=').with_context(|| format!("--set takes name=value, not {kv:?}"))?;
            Ok((k.trim().to_owned(), v.to_owned()))
        })
        .collect()
}

fn wait_for(jobs: &[i64]) -> anyhow::Result<()> {
    let mut failed = false;
    loop {
        let Response::Jobs { jobs: all } = expect(send(&Request::Jobs { limit: 200 })?)? else { bail!("bad reply") };
        let mine: Vec<_> = all.iter().filter(|j| jobs.contains(&j.id)).collect();
        if mine.iter().all(|j| j.status.is_final()) {
            for j in mine {
                if j.status == JobStatus::Done {
                    for o in &j.outputs {
                        println!("{}", o.display());
                    }
                } else {
                    failed = true;
                    eprintln!("job {} {}: {}", j.id, j.status.as_str(), j.error.as_deref().unwrap_or(""));
                }
            }
            if failed {
                bail!("some jobs didn't finish");
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

fn print_items(r: &Response, json: bool) -> anyhow::Result<()> {
    let Response::Items { collections, items } = expect(r.clone())? else { bail!("bad reply") };
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    for c in &collections {
        let mine: Vec<_> = items.iter().filter(|i| i.collection == c.id).collect();
        if mine.is_empty() && c.id != waystow_core::model::TEMPORARY {
            continue;
        }
        println!("{} {}", c.emoji, c.name);
        for i in mine {
            let what = match &i.content {
                Content::File(p) | Content::Stash(p) => p.display().to_string(),
                Content::Text(_) | Content::Url(_) => i.display_name(),
            };
            println!("  {:>4}  {}{}", i.id, what, if i.pinned { "  📌" } else { "" });
        }
    }
    Ok(())
}

fn print_jobs(r: &Response, json: bool) -> anyhow::Result<()> {
    let Response::Jobs { jobs } = expect(r.clone())? else { bail!("bad reply") };
    if json {
        println!("{}", serde_json::to_string_pretty(&jobs)?);
        return Ok(());
    }
    for j in jobs {
        let outs: Vec<_> = j.outputs.iter().map(|o| o.display().to_string()).collect();
        println!("{:>4}  {:<10} {}  {}", j.id, j.status.as_str(), j.title, outs.join(", "));
        if let Some(e) = j.error {
            println!("      {e}");
        }
    }
    Ok(())
}

fn print_recent(r: &Response, json: bool) -> anyhow::Result<()> {
    let Response::Recent { recent } = expect(r.clone())? else { bail!("bad reply") };
    if json {
        println!("{}", serde_json::to_string_pretty(&recent)?);
        return Ok(());
    }
    for e in recent {
        println!("{:<10}  {}", e.kind.as_str(), e.path.display());
    }
    Ok(())
}
