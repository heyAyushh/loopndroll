#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TuiTab {
    Dashboard,
    Sessions,
    Connections,
    Settings,
    Logs,
}

impl TuiTab {
    pub(crate) const ALL: [Self; 5] = [
        Self::Dashboard,
        Self::Sessions,
        Self::Connections,
        Self::Settings,
        Self::Logs,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Sessions => "Sessions",
            Self::Connections => "Connections",
            Self::Settings => "Settings",
            Self::Logs => "Logs",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Sessions => 1,
            Self::Connections => 2,
            Self::Settings => 3,
            Self::Logs => 4,
        }
    }

    pub(crate) fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Dashboard)
    }

    pub(crate) fn next(self) -> Self {
        let next_index = (self.index() + 1) % Self::ALL.len();
        Self::from_index(next_index)
    }

    pub(crate) fn previous(self) -> Self {
        let previous_index = if self.index() == 0 {
            Self::ALL.len() - 1
        } else {
            self.index() - 1
        };
        Self::from_index(previous_index)
    }
}
