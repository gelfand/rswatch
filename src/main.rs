use anyhow::Result;
use crossbeam::channel;
use crossbeam::channel::select;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
use structopt::StructOpt;

/// rswatch is a file watcher that can be used to run arbitrary commands when a file changes.
#[derive(StructOpt, Debug)]
struct Cli {
    /// path is the path to the file or directory to watch.
    path: std::path::PathBuf,
    /// cmd is the command to run when a file changes.
    cmd: Vec<String>,
}

/// read_metadata reads metadata of all files recursively in the given path.
fn read_metadata(path: &std::path::PathBuf) -> HashMap<std::path::PathBuf, std::fs::Metadata> {
    let mut entries: HashMap<std::path::PathBuf, std::fs::Metadata> = HashMap::new();

    let mdata = std::fs::metadata(path).unwrap();
    if mdata.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_dir() {
                entries.extend(read_metadata(&entry.path()));
            } else {
                entries.insert(entry.path(), std::fs::metadata(entry.path()).unwrap());
            }
        }
    } else {
        entries.insert(path.to_path_buf(), mdata);
    }
    entries
}

/// ctrl_channel creates a channel for the Ctrl+C messages.
fn ctrl_channel() -> Result<channel::Receiver<()>, ctrlc::Error> {
    let (sender, receiver) = channel::bounded(100);
    ctrlc::set_handler(move || {
        let _ = sender.send(());
    })?;

    Ok(receiver)
}

/// build_cmd builds the command to run.
fn build_command(raw_cmd: Vec<String>) -> Result<Command, anyhow::Error> {
    if raw_cmd.len() == 0 {
        return Err(anyhow::anyhow!("No command specified"));
    }

    let mut cmd = Command::new(&raw_cmd[0]);
    for i in 1..raw_cmd.len() {
        cmd.arg(&raw_cmd[i]);
    }

    Ok(cmd)
}

fn main() -> Result<()> {
    let args = Cli::from_args();
    match std::fs::metadata(&args.path) {
        Ok(_) => (),
        Err(err) => return Err(err.into()),
    };

    let interrupt_events = ctrl_channel()?;
    let ticks = channel::tick(Duration::from_secs(1));

    let mut entries = read_metadata(&args.path);

    let mut cmd: Command;
    match build_command(args.cmd) {
        Ok(c) => cmd = c,
        Err(err) => return Err(err),
    }

    let mut child = cmd.spawn().unwrap();

    loop {
        select! {
            recv(ticks) -> _ => {
                let new_entries = read_metadata(&args.path);
                for (path, metadata) in read_metadata(&args.path).into_iter() {
                    if let Some(old_metadata) = entries.get(&path) {
                        if std::time::SystemTime::eq(&metadata.modified().unwrap(), &old_metadata.modified().unwrap()) {
                            continue
                        }
                        child.kill().unwrap();
                        child = cmd.spawn().unwrap();

                    } else {
                        child.kill().unwrap();
                        child = cmd.spawn().unwrap();
                    }
                }
                entries = new_entries;
            }
            recv(interrupt_events) -> _ => {
                println!("Interrupted");
                child.kill().unwrap();
                break
            }
        }
    }
    Ok(())
}
