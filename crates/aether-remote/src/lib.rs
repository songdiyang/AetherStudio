pub mod container;
pub mod git;
pub mod remote_fs;
pub mod ssh;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use container::{ContainerBackend, ContainerConfig, ContainerRemoteFs};
pub use git::{
    git_available, GitCommit, GitError, GitRepoConfig, GitRepoType, GitRepository, GitStatus,
    GIT_DOWNLOAD_URL,
};
pub use remote_fs::{FsEvent, GitRemoteInfo, GitSshRepo, RemoteDirEntry, RemoteFs, Result};
pub use ssh::{ssh_available, SshAuth, SshConfig, SshRemoteFs, SSH_DOWNLOAD_URL};
pub use workspace::RemoteWorkspace;
