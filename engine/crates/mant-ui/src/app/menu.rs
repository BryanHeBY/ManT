//! Defines the classic menu hierarchy independently from input and rendering.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuId {
    File,
    View,
    Navigate,
    Search,
    Help,
}

impl MenuId {
    pub(super) const ALL: [Self; 5] = [
        Self::File,
        Self::View,
        Self::Navigate,
        Self::Search,
        Self::Help,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::View => "View",
            Self::Navigate => "Navigate",
            Self::Search => "Search",
            Self::Help => "Help",
        }
    }

    pub(super) const fn left(self) -> u16 {
        match self {
            Self::File => 0,
            Self::View => 6,
            Self::Navigate => 12,
            Self::Search => 22,
            Self::Help => 30,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MenuEntry {
    pub(super) label: &'static str,
    pub(super) shortcut: &'static str,
    pub(super) action: MenuAction,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MenuAction {
    Quit,
    ToggleSidebar,
    ResetSidebar,
    ExpandAll,
    CollapseAll,
    Previous,
    Next,
    Parent,
    FirstChild,
    First,
    Last,
    Find,
    FindNext,
    FindPrevious,
    Help,
}

const FILE_MENU: &[MenuEntry] = &[MenuEntry {
    label: "Quit",
    shortcut: "q",
    action: MenuAction::Quit,
}];

const VIEW_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Sidebar",
        shortcut: "",
        action: MenuAction::ToggleSidebar,
    },
    MenuEntry {
        label: "Reset Sidebar Width",
        shortcut: "",
        action: MenuAction::ResetSidebar,
    },
    MenuEntry {
        label: "Expand All",
        shortcut: "",
        action: MenuAction::ExpandAll,
    },
    MenuEntry {
        label: "Collapse All",
        shortcut: "",
        action: MenuAction::CollapseAll,
    },
];

const NAVIGATE_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Previous Section",
        shortcut: "↑ / k",
        action: MenuAction::Previous,
    },
    MenuEntry {
        label: "Next Section",
        shortcut: "↓ / j",
        action: MenuAction::Next,
    },
    MenuEntry {
        label: "Parent Section",
        shortcut: "← / h",
        action: MenuAction::Parent,
    },
    MenuEntry {
        label: "First Child",
        shortcut: "→ / l",
        action: MenuAction::FirstChild,
    },
    MenuEntry {
        label: "First Section",
        shortcut: "",
        action: MenuAction::First,
    },
    MenuEntry {
        label: "Last Section",
        shortcut: "",
        action: MenuAction::Last,
    },
];

const SEARCH_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Find in Page…",
        shortcut: "Ctrl+F / /",
        action: MenuAction::Find,
    },
    MenuEntry {
        label: "Find Next",
        shortcut: "n",
        action: MenuAction::FindNext,
    },
    MenuEntry {
        label: "Find Previous",
        shortcut: "N",
        action: MenuAction::FindPrevious,
    },
];

const HELP_MENU: &[MenuEntry] = &[MenuEntry {
    label: "Keyboard Shortcuts",
    shortcut: "?",
    action: MenuAction::Help,
}];

pub(super) const fn menu_entries(id: MenuId) -> &'static [MenuEntry] {
    match id {
        MenuId::File => FILE_MENU,
        MenuId::View => VIEW_MENU,
        MenuId::Navigate => NAVIGATE_MENU,
        MenuId::Search => SEARCH_MENU,
        MenuId::Help => HELP_MENU,
    }
}
