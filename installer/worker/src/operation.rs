#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerOperation {
    Installer,
    Component,
}

impl WorkerOperation {
    pub(crate) fn pipe_prefix(self) -> &'static str {
        match self {
            Self::Installer => r"\\.\pipe\MediaDrop.Installer.v1.",
            Self::Component => r"\\.\pipe\MediaDrop.Component.v1.",
        }
    }

    pub(crate) fn elevated_mode(self) -> &'static str {
        match self {
            Self::Installer => "--elevated-worker",
            Self::Component => "--component-elevated-worker",
        }
    }
}
