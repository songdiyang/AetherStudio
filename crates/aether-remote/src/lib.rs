pub mod container;
pub mod git;
pub mod remote_fs;
pub mod ssh;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use container::{ContainerBackend, ContainerConfig, ContainerRemoteFs};
pub use git::{
    setup_ssh_credentials, GitCommit, GitError, GitRepoConfig, GitRepoType, GitRepository,
    GitStatus,
};
pub use remote_fs::{FsEvent, GitRemoteInfo, GitSshRepo, RemoteDirEntry, RemoteFs, Result};
pub use ssh::{SshAuth, SshConfig, SshRemoteFs};
pub use workspace::RemoteWorkspace;
