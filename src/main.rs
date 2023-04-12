use clap::{Parser, Subcommand, ValueHint};
use git2::build::TreeUpdateBuilder;
use git2::{
    AutotagOption, BranchType, FileMode, ObjectType, Reference, RemoteCallbacks, Repository,
};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use which::which;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    pub version: String,
    pub dependencies: BTreeMap<String, Dependency>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub(crate) struct Dependency {
    pub url: String,
    pub heads: BTreeMap<String, Head>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub(crate) struct Head {
    commit: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: "1.1".to_string(),
            dependencies: BTreeMap::new(),
        }
    }
}

#[derive(Parser)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Run as if was started in <path>
    #[clap(short = 'C', num_args = 1, value_hint = ValueHint::DirPath)]
    pub change_dir: Option<PathBuf>,

    /// Directory where the GIT_DIR is
    #[clap(long, env = "GIT_DIR", value_hint = ValueHint::DirPath)]
    pub git_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Initializes paravendor in a repository
    Init {
        /// If no local `paravendor` branch is found, don't try to get a remote one
        #[clap(long, default_value = "false")]
        ignore_remote: bool,
    },
    /// Vendorizes a new dependency
    Add {
        /// Dependency name
        name: String,
        /// Dependency URL
        #[clap(value_hint = ValueHint::Url)]
        url: String,
    },
    /// List vendorized dependencies
    List,
    /// Shows all refs for a vendorized dependency
    ShowRefs {
        /// Dependency name
        name: String,
    },
    /// Resolves a ref in a vendorized dependency
    ShowRef {
        /// Dependency name
        name: String,
        /// Ref
        reference: String,
    },
    /// Sync vendorized dependencies
    Sync {
        /// Limit syncing to a list of dependencies
        ///
        /// If not specified, all dependencies will be synced
        names: Vec<String>,
    },
    /// Shows commits belonging to paravendor branch
    Log {
        /// Extra options for `git log`
        ///
        /// Effective if `git` is present, otherwise ignored
        options: Option<Vec<String>>,
    },
}

impl Cli {
    pub(crate) fn ensure_initialized(
        repository: &Repository,
    ) -> Result<(git2::Branch, Config), anyhow::Error> {
        repository
            .find_branch("paravendor", BranchType::Local)
            .or_else(|e| {
                if e.code() == git2::ErrorCode::NotFound {
                    if let Some(ref remote) = repository
                        // Try resolving head to branch
                        .head()
                        .ok()
                        .filter(Reference::is_branch)
                        .and_then(|r| r.name().map(|n| n.to_string()))
                        // And then the branch to a remote
                        .and_then(|branch| {
                            repository
                                .branch_upstream_remote(&branch)
                                .ok()
                                .and_then(|b| b.as_str().map(str::to_string))
                        })
                        // Otherwise, pick the first one (FIXME: is this a good idea?)
                        .or_else(|| {
                            repository
                                .remotes()
                                .ok()
                                .and_then(|arr| arr.get(0).map(str::to_string))
                        })
                    {
                        if let Ok(branch) = repository
                            .find_branch(&format!("{remote}/paravendor"), BranchType::Remote)
                        {
                            return repository.branch(
                                "paravendor",
                                &branch.get().peel_to_commit()?,
                                false,
                            );
                        }
                    }
                }
                Err(e)
            })
            .map_err(|e| {
                if e.code() == git2::ErrorCode::NotFound {
                    anyhow::Error::msg("paravendor is not initialized, run `git paravendor init`")
                } else {
                    anyhow::Error::new(e)
                }
            })
            .and_then(|branch| {
                let obj = repository.revparse_single("paravendor:config")?;
                if obj.kind() == Some(ObjectType::Blob) {
                    let config: Config =
                        toml::from_str(std::str::from_utf8(obj.as_blob().unwrap().content())?)?;
                    Ok((branch, config))
                } else {
                    Err(anyhow::Error::msg("paravendor config not found"))
                }
            })
    }

    pub(crate) fn sync_dependency<'a>(
        repository: &'a Repository,
        url: &str,
    ) -> Result<(BTreeMap<String, Head>, Vec<git2::Commit<'a>>), anyhow::Error> {
        let mut remote = repository.remote_anonymous(url)?;
        let mut cb = RemoteCallbacks::new();

        let received_objects = ProgressBar::hidden();
        received_objects.set_message("Received objects");
        received_objects.set_style(ProgressStyle::with_template(
            "{msg} {wide_bar} {pos:>7}/{len:7} (ETA {eta})",
        )?);
        let indexed_deltas = ProgressBar::hidden();
        indexed_deltas.set_message("Indexed deltas");
        indexed_deltas.set_style(ProgressStyle::with_template(
            "{msg} {wide_bar} {pos:>7}/{len:7} (ETA {eta})",
        )?);
        let multi_pb = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
        multi_pb.add(received_objects.clone());
        multi_pb.add(indexed_deltas.clone());

        cb.transfer_progress(move |p| {
            if received_objects.is_hidden() {
                received_objects.set_draw_target(ProgressDrawTarget::stderr());
                indexed_deltas.set_draw_target(ProgressDrawTarget::stderr());
            }
            received_objects.set_length(p.total_objects() as u64);
            received_objects.set_position(p.received_objects() as u64);
            if p.total_objects() == p.received_objects() {
                received_objects.finish_and_clear();
            }

            indexed_deltas.set_length(p.total_deltas() as u64);
            indexed_deltas.set_position(p.indexed_deltas() as u64);

            if p.total_deltas() == p.indexed_deltas() {
                indexed_deltas.finish_and_clear();
            }

            true
        });
        remote.fetch::<&str>(
            &[],
            Some(
                git2::FetchOptions::new()
                    .download_tags(AutotagOption::None)
                    .remote_callbacks(cb),
            ),
            None,
        )?;

        let heads = remote
            .list()?
            .iter()
            .map(|h| {
                (
                    h.name().to_string(),
                    Head {
                        commit: h.oid().to_string(),
                    },
                )
            })
            .collect();

        let head_commits: Vec<_> = remote
            .list()?
            .iter()
            .filter_map(|h| repository.find_commit(h.oid()).ok())
            .collect();

        fn is_commit_in_history(
            repo: &Repository,
            target: &git2::Commit,
            reference: &git2::Commit,
        ) -> Result<bool, anyhow::Error> {
            let mut revwalk = repo.revwalk()?;
            revwalk.push(reference.id())?;

            for oid in revwalk {
                let oid = oid?;
                if oid == target.id() {
                    return Ok(true);
                }
            }
            Ok(false)
        }

        let pruned_head_commits: Vec<_> = head_commits
            .clone()
            .into_iter()
            .filter(|c| {
                !head_commits
                    .iter()
                    .any(|c_| c_.id() != c.id() && is_commit_in_history(repository, c, c_).unwrap())
            })
            .collect();

        Ok((heads, pruned_head_commits))
    }

    pub(crate) fn execute(mut self) -> Result<Self, anyhow::Error> {
        let option = std::env::current_dir().ok();
        let repository_path = self
            .git_dir
            .as_ref()
            .or(self.change_dir.as_ref())
            .or(option.as_ref())
            .ok_or(anyhow::Error::msg("no repository path specified"))?;
        let repository = git2::Repository::open(repository_path)?;
        match self.command {
            Command::Init { ignore_remote } => {
                match repository.find_branch("paravendor", BranchType::Local) {
                    Ok(_) => return Err(anyhow::Error::msg("'paravendor' branch already exists")),
                    Err(err) => {
                        if err.code() == git2::ErrorCode::NotFound && !ignore_remote {
                            if let Ok(branch) =
                                repository.find_branch("origin/paravendor", BranchType::Remote)
                            {
                                repository.branch(
                                    "paravendor",
                                    &branch.get().peel_to_commit()?,
                                    false,
                                )?;
                                return Ok(self);
                            }
                        }

                        let config = Config::default();
                        let serialized_config = toml::to_string_pretty(&config)?;

                        // Prepare initial commit
                        let mut tree = repository.treebuilder(None)?;
                        let odb = repository.odb()?;
                        let blob = odb.write(ObjectType::Blob, serialized_config.as_bytes())?;
                        tree.insert("config", blob, 0o100644)?;
                        let tree_oid = tree.write()?;

                        let signature = repository.signature()?;
                        let init_commit = repository.commit(
                            None,
                            &signature,
                            &signature,
                            "Initialize paravendor",
                            &repository.find_tree(tree_oid)?,
                            &[],
                        )?;

                        // Create the branch
                        repository.branch(
                            "paravendor",
                            &repository.find_commit(init_commit)?,
                            false,
                        )?;
                    }
                }
            }
            Command::Add { ref name, ref url } => {
                let (branch, mut config) = Self::ensure_initialized(&repository)?;
                if config.dependencies.get(name).is_some() {
                    return Err(anyhow::Error::msg(format!(
                        "{name} has been already added, aborting"
                    )));
                }

                let (heads, mut pruned_head_commits) = Self::sync_dependency(&repository, url)?;

                config.dependencies.insert(
                    name.clone(),
                    Dependency {
                        url: url.clone(),
                        heads,
                    },
                );

                let serialized_config = toml::to_string_pretty(&config)?;
                let commit = branch.into_reference().peel_to_commit()?;

                let mut tree = TreeUpdateBuilder::new();
                let odb = repository.odb()?;
                let blob = odb.write(ObjectType::Blob, serialized_config.as_bytes())?;
                tree.upsert("config", blob, FileMode::Blob);
                let tree_oid = tree.create_updated(&repository, &commit.tree()?)?;

                pruned_head_commits.insert(0, commit);

                let _add_commit = repository.commit(
                    Some("refs/heads/paravendor"),
                    &repository.signature()?,
                    &repository.signature()?,
                    &format!("Add {} from {}", name, url),
                    &repository.find_tree(tree_oid)?,
                    &pruned_head_commits.iter().collect::<Vec<_>>(),
                )?;
            }
            Command::Sync { ref names } => {
                let (branch, mut config) = Self::ensure_initialized(&repository)?;
                let original_config = config.clone();

                let effective_dependencies = config
                    .dependencies
                    .iter_mut()
                    .filter(|d| names.is_empty() || names.iter().any(|n| d.0 == n))
                    .collect::<Vec<_>>();

                let mut pruned_head_commits = Vec::new();
                let mut changed_dependencies = Vec::new();
                for (name, dependency) in effective_dependencies {
                    let (heads, mut dependency_pruned_head_commits) =
                        Self::sync_dependency(&repository, &dependency.url)?;
                    let old_heads = dependency.heads.clone();
                    dependency.heads = heads;
                    pruned_head_commits.append(&mut dependency_pruned_head_commits);
                    if old_heads != dependency.heads {
                        println!("Synced {name}");
                        changed_dependencies.push(name.to_string());
                    }
                }

                if original_config == config {
                    eprintln!("No updates detected");
                } else {
                    let serialized_config = toml::to_string_pretty(&config)?;

                    let commit = branch.into_reference().peel_to_commit()?;

                    let mut tree = TreeUpdateBuilder::new();
                    let odb = repository.odb()?;
                    let blob = odb.write(ObjectType::Blob, serialized_config.as_bytes())?;
                    tree.upsert("config", blob, FileMode::Blob);
                    let tree_oid = tree.create_updated(&repository, &commit.tree()?)?;

                    pruned_head_commits.insert(0, commit);

                    let _sync_commit = repository.commit(
                        Some("refs/heads/paravendor"),
                        &repository.signature()?,
                        &repository.signature()?,
                        &format!("Sync: {}", changed_dependencies.join(", ")),
                        &repository.find_tree(tree_oid)?,
                        &pruned_head_commits.iter().collect::<Vec<_>>(),
                    )?;
                }
            }
            Command::List => {
                let (_branch, config) = Self::ensure_initialized(&repository)?;

                for (name, details) in &config.dependencies {
                    println!("{name} {}", details.url);
                }
            }
            Command::ShowRefs { ref name } => {
                let (_branch, config) = Self::ensure_initialized(&repository)?;

                match config.dependencies.get(name) {
                    None => return Err(anyhow::Error::msg("dependency not found")),
                    Some(dependency) => {
                        for name in dependency.heads.keys() {
                            println!("{name}");
                        }
                    }
                }
            }
            Command::ShowRef {
                ref name,
                ref reference,
            } => {
                let (_branch, config) = Self::ensure_initialized(&repository)?;

                match config.dependencies.get(name) {
                    None => return Err(anyhow::Error::msg("dependency not found")),
                    Some(dependency) => {
                        match dependency
                            .heads
                            .get(reference)
                            .or_else(|| dependency.heads.get(&format!("refs/heads/{reference}")))
                            .or_else(|| {
                                dependency.heads.get(&format!("refs/tags/{reference}^{{}}"))
                            })
                            .or_else(|| dependency.heads.get(&format!("refs/tags/{reference}")))
                        {
                            None => return Err(anyhow::Error::msg("ref not found")),
                            Some(head) => {
                                println!("{}", head.commit);
                            }
                        }
                    }
                }
            }
            Command::Log { ref mut options } => {
                let (branch, _config) = Self::ensure_initialized(&repository)?;

                // If possible, try doing this with git as it makes a better output
                match which("git") {
                    Err(which::Error::CannotFindBinaryPath) => {}
                    Err(e) => return Err(e)?,
                    Ok(git) => {
                        let mut args = vec!["log".to_string()];
                        args.append(options.as_mut().unwrap_or(&mut vec![]));
                        args.append(&mut vec![
                            "paravendor".to_string(),
                            "--first-parent".to_string(),
                            "-C".to_string(),
                            repository.workdir().unwrap().to_string_lossy().to_string(),
                        ]);
                        std::process::Command::new(git).args(args).spawn()?.wait()?;
                        return Ok(self);
                    }
                };

                // Otherwise, do it ourselves
                let mut top = branch.into_reference().peel_to_commit()?;
                loop {
                    println!(
                        "{} {}",
                        top.id(),
                        top.message().unwrap_or("").lines().next().unwrap_or("")
                    );
                    if let Some(parent) = top.parents().next() {
                        top = parent;
                    } else {
                        break;
                    }
                }
            }
        }
        Ok(self)
    }
}

fn main() -> Result<(), anyhow::Error> {
    Cli::parse().execute()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::{Deref, DerefMut};
    use std::path::Path;
    use std::process::{ExitCode, Termination};
    use tempfile::*;

    struct TempRepository {
        repository: Repository,
        dir: TempDir,
        dependencies: BTreeMap<String, TempRepository>,
    }

    impl TempRepository {
        fn new() -> Result<Self, anyhow::Error> {
            let dir = tempdir()?;
            let repository = Repository::init(dir.as_ref())?;
            Ok(Self {
                repository,
                dir,
                dependencies: BTreeMap::new(),
            })
        }

        fn depends_on(&mut self, name: &str, repository: TempRepository) {
            self.dependencies.insert(name.to_string(), repository);
        }

        fn get_dependency(&self, name: &str) -> Option<&TempRepository> {
            self.dependencies.get(name)
        }

        fn get_mut_dependency(&mut self, name: &str) -> Option<&mut TempRepository> {
            self.dependencies.get_mut(name)
        }
    }

    impl Deref for TempRepository {
        type Target = Repository;

        fn deref(&self) -> &Self::Target {
            &self.repository
        }
    }

    impl DerefMut for TempRepository {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.repository
        }
    }

    impl Termination for TempRepository {
        fn report(self) -> ExitCode {
            ExitCode::SUCCESS
        }
    }

    #[test]
    fn init_clean() -> Result<TempRepository, anyhow::Error> {
        let repo = TempRepository::new()?;
        {
            assert!(repo.find_branch("paravendor", BranchType::Local).is_err());

            let cli = Cli {
                command: Command::Init {
                    ignore_remote: false,
                },
                change_dir: Some(repo.dir.as_ref().to_path_buf()),
                git_dir: None,
            };
            cli.execute()?;
            let (_branch, config) = Cli::ensure_initialized(&repo)?;
            assert_eq!(config.version, "1.1");
        }
        Ok(repo)
    }

    fn demo_repo_with_one_commit() -> Result<TempRepository, anyhow::Error> {
        let repo = TempRepository::new()?;
        let sig = git2::Signature::new("John Doe", "john@doe.com", &git2::Time::new(0, 0))?;

        // Prepare initial commit
        let tree_oid = repo.treebuilder(None)?.write()?;

        let _commit = repo.commit(
            Some("refs/heads/master"),
            &sig,
            &sig,
            "init",
            &repo.find_tree(tree_oid)?,
            &[],
        )?;
        Ok(repo)
    }

    fn add_dependency_to_repo(
        mut repo: TempRepository,
        name: &str,
    ) -> Result<TempRepository, anyhow::Error> {
        let dep = demo_repo_with_one_commit()?;
        let dep_repo_commit = dep.head()?.peel_to_commit()?.id();

        {
            let init_commit = dep.head()?.peel_to_commit()?;
            let cli = Cli {
                change_dir: Some(repo.dir.as_ref().to_path_buf()),
                git_dir: None,
                command: Command::Add {
                    name: name.to_string(),
                    url: dep.dir.as_ref().to_string_lossy().to_string(),
                },
            };
            let _cli = cli.execute()?;
            let (branch, config) = Cli::ensure_initialized(&repo)?;

            let dep = config.dependencies.get(name).unwrap();
            for head_name in ["HEAD", "refs/heads/master"] {
                let head = dep.heads.get(head_name).unwrap();
                assert_eq!(head.commit, dep_repo_commit.to_string());

                let commit = branch.get().peel_to_commit()?;
                assert!(commit.parents().any(|p| p.id() == dep_repo_commit));
                assert!(commit.parents().any(|p| p.id() == init_commit.id()));
            }
        }

        repo.depends_on(name, dep);

        Ok(repo)
    }

    #[test]
    fn add() -> Result<TempRepository, anyhow::Error> {
        add_dependency_to_repo(init_clean()?, "dep")
    }

    #[test]
    fn sync_no_changes() -> Result<(), anyhow::Error> {
        let repo = add()?;

        let (original_branch, _config) = Cli::ensure_initialized(&repo)?;

        let cli = Cli {
            command: Command::Sync { names: vec![] },
            change_dir: repo.workdir().map(Path::to_path_buf),
            git_dir: None,
        };
        let _ = cli.execute()?;

        let (branch, _config) = Cli::ensure_initialized(&repo)?;

        assert_eq!(
            branch.get().peel_to_commit()?.id(),
            original_branch.get().peel_to_commit()?.id()
        );

        Ok(())
    }

    fn repo_with_changed_dependency(
        name: &str,
        mut repo: TempRepository,
    ) -> Result<TempRepository, anyhow::Error> {
        {
            let dep = repo
                .get_mut_dependency(name)
                .ok_or_else(|| anyhow::Error::msg(format!("{name} dependency not found")))?;

            let tree = dep.repository.treebuilder(None)?.write()?;
            let tree = dep.find_tree(tree)?;

            let sig = git2::Signature::new("John Doe", "john@doe.com", &git2::Time::new(0, 0))?;
            // Prepare a commit
            let _commit = dep.commit(
                Some("refs/heads/master"),
                &sig,
                &sig,
                "update",
                &tree,
                &[&dep.head()?.peel_to_commit()?],
            )?;
        }
        Ok(repo)
    }

    #[test]
    fn sync_singular_dependency_change() -> Result<(), anyhow::Error> {
        for names in [vec![], vec!["dep".to_string()]] {
            let repo = add()?;
            let original_branch_commit = {
                let (original_branch, _config) = Cli::ensure_initialized(&repo)?;
                dbg!(&_config);
                original_branch.into_reference().peel_to_commit()?.id()
            };

            let repo = repo_with_changed_dependency("dep", repo)?;

            let cli = Cli {
                // don't specify dependency name
                command: Command::Sync { names },
                change_dir: repo.workdir().map(Path::to_path_buf),
                git_dir: None,
            };
            let _ = cli.execute()?;

            let (branch, config) = Cli::ensure_initialized(&repo)?;

            let dep_last_commit = repo
                .get_dependency("dep")
                .unwrap()
                .head()?
                .peel_to_commit()?;
            // config is pointing to the updated dependency
            dbg!(&config);
            assert_eq!(
                dep_last_commit.id().to_string(),
                config
                    .dependencies
                    .get("dep")
                    .unwrap()
                    .heads
                    .get("refs/heads/master")
                    .unwrap()
                    .commit
            );
            // paravendor branch has been updated to include the dependency
            assert_eq!(
                1,
                branch
                    .get()
                    .peel_to_commit()?
                    .parents()
                    .filter(|p| p.id() == original_branch_commit)
                    .count()
            );
        }
        Ok(())
    }
}
