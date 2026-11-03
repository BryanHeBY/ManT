//! Fixed menu identities, entries, shortcuts and mnemonic classification.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum MenuId {
    Manual,
    Edit,
    View,
    Navigate,
    Search,
    Help,
}

impl MenuId {
    pub(in crate::app) const ALL: [Self; 6] = [
        Self::Manual,
        Self::Edit,
        Self::View,
        Self::Navigate,
        Self::Search,
        Self::Help,
    ];

    pub(in crate::app) const fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Navigate => "Navigate",
            Self::Search => "Search",
            Self::Help => "Help",
        }
    }

    pub(in crate::app) fn from_mnemonic_key(key: KeyEvent, allow_plain: bool) -> Option<Self> {
        // Alt+Shift permits uppercase events without admitting Ctrl/AltGr or
        // unrelated modifier combinations. Plain letters are menu-local only.
        let modifiers = key.modifiers - KeyModifiers::SHIFT;
        if modifiers != KeyModifiers::ALT && !(allow_plain && modifiers.is_empty()) {
            return None;
        }
        let KeyCode::Char(character) = key.code else {
            return None;
        };
        Self::ALL
            .into_iter()
            .find(|id| id.label().starts_with(character.to_ascii_uppercase()))
    }

    pub(in crate::app) const fn left(self) -> u16 {
        match self {
            Self::Manual => 0,
            Self::Edit => 8,
            Self::View => 14,
            Self::Navigate => 20,
            Self::Search => 30,
            Self::Help => 38,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct MenuEntry {
    pub(in crate::app) label: &'static str,
    pub(in crate::app) shortcut: &'static str,
    pub(in crate::app) action: MenuAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum MenuAction {
    Quit,
    OpenDocument,
    CopySelection,
    CopyNodeText,
    CopyNodeMarkdown,
    CopyReference,
    OpenReference,
    Back,
    Forward,
    ToggleSidebar,
    ToggleFullOutlineLabels,
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
    FindBackward,
    SearchCase,
    SearchSyntax,
    SearchWrap,
    FindOutline,
    FindNext,
    FindPrevious,
    Help,
}

const MANUAL_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Open Document",
        shortcut: "Ctrl+O",
        action: MenuAction::OpenDocument,
    },
    MenuEntry {
        label: "Quit",
        shortcut: "q",
        action: MenuAction::Quit,
    },
];

const EDIT_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Copy Selection",
        shortcut: "y",
        action: MenuAction::CopySelection,
    },
    MenuEntry {
        label: "Copy Current Node as Text",
        shortcut: "",
        action: MenuAction::CopyNodeText,
    },
    MenuEntry {
        label: "Copy Current Node as Markdown",
        shortcut: "",
        action: MenuAction::CopyNodeMarkdown,
    },
    MenuEntry {
        label: "Copy Reference Target",
        shortcut: "Y",
        action: MenuAction::CopyReference,
    },
];

const VIEW_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Outline Sidebar",
        shortcut: "B",
        action: MenuAction::ToggleSidebar,
    },
    MenuEntry {
        label: "Full Outline Labels",
        shortcut: "",
        action: MenuAction::ToggleFullOutlineLabels,
    },
    MenuEntry {
        label: "Reset Outline Width",
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
        label: "Open Reference",
        shortcut: "O",
        action: MenuAction::OpenReference,
    },
    MenuEntry {
        label: "Back",
        shortcut: "Alt+←",
        action: MenuAction::Back,
    },
    MenuEntry {
        label: "Forward",
        shortcut: "Alt+→",
        action: MenuAction::Forward,
    },
    MenuEntry {
        label: "Previous Node",
        shortcut: "↑ / k (Outline)",
        action: MenuAction::Previous,
    },
    MenuEntry {
        label: "Next Node",
        shortcut: "↓ / j (Outline)",
        action: MenuAction::Next,
    },
    MenuEntry {
        label: "Parent Node",
        shortcut: "← / h (Outline)",
        action: MenuAction::Parent,
    },
    MenuEntry {
        label: "First Child Node",
        shortcut: "→ / l (Outline)",
        action: MenuAction::FirstChild,
    },
    MenuEntry {
        label: "First Node",
        shortcut: "",
        action: MenuAction::First,
    },
    MenuEntry {
        label: "Last Node",
        shortcut: "",
        action: MenuAction::Last,
    },
];

const SEARCH_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Find in Page",
        shortcut: "Ctrl+F / /",
        action: MenuAction::Find,
    },
    MenuEntry {
        label: "Find in Outline",
        shortcut: "Ctrl+G",
        action: MenuAction::FindOutline,
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
    MenuEntry {
        label: "Find Backward in Page",
        shortcut: "?",
        action: MenuAction::FindBackward,
    },
    MenuEntry {
        label: "Cycle Case: Sensitive / Ignore / Smart",
        shortcut: "Alt+C (Search)",
        action: MenuAction::SearchCase,
    },
    MenuEntry {
        label: "Toggle Regex / Literal",
        shortcut: "Ctrl+R (Search)",
        action: MenuAction::SearchSyntax,
    },
    MenuEntry {
        label: "Toggle Search Wrap",
        shortcut: "Alt+W (Search)",
        action: MenuAction::SearchWrap,
    },
];

const HELP_MENU: &[MenuEntry] = &[MenuEntry {
    label: "Keyboard Shortcuts",
    shortcut: "F1",
    action: MenuAction::Help,
}];

pub(in crate::app) const fn menu_entries(id: MenuId) -> &'static [MenuEntry] {
    match id {
        MenuId::Manual => MANUAL_MENU,
        MenuId::Edit => EDIT_MENU,
        MenuId::View => VIEW_MENU,
        MenuId::Navigate => NAVIGATE_MENU,
        MenuId::Search => SEARCH_MENU,
        MenuId::Help => HELP_MENU,
    }
}
