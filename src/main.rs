use std::fs::{self, File};
use std::io;
use std::io::prelude::*;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::vec::Vec;

use serde_derive::Deserialize;
use structopt::StructOpt;


#[derive(Deserialize, Debug)]
struct Config {
    repo_dir: PathBuf,
}

fn read_config() -> Result<Config, String> {
    let mut path = dirs::config_dir().ok_or("Failed to obtain the user's config directory")?;
    path.push(concat!(env!("CARGO_PKG_NAME"), ".toml"));
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    let mut config: Config = toml::from_str(&buf).map_err(|e| e.to_string())?;
    config.repo_dir = expand_tilde(config.repo_dir).unwrap();
    Ok(config)
}

fn expand_tilde<P: AsRef<Path>>(path: P) -> Option<PathBuf> {
    let path = path.as_ref();
    if !path.starts_with("~") {
        Some(path.to_path_buf())
    }
    else {
        if path == Path::new("~") {
            dirs::home_dir()
        }
        else {
            let stripped = path.strip_prefix("~").unwrap();
            dirs::home_dir().map(|mut home_dir| {
                home_dir.push(stripped);
                home_dir
            })
        }
    }
}

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(StructOpt, Debug)]
enum Command {
    #[structopt(name = "add")]
    Add {
        #[structopt(name = "FILE", parse(from_os_str))]
        files: Vec<PathBuf>,
    },
}

fn add(files: Vec<PathBuf>, config: Config) -> Result<(), String> {
    let mut failed = Vec::new();
    for fp in files {
        match file_type(&fp).map_err(|e| e.to_string())? {
            FileType::Dir => {
                failed.push((fp.clone(), "file is a directory, which cannot be added"));
            },
            FileType::Symlink => {
                failed.push((fp.clone(), "file is a symlink, which cannot be added"));
            },
            FileType::File => (),
        }

        let fp = fs::canonicalize(fp).map_err(|e| e.to_string())?;
        let from = fp.as_path();
        let to = config.repo_dir.join(from.file_name().unwrap());
        fs::create_dir_all(&config.repo_dir).unwrap();
        fs::rename(&from, &to);

        let src = relative_path_from(&fp.parent().unwrap(), &to)?;
        let dst = fp.as_path();
        symlink(src, dst);
    }

    if failed.len() > 0 {
        eprintln!("The following paths are ignored:");
        for (fp, reason) in failed {
            eprintln!("{}\t({})", fp.display(), reason);
        }
    }

    Ok(())
}

#[derive(Eq, PartialEq, Debug)]
enum FileType {
    Dir,
    File,
    Symlink,
}

fn file_type<P: AsRef<Path>>(path: P) -> io::Result<FileType> {
    let path = path.as_ref();
    let metadata = path.symlink_metadata()?;
    if metadata.file_type().is_dir() {
        Ok(FileType::Dir)
    }
    else if metadata.file_type().is_file() {
        Ok(FileType::File)
    }
    else if metadata.file_type().is_symlink() {
        Ok(FileType::Symlink)
    }
    else {
        unreachable!()
    }
}

fn relative_path_from<P: AsRef<Path>, Q: AsRef<Path>>(base: P, target: Q) -> Result<PathBuf, String> {
    let mut base = fs::canonicalize(base).map_err(|e| e.to_string())?;
    let mut target = fs::canonicalize(target).map_err(|e| e.to_string())?;

    let mut count = 0;
    while !target.starts_with(&base) {
        if base.pop() {
            count += 1;
        }
        else {
            return Err("base cannot be a prefix of target".into());
        }
    }

    let mut relpath = PathBuf::new();
    for _ in 0..count {
        relpath.push("..");
    }
    Ok(relpath.join(target.strip_prefix(base).unwrap()))
}

fn to_absolute<P: AsRef<Path>>(path: P) -> Result<PathBuf, String> {
    let path = path.as_ref();
    if path.is_absolute() {
        Ok(path.to_path_buf())
    }
    else {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        Ok(cwd.join(path))
    }
}

fn main() {
    let opt = Opt::from_args();
    let config = read_config().unwrap();

    match opt.cmd {
        Command::Add { files } => {
            add(files, config).unwrap();
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        std::env::set_var("HOME", "/home/alice");
        assert_eq!(expand_tilde("~"), Some("/home/alice".into()));
        assert_eq!(expand_tilde("~/"), Some("/home/alice/".into()));
        assert_eq!(expand_tilde("~/foo"), Some("/home/alice/foo".into()));
        assert_eq!(expand_tilde("/foo/bar"), Some("/foo/bar".into()));
        assert_eq!(expand_tilde("~bob/foo/bar"), Some("~bob/foo/bar".into()));

        std::env::set_var("HOME", "/");
        assert_eq!(expand_tilde("~"), Some("/".into()));
        assert_eq!(expand_tilde("~/"), Some("/".into()));
        assert_eq!(expand_tilde("~/foo"), Some("/foo".into()));
        assert_eq!(expand_tilde("/foo/bar"), Some("/foo/bar".into()));
        assert_eq!(expand_tilde("~bob/foo/bar"), Some("~bob/foo/bar".into()));
    }

    #[test]
    fn test_to_absolute() {
        std::env::set_current_dir("/usr");
        assert_eq!(to_absolute("foo/bar"), Ok("/usr/foo/bar".into()));
        assert_eq!(to_absolute("/"), Ok("/".into()));
        assert_eq!(to_absolute("/foo/bar"), Ok("/foo/bar".into()));

        std::env::set_current_dir("/");
        assert_eq!(to_absolute("foo/bar"), Ok("/foo/bar".into()));
        assert_eq!(to_absolute("/"), Ok("/".into()));
        assert_eq!(to_absolute("/foo/bar"), Ok("/foo/bar".into()));
    }

    #[test]
    fn test_relative_path_from() {
        assert_eq!(relative_path_from("/usr", "/usr/share"), Ok("share".into()));
        assert_eq!(relative_path_from("/usr/", "/usr/share"), Ok("share".into()));
        assert_eq!(relative_path_from("/usr/bin", "/usr/share"), Ok("../share".into()));
    }

    #[test]
    fn test_file_type() {
        assert_eq!(file_type("/").map_err(|e| e.to_string()), Ok(FileType::Dir));
        assert_eq!(file_type("/bin/echo").map_err(|e| e.to_string()), Ok(FileType::File));
    }
}
